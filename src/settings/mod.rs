use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemSubitem};
use crate::domain::money::MoneyCents;
use crate::round::{ItemClass, PhaseWindow, RoundCategory};

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
    /// 临近同发送者消息合并（实验特性，2026-10-13；默认关）。
    #[serde(default)]
    pub merge: MergeSettings,
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
    /// 是否接受**伪造来源消息**（测试用）：入站文本本身是 `{"id":"<qq>","text":"<内容>"}`
    /// 双层 JSON 时，改判来源为内层 `id`、正文为内层 `text`。默认关闭。
    #[serde(default)]
    pub accept_forged_messages: bool,
    /// 收到消息后以指定表情回应（`set_msg_emoji_like`，如 `76` = 👍），
    /// **不发送文本消息**；`None` = 关闭。默认关闭。
    #[serde(default)]
    pub react_emoji_id: Option<u32>,
    /// **测试群**：`RoundCategory::Test` 轮次绑定的群，也是伪造解包与测试注入的唯一准入群。
    /// `None` = 不启用测试群隔离。默认关闭。
    #[serde(default)]
    pub test_group_id: Option<String>,
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
    /// 疑似排谷消息是否**优先走 LLM 切分/抽取**（关键词提取；失败回退规则）。默认 false。
    #[serde(default)]
    pub segment_claims: bool,
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

/// 临近同发送者消息合并（实验特性，2026-10-13）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeSettings {
    /// 总开关（默认关）。
    #[serde(default)]
    pub enabled: bool,
    /// 同发送者合并（唯一模式；默认 true，受 `enabled` 总闸）。
    #[serde(default = "default_true")]
    pub same_sender: bool,
}

impl Default for MergeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            same_sender: true,
        }
    }
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
    /// 轮次类别：**测试轮**只绑定测试群（`gateway.test_group_id`），
    /// **正常轮**只绑定正常群。默认 `normal`，旧轮次文件缺省即正常轮。
    #[serde(default)]
    pub category: RoundCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemConfig {
    pub item_id: String,
    pub name: String,
    /// 种类（U6；UI 用「种类」，字段沿用 `kind`）：`拼团`/`单领`/`整盒`/`特典`
    /// （兼容 `group`/`single`/`box`/`gift`，以及旧值 `split`/`single`/`gift`）。
    pub kind: String,
    /// 商品类别：`"A"`（盲盒/子项，阶段受限）或 `"B"`（固定价单领）；缺省按 B 处理。
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
    /// 整盒路由（§5）：拼团项（`kind="group"`）在「子项为空」时重定向到的整盒商品 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_item_id: Option<String>,
    #[serde(default)]
    pub subitems: Vec<SubitemConfig>,
}

