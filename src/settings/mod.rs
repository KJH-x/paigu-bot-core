use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemVariant};
use crate::domain::money::MoneyCents;
use crate::round::{ItemClass, PhaseWindow};

/// 轮次库（`data/rounds/<round_id>.json`）读写与激活解析（U3）。
pub mod rounds;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub revision: u64,
    pub gateway: GatewayConfig,
    pub llm: LlmSettings,
    pub round: RoundSettings,
    pub display: DisplaySettings,
    pub members: MembersSettings,
    #[serde(default)]
    pub settlement: crate::settlement::SettlementConfig,
    /// 当前激活轮次 id（U3）：指向 `data/rounds/<id>.json`。
    #[serde(default)]
    pub active_round_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub bind: String,
    #[serde(default)]
    pub whitelist_groups: Vec<String>,
    /// 成员白名单（user_id / 身份 / 显示名任一命中即放行）；空 = 不限制。
    #[serde(default)]
    pub whitelist_members: Vec<String>,
    #[serde(default = "default_heartbeat")]
    pub heartbeat_secs: u64,
    #[serde(default)]
    pub reply_enabled: bool,
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    /// 是否允许管理员斜杠命令**落地执行**（D-1）；关闭时仅记录。
    #[serde(default)]
    pub admin_commands_enabled: bool,
}

fn default_heartbeat() -> u64 {
    15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_true")]
    pub fallback_to_rules: bool,
    #[serde(default)]
    pub prompt_template: String,
}

