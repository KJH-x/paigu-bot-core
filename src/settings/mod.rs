use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemVariant};
use crate::domain::money::MoneyCents;
use crate::round::{ItemClass, PhaseWindow};

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
    pub kind: String,
    /// 商品类别：`"A"`（盲盒/变体，阶段受限）或 `"B"`（固定价单领）；缺省按 B 处理。
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 标价（分）。添加商品时由管理员手工确认。
    #[serde(default)]
    pub unit_price_cents: i64,
    /// 每盒件数（拼团整盒判定用）。
    #[serde(default)]
    pub box_size: Option<u32>,
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
    /// 变体标价（分）。通行证类 = `25 × pieces`（结城理/岳羽由加莉/埃癸斯 2 件=5000、虎狼丸 1 件=2500）。
    #[serde(default)]
    pub unit_price_cents: i64,
    /// 件数（精一/精二两块 = 2）。
    #[serde(default)]
    pub pieces: u32,
    #[serde(default)]
    pub capacity: Option<u32>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl RoundSettings {
    /// 商品类别；未配置或非法值按 `B`（不限制）处理。
    pub fn item_class(&self, item_id: &str) -> ItemClass {
        self.items
            .iter()
            .find(|it| it.item_id == item_id)
            .and_then(|it| it.class.as_deref())
            .and_then(ItemClass::parse)
            .unwrap_or(ItemClass::B)
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
                kind: match it.kind.as_str() {
                    "single" => ItemKind::Single,
                    "gift" => ItemKind::Gift,
                    "shipping" => ItemKind::Shipping,
                    "adjustment" => ItemKind::Adjustment,
                    _ => ItemKind::Split,
                },
                unit_price: MoneyCents(it.unit_price_cents),
                box_size: it.box_size,
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
}

fn default_pull_at() -> String {
    "19:00".to_string()
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
        let cfg = if path.exists() {
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
        if cfg.revision <= guard.revision {
            cfg.revision = guard.revision + 1;
        }
        let revision = cfg.revision;
        *guard = cfg;
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