/// §5 整盒路由：由目录构造「拼团项 → 整盒项」映射（仅当设置了 `box_item_id`）。
pub fn box_redirect_map(
    items: &[ItemConfig],
) -> std::collections::HashMap<crate::domain::ids::ItemId, crate::domain::ids::ItemId> {
    items
        .iter()
        .filter_map(|it| {
            it.box_item_id.as_ref().map(|b| {
                (
                    crate::domain::ids::ItemId(it.item_id.clone()),
                    crate::domain::ids::ItemId(b.clone()),
                )
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubitemConfig {
    pub subitem_id: String,
    pub name: String,
    /// 子项原价 A（分）。
    #[serde(default)]
    pub unit_price_cents: i64,
    /// 子项调价 B（分，可负）；最终价 C = A + B（§U6）。
    #[serde(default)]
    pub adjust_cents: i64,
    #[serde(default)]
    pub capacity: Option<u32>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl SubitemConfig {
    /// 最终价 C = 原价 A + 调价 B（饱和运算）。
    #[allow(dead_code)] // UI/展示与测试派生；结算经 `pricing(AdjustBy)` 复用同一 B。
    pub fn final_price_cents(&self) -> i64 {
        self.unit_price_cents.saturating_add(self.adjust_cents)
    }
}

/// 商品种类（U6）。`class` 由种类/子项自动推导（有子项 ⇒ A，无子项 ⇒ B）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCategory {
    /// 拼团（有子项）。
    Group,
    /// 单领（无子项）。
    Single,
    /// 整盒（无子项；独立种类）。
    Box,
    /// 特典（有子项）。
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

    /// 该种类是否必须带子项（U6：有子项 ⇒ 拼团/特典；无子项 ⇒ 单领/整盒）。
    pub fn requires_subitems(&self) -> bool {
        matches!(self, ItemCategory::Group | ItemCategory::Gift)
    }
}

impl ItemConfig {
    /// 种类（由 `kind` 解析；旧值 `split`/`single`/`gift` 与中文/英文种类均识别）。
    pub fn category(&self) -> Option<ItemCategory> {
        ItemCategory::parse(&self.kind)
    }

    pub fn has_subitems(&self) -> bool {
        !self.subitems.is_empty()
    }

    /// `class` 自动推导（U6）：有子项 ⇒ A（阶段受限）；无子项 ⇒ B。
    pub fn derived_class(&self) -> ItemClass {
        if self.has_subitems() {
            ItemClass::A
        } else {
            ItemClass::B
        }
    }
}

impl RoundSettings {
    /// 商品类别：**优先自动推导**（有子项 ⇒ A，无子项 ⇒ B；U6），
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
                subitems: it
                    .subitems
                    .iter()
                    .map(|v| ItemSubitem {
                        subitem_id: v.subitem_id.clone(),
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
                subitem_id: None,
                unit_price_cents: it.unit_price_cents,
            });
            for v in &it.subitems {
                out.push(crate::settlement::UnitPrice {
                    item_id: it.item_id.clone(),
                    subitem_id: Some(v.subitem_id.clone()),
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
    // 代排群外标签：`发信人(代名称)` 原样展示，不做昵称清洗。
    if raw_or_clean_nickname.contains("(代") && raw_or_clean_nickname.contains(')') {
        return Some(raw_or_clean_nickname.trim().to_string());
    }
    let (identity, display) = crate::parser::normalize::clean_nickname(raw_or_clean_nickname);
    // 反查：昵称缺失/为空、或本身就是 QQ 号（伪造来源只给 id）→ 用成员缓存名。
    if identity.trim().is_empty() || is_id_like(&identity) || identity.trim() == user_id.trim() {
        if let Some(name) = member_display_name(cfg, user_id) {
            return Some(name);
        }
    }
    if !identity.trim().is_empty() {
        return Some(identity);
    }
    if !display.trim().is_empty() {
        return Some(display);
    }
    let uid = user_id.trim();
    (!uid.is_empty()).then(|| uid.to_string())
}

fn is_id_like(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// 成员缓存（`members.cache_path`）→ `user_id -> 展示名`（`card` 优先，回退 `nickname`）。
/// 按文件 mtime 缓存，避免每条消息读盘。
type MemberNameCache = Mutex<Option<(SystemTime, HashMap<String, String>)>>;

fn member_name_map() -> &'static MemberNameCache {
    static CACHE: OnceLock<MemberNameCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn load_member_name_map(path: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return map;
    };
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) else {
        return map;
    };
    for entry in list {
        let Some(uid) = entry.get("user_id").and_then(value_to_str) else {
            continue;
        };
        let card = entry.get("card").and_then(value_to_str).unwrap_or_default();
        let nick = entry
            .get("nickname")
            .and_then(value_to_str)
            .unwrap_or_default();
        let raw = if !card.trim().is_empty() { card } else { nick };
        // 统一昵称来源=群名片；此处做同样清洗（截首个括号/空白 + NFKC + 尾标点）。
        let (name, _display) = crate::parser::normalize::clean_nickname(&raw);
        if !name.trim().is_empty() {
            map.insert(uid, name);
        }
    }
    map
}

fn value_to_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn with_member_map<T>(cfg: &AppConfig, f: impl FnOnce(&HashMap<String, String>) -> T) -> Option<T> {
    let path = cfg.members.cache_path.trim();
    if path.is_empty() {
        return None;
    }
    let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    let mut guard = member_name_map().lock().ok()?;
    let stale = match (&*guard, mtime) {
        (Some((cached, _)), Some(m)) => *cached != m,
        (None, _) => true,
        (Some(_), None) => false,
    };
    if stale {
        let map = load_member_name_map(path);
        *guard = Some((mtime.unwrap_or(SystemTime::UNIX_EPOCH), map));
    }
    guard.as_ref().map(|(_, map)| f(map))
}

/// 反查展示名：`user_id -> 成员名`（缓存）。
pub fn member_display_name(cfg: &AppConfig, user_id: &str) -> Option<String> {
    with_member_map(cfg, |map| map.get(user_id.trim()).cloned()).flatten()
}

/// 反查 `user_id`：按 `card`/`nickname` 精确或包含匹配名字（用于代排群内反查）。
pub fn member_user_id_by_name(cfg: &AppConfig, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    with_member_map(cfg, |map| {
        map.iter()
            .find(|(_, n)| n.trim() == name)
            .or_else(|| map.iter().find(|(_, n)| n.contains(name)))
            .map(|(uid, _)| uid.clone())
    })
    .flatten()
}

/// 匿名/具名代排标签 `发送者(代…)` → 发送者基名（供权限/白名单按发送者判定）。
pub fn proxy_base(id_or_nick: &str) -> Option<&str> {
    let s = id_or_nick.trim();
    let (base, _) = s.split_once("(代")?;
    let base = base.trim();
    (!base.is_empty()).then_some(base)
}

/// 优先用户判定候选串（U4）：`user_id`/昵称/归一化昵称，并加入 CN 与别名。
#[allow(dead_code)] // 供 pipeline/replay 后续接线（当前仅测试与 helper 内部使用）
pub fn priority_candidates(cfg: &AppConfig, user_id: &str, raw_nickname: &str) -> Vec<String> {
    let mut out = vec![user_id.to_string(), raw_nickname.to_string()];
    let (identity, display) = crate::parser::normalize::clean_nickname(raw_nickname);
    out.push(identity);
    out.push(display);
    // 代排标签 `发送者(代)`：权限按发送者（基名）判定。
    if let Some(base) = proxy_base(user_id) {
        out.push(base.to_string());
    }
    if let Some(base) = proxy_base(raw_nickname) {
        out.push(base.to_string());
    }
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

    /// Hold a stable active round/config while committing a guarded manual message edit.
    pub(crate) async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, AppConfig> {
        self.inner.read().await
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