fn default_true() -> bool {
    true
}
fn default_timeout() -> u64 {
    60
}
fn default_max_tokens() -> u32 {
    2048
}
fn default_temperature() -> f32 {
    0.1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityWindow {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundSettings {
    pub round_id: String,
    pub title: String,
    pub group_id: String,
    #[serde(default)]
    pub priority_users: Vec<String>,
    #[serde(default)]
    pub priority_window: Option<PriorityWindow>,
    /// 阶段时间窗（可配）；空 = 不限制。
    #[serde(default)]
    pub phases: Vec<PhaseWindow>,
    #[serde(default)]
    pub items: Vec<ItemConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemConfig {
    pub item_id: String,
    pub name: String,
    /// 种类（U6；UI 用「种类」，字段沿用 `kind`）：`拼团`/`单领`/`整盒`/`特典`
    /// （兼容 `group`/`single`/`box`/`gift`，以及旧值 `split`/`single`/`gift`）。
    pub kind: String,
    /// 商品类别：`"A"`（盲盒/变体，阶段受限）或 `"B"`（固定价单领）；缺省按 B 处理。
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 标价（分）。添加商品时由管理员手工确认（§U6：商品级只设原价，无调价）。
    #[serde(default)]
    pub unit_price_cents: i64,
    /// 单领上限（`kind="single"`）。
    #[serde(default)]
    pub max_quantity: Option<u32>,
    #[serde(default)]
    pub variants: Vec<VariantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantConfig {
    pub variant_id: String,
    pub name: String,
    /// 变体原价 A（分）。
    #[serde(default)]
    pub unit_price_cents: i64,
    /// 变体调价 B（分，可负）；最终价 C = A + B（§U6）。
    #[serde(default)]
    pub adjust_cents: i64,
    #[serde(default)]
    pub capacity: Option<u32>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl VariantConfig {
    /// 最终价 C = 原价 A + 调价 B（饱和运算）。
    #[allow(dead_code)] // UI/展示与测试派生；结算经 `pricing(AdjustBy)` 复用同一 B。
    pub fn final_price_cents(&self) -> i64 {
        self.unit_price_cents.saturating_add(self.adjust_cents)
    }
}

/// 商品种类（U6）。`class` 由种类/变体自动推导（有变体 ⇒ A，无变体 ⇒ B）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCategory {
    /// 拼团（有变体）。
    Group,
    /// 单领（无变体）。
    Single,
    /// 整盒（无变体；独立种类）。
    Box,
    /// 特典（有变体）。
    Gift,
}

impl ItemCategory {
    /// 解析种类（中英兼容）；未识别返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "拼团" | "group" | "split" => Some(ItemCategory::Group),
            "单领" | "single" => Some(ItemCategory::Single),
            "整盒" | "box" | "whole_box" | "fullbox" => Some(ItemCategory::Box),
            "特典" | "gift" => Some(ItemCategory::Gift),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ItemCategory::Group => "group",
            ItemCategory::Single => "single",
            ItemCategory::Box => "box",
            ItemCategory::Gift => "gift",
        }
    }

    /// 该种类是否必须带变体（U6：有变体 ⇒ 拼团/特典；无变体 ⇒ 单领/整盒）。
    pub fn requires_variants(&self) -> bool {
        matches!(self, ItemCategory::Group | ItemCategory::Gift)
    }
}

impl ItemConfig {
    /// 种类（由 `kind` 解析；旧值 `split`/`single`/`gift` 与中文/英文种类均识别）。
    pub fn category(&self) -> Option<ItemCategory> {
        ItemCategory::parse(&self.kind)
    }

    pub fn has_variants(&self) -> bool {
        !self.variants.is_empty()
    }

    /// `class` 自动推导（U6）：有变体 ⇒ A（阶段受限）；无变体 ⇒ B。
    pub fn derived_class(&self) -> ItemClass {
        if self.has_variants() {
            ItemClass::A
        } else {
            ItemClass::B
        }
    }
}

impl RoundSettings {
    /// 商品类别：**优先自动推导**（有变体 ⇒ A，无变体 ⇒ B；U6），
    /// 显式 `class` 存在且合法时可覆盖推导；商品未配置时按 `B`（不限制）处理。
    pub fn item_class(&self, item_id: &str) -> ItemClass {
        match self.items.iter().find(|it| it.item_id == item_id) {
            Some(it) => it
                .class
                .as_deref()
                .and_then(ItemClass::parse)
                .unwrap_or_else(|| it.derived_class()),
            None => ItemClass::B,
        }
    }

    pub fn to_items(&self) -> Vec<Item> {
        let round_id = RoundId(self.round_id.clone());
        self.items
            .iter()
            .enumerate()
            .map(|(idx, it)| Item {
                item_id: ItemId(it.item_id.clone()),
                round_id: round_id.clone(),
                name: it.name.clone(),
                kind: match it.category() {
                    Some(ItemCategory::Single) => ItemKind::Single,
                    // §U8：整盒是独立种类，引擎默认路由到单领队列。
                    Some(ItemCategory::Box) => ItemKind::WholeBox,
                    Some(ItemCategory::Gift) => ItemKind::Gift,
                    Some(ItemCategory::Group) => ItemKind::Split,
                    // 兼容旧/非目录值：shipping/adjustment 保留，其余按拼团。
                    None => match it.kind.as_str() {
                        "shipping" => ItemKind::Shipping,
                        "adjustment" => ItemKind::Adjustment,
                        _ => ItemKind::Split,
                    },
                },
                unit_price: MoneyCents(it.unit_price_cents),
                // §U6：整盒改由「种类=整盒」表达，配置侧不再有 `box_size`。
                box_size: None,
                max_quantity: it.max_quantity,
                is_blind: false,
                is_proxy_card: false,
                aliases: it.aliases.clone(),
                sort_order: idx as i32,
                metadata: serde_json::Value::Null,
                variants: it
                    .variants
                    .iter()
                    .map(|v| ItemVariant {
                        variant_id: v.variant_id.clone(),
                        name: v.name.clone(),
                        unit_price: MoneyCents(v.unit_price_cents),
                        capacity: v.capacity,
                        aliases: v.aliases.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// 标价表（商品目录 → `UnitPrice`），供结算/planner 取标价（替代请求级 `prices`）。
    pub fn to_unit_prices(&self) -> Vec<crate::settlement::UnitPrice> {
        let mut out = Vec::new();
        for it in &self.items {
            out.push(crate::settlement::UnitPrice {
                item_id: it.item_id.clone(),
                variant_id: None,
                unit_price_cents: it.unit_price_cents,
            });
            for v in &it.variants {
                out.push(crate::settlement::UnitPrice {
                    item_id: it.item_id.clone(),
                    variant_id: Some(v.variant_id.clone()),
                    unit_price_cents: v.unit_price_cents,
                });
            }
        }
        out
    }
}

/// 是否落在优先时段 `[start_ms, end_ms)` 内（end 独占）。
pub fn in_priority_window(window: Option<(i64, i64)>, timestamp_ms: i64) -> bool {
    matches!(window, Some((start, end)) if timestamp_ms >= start && timestamp_ms < end)
}

/// 任一候选串（user_id/昵称/身份/显示）命中预存(购物金)名单即视为预存用户。
pub fn is_priority_user(priority_users: &[String], candidates: &[&str]) -> bool {
    priority_users.iter().any(|u| {
        let u = u.trim();
        candidates.contains(&u)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    #[serde(default = "default_refresh")]
    pub refresh_ms: u64,
    #[serde(default = "default_source")]
    pub data_source: String,
    #[serde(default)]
    pub remote_base_url: String,
}

fn default_refresh() -> u64 {
    5000
}
fn default_source() -> String {
    "local".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembersSettings {
    pub group_id: String,
    pub cache_path: String,
    #[serde(default = "default_pull_at")]
    pub daily_pull_at: String,
    /// 成员具体名（CN）覆盖：`user_id → CN/别名`（U4）。
    #[serde(default)]
    pub cn_overrides: Vec<CnOverride>,
}

fn default_pull_at() -> String {
    "19:00".to_string()
}

/// 成员具体名（CN）：绑定 QQ(user_id)，可带别名（U4）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CnOverride {
    pub user_id: String,
    pub cn: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// 解析展示用人名（U4）：**CN → 归一化昵称 → user_id**。
///
/// CN 按 `user_id` 绑定；无覆盖时回退到 `clean_nickname` 的 `identity`（归一化昵称），
/// 再回退 `user_id`；三者皆空返回 `None`。
pub fn resolve_cn(cfg: &AppConfig, user_id: &str, raw_or_clean_nickname: &str) -> Option<String> {
    if let Some(over) = cfg
        .members
        .cn_overrides
        .iter()
        .find(|o| o.user_id == user_id)
    {
        let cn = over.cn.trim();
        if !cn.is_empty() {
            return Some(cn.to_string());
        }
    }
    let (identity, display) = crate::parser::normalize::clean_nickname(raw_or_clean_nickname);
    if !identity.trim().is_empty() {
        return Some(identity);
    }
    if !display.trim().is_empty() {
        return Some(display);
    }
    let uid = user_id.trim();
    (!uid.is_empty()).then(|| uid.to_string())
}

/// 优先用户判定候选串（U4）：`user_id`/昵称/归一化昵称，并加入 CN 与别名。
#[allow(dead_code)] // 供 pipeline/replay 后续接线（当前仅测试与 helper 内部使用）
pub fn priority_candidates(cfg: &AppConfig, user_id: &str, raw_nickname: &str) -> Vec<String> {
    let mut out = vec![user_id.to_string(), raw_nickname.to_string()];
    let (identity, display) = crate::parser::normalize::clean_nickname(raw_nickname);
    out.push(identity);
    out.push(display);
    if let Some(over) = cfg
        .members
        .cn_overrides
        .iter()
        .find(|o| o.user_id == user_id)
    {
        out.push(over.cn.clone());
        out.extend(over.aliases.iter().cloned());
    }
    out.retain(|s| !s.trim().is_empty());
    out.sort();
    out.dedup();
    out
}

/// 优先用户判定（U4 版）：候选集含 CN/别名，避免替换 `policy::is_priority` 的调用点。
#[allow(dead_code)] // 供 pipeline/replay 后续接线
pub fn is_priority_with_cn(cfg: &AppConfig, user_id: &str, raw_nickname: &str) -> bool {
    let owned = priority_candidates(cfg, user_id, raw_nickname);
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    is_priority_user(&cfg.round.priority_users, &refs)
}

pub fn default_config() -> AppConfig {
    serde_json::from_str(include_str!("../../config.example.json"))
        .expect("config.example.json must be valid")
}

#[derive(Debug)]
pub enum ConfigError {
    StaleRevision { expected: u64, actual: u64 },
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::StaleRevision { expected, actual } => {
                write!(
                    f,
                    "stale config revision: expected {expected}, actual {actual}"
                )
            }
            ConfigError::Io(e) => write!(f, "io: {e}"),
            ConfigError::Json(e) => write!(f, "json: {e}"),
        }
    }
}
impl std::error::Error for ConfigError {}

pub struct ConfigStore {
    path: PathBuf,
    inner: RwLock<AppConfig>,
}

impl ConfigStore {
    pub fn load(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let mut cfg = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read config {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("parse config {}", path.display()))?
        } else {
            let cfg = default_config();
            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("create config dir {} failed: {e}", parent.display());
                }
            }
            if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&cfg)?) {
                tracing::warn!("write default config {} failed: {e}", path.display());
            }
            cfg
        };
        // U3：启动解析激活轮次（存在则覆盖 round；缺失则落盘当前 round）。
        if rounds::resolve_active(&mut cfg) {
            if let Ok(raw) = serde_json::to_string_pretty(&cfg) {
                if let Err(e) = std::fs::write(&path, raw) {
                    tracing::warn!("write config {} failed: {e}", path.display());
                }
            }
        }
        Ok(Self {
            path,
            inner: RwLock::new(cfg),
        })
    }

    pub async fn get(&self) -> AppConfig {
        self.inner.read().await.clone()
    }

    pub async fn put(&self, mut cfg: AppConfig, expected: u64) -> Result<u64, ConfigError> {
        let mut guard = self.inner.write().await;
        if guard.revision != expected {
            return Err(ConfigError::StaleRevision {
                expected,
                actual: guard.revision,
            });
        }
        cfg.revision = expected + 1;
        // U3：任何改变 round 的写入都同步落盘到 data/rounds/<round_id>.json。
        if rounds::is_valid_round_id(&cfg.round.round_id) {
            cfg.active_round_id = Some(cfg.round.round_id.clone());
            if let Err(e) = rounds::write_round(&cfg.round) {
                tracing::warn!("轮次落盘失败: {e}");
            }
        } else {
            tracing::warn!("非法 round_id，跳过轮次落盘: {}", cfg.round.round_id);
        }
        let revision = cfg.revision;
        let raw = serde_json::to_string_pretty(&cfg).map_err(ConfigError::Json)?;
        std::fs::write(&self.path, raw).map_err(ConfigError::Io)?;
        *guard = cfg;
        Ok(revision)
    }

    pub async fn reload(&self) -> anyhow::Result<u64> {
        let raw = std::fs::read_to_string(&self.path)?;
        let mut cfg: AppConfig = serde_json::from_str(&raw)?;
        let mut guard = self.inner.write().await;
        // U3：热载同样解析激活轮次（以轮次文件为准）。
        let persist = rounds::resolve_active(&mut cfg);
        if cfg.revision <= guard.revision {
            cfg.revision = guard.revision + 1;
        }
        let revision = cfg.revision;
        let snapshot = cfg.clone();
        *guard = cfg;
        drop(guard);
        if persist {
            if let Ok(text) = serde_json::to_string_pretty(&snapshot) {
                let _ = std::fs::write(&self.path, text);
            }
        }
        Ok(revision)
    }

    /// 监听文件变更并热载（notify，带 300ms 去抖）。
    pub fn spawn_watch(self: &Arc<Self>) {
        use notify::{Config as NConfig, RecursiveMode, Watcher};
        let store = self.clone();
        let path = self.path.clone();
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = match notify::RecommendedWatcher::new(tx, NConfig::default()) {
                Ok(w) => w,
                Err(_) => return,
            };
            if watcher.watch(&path, RecursiveMode::NonRecursive).is_err() {
                return;
            }
            let mut last = std::time::Instant::now();
            while rx.recv().is_ok() {
                if last.elapsed() < std::time::Duration::from_millis(300) {
                    continue;
                }
                last = std::time::Instant::now();
                let rt = tokio::runtime::Handle::try_current();
                if let Ok(rt) = rt {
                    let store = store.clone();
                    rt.spawn(async move {
                        if let Err(e) = store.reload().await {
                            tracing::warn!("config reload failed: {e}");
                        }
                    });
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
