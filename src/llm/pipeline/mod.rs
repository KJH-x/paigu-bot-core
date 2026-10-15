use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::bus::{EventSink, IncomingEvent, PipelineOutcome};
use crate::domain::event::DomainEvent;
use crate::domain::ids::{RoundId, UserId};
use crate::domain::item::RoundContext;
use crate::engine::replay::{describe_event, rebuild_allocation_snapshot};
use crate::messages::{MessageLog, MessageRecord as LogMessageRecord};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::policy;
use crate::parser::coalesce::{Classification, GroupIntent};
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidateContext, ValidationOutcome};
use crate::replay::session::{ReplayOverrides, ReplayResult};
use crate::settings::{AppConfig, ConfigStore};

use super::client::{LlmClient, OpenAiClient};

mod admin;
mod export;
mod llm_parse;
mod replay_view;
mod state;

#[cfg(test)]
mod tests;

use admin::{cancel_for_modify, priority_eligibility};
use llm_parse::BareSubitemOutcome;
use state::{MessageRecord, State};

const RULE_CONFIDENCE: f32 = 0.9;
const CONFIDENCE_THRESHOLD: f32 = 0.65;

/// 测试用：识别“伪造来源”双层 JSON 封装。NC 的完整文本本身为
/// `{"id":"<qq>","text":"<内容>"}`（可选 `nickname` / `is_admin`）；命中则把来源改为内层
/// `id`、正文改为内层 `text`，其余字段（群 / 时间戳 / 消息 id / 原始事件）沿用外层。
/// 仅在 `gateway.accept_forged_messages` 开启时生效。
fn unwrap_forged_envelope(ev: IncomingEvent) -> IncomingEvent {
    let Some(envelope) = parse_forged_envelope(&ev.text) else {
        return ev;
    };
    IncomingEvent {
        group_id: ev.group_id,
        user_id: envelope.id.clone(),
        nickname: envelope.nickname.unwrap_or(envelope.id),
        message_id: ev.message_id,
        text: envelope.text,
        timestamp_ms: envelope.timestamp_ms.unwrap_or(ev.timestamp_ms),
        is_admin: envelope.is_admin.unwrap_or(false),
        sender_user_id: None,
        sender_nickname: None,
        raw: ev.raw,
    }
}

/// NC 入口隔离（R3/R4）：仅当开启伪造解包、当前轮为**测试轮**、且消息来自**测试群**时，
/// 才把伪造信封解包；其余情况原样透传。
/// 真实消息处理链路（`Pipeline::process`）不再感知伪造 —— 隔离只管理来自 NC 的消息。
#[allow(dead_code)]
pub(crate) fn gate_nc(cfg: &AppConfig, ev: IncomingEvent) -> IncomingEvent {
    let is_test_round = cfg.round.category.is_test();
    let from_test_group = cfg.gateway.test_group_id.as_deref() == Some(ev.group_id.as_str());
    if cfg.gateway.accept_forged_messages && is_test_round && from_test_group {
        unwrap_forged_envelope(ev)
    } else {
        ev
    }
}

/// NC 入口隔离（多消息版）：支持伪造**批次**——文本为 JSON 数组 `[{id,text,...},…]`
/// 时解包为多条（每条 `message_id` 追加 `#i`）；单对象则单条；非测试群/未开启原样单条。
pub(crate) fn gate_nc_multi(cfg: &AppConfig, ev: IncomingEvent) -> Vec<IncomingEvent> {
    let is_test_round = cfg.round.category.is_test();
    let from_test_group = cfg.gateway.test_group_id.as_deref() == Some(ev.group_id.as_str());
    if !(cfg.gateway.accept_forged_messages && is_test_round && from_test_group) {
        return vec![ev];
    }
    if let Some(list) = parse_forged_list(&ev.text) {
        return list
            .into_iter()
            .enumerate()
            .map(|(i, e)| IncomingEvent {
                group_id: ev.group_id.clone(),
                user_id: e.id.clone(),
                nickname: e.nickname.unwrap_or_else(|| e.id.clone()),
                message_id: format!("{}#{}", ev.message_id, i),
                text: e.text,
                timestamp_ms: e.timestamp_ms.unwrap_or(ev.timestamp_ms),
                is_admin: e.is_admin.unwrap_or(false),
                sender_user_id: None,
                sender_nickname: None,
                raw: None,
            })
            .collect();
    }
    vec![unwrap_forged_envelope(ev)]
}

struct ForgedEnvelope {
    id: String,
    text: String,
    nickname: Option<String>,
    is_admin: Option<bool>,
    /// 覆盖消息时间（毫秒）；缺省用外层时间。支持整数毫秒 / 纯数字串 /
    /// RFC3339（`2026-10-12T22:30:00+08:00`）/ 本地 `YYYY-MM-DD HH:MM[:SS]`。
    timestamp_ms: Option<i64>,
}

fn parse_forged_envelope(text: &str) -> Option<ForgedEnvelope> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    parse_forged_value(&value)
}

/// 解析单个信封对象 `{id|user_id, text, timestamp?, nickname?, is_admin?}`。
fn parse_forged_value(value: &Value) -> Option<ForgedEnvelope> {
    let obj = value.as_object()?;
    let id = obj
        .get("id")
        .or_else(|| obj.get("user_id"))
        .and_then(scalar_to_string)?;
    let text = obj.get("text").and_then(Value::as_str)?.to_string();
    let nickname = obj
        .get("nickname")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let is_admin = obj.get("is_admin").and_then(Value::as_bool);
    let timestamp_ms = obj
        .get("timestamp")
        .or_else(|| obj.get("timestamp_ms"))
        .and_then(parse_timestamp_ms);
    Some(ForgedEnvelope {
        id,
        text,
        nickname,
        is_admin,
        timestamp_ms,
    })
}

/// 伪造**批次**：文本为 JSON 数组 `[{id,text,...},…]` → 多条信封；否则 `None`。
fn parse_forged_list(text: &str) -> Option<Vec<ForgedEnvelope>> {
    let trimmed = text.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let arr = value.as_array()?;
    let out: Vec<ForgedEnvelope> = arr.iter().filter_map(parse_forged_value).collect();
    (!out.is_empty()).then_some(out)
}

/// 解析时间：整数/数值 ⇒ 毫秒；字符串 ⇒ 纯数字毫秒 / RFC3339 / 本地 `YYYY-MM-DD HH:MM[:SS]`。
pub(crate) fn parse_timestamp_ms(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            if let Ok(ms) = s.parse::<i64>() {
                return Some(ms);
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Some(dt.timestamp_millis());
            }
            for fmt in [
                "%Y-%m-%d %H:%M:%S",
                "%Y-%m-%d %H:%M",
                "%Y/%m/%d %H:%M:%S",
                "%Y/%m/%d %H:%M",
            ] {
                if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                    if let Some(dt) = chrono::Local.from_local_datetime(&naive).single() {
                        return Some(dt.timestamp_millis());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn scalar_to_string(value: &Value) -> Option<String> {
    let s = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

/// 代排分段：识别 `代<名称>排[:]` 标记；有标记时返回分段 `(target, text)`。
/// `target=None` 为发信人段，`Some(名称)` 为被代理人段（标记本身被剔除）。
fn proxy_segments(text: &str) -> Option<Vec<(Option<String>, String)>> {
    let markers = find_proxy_markers(text);
    if markers.is_empty() {
        return None;
    }
    let mut segs: Vec<(Option<String>, String)> = Vec::new();
    let mut pos = 0usize;
    let mut current: Option<String> = None;
    for (name, start, end) in markers {
        if start > pos {
            segs.push((current.clone(), text[pos..start].to_string()));
        }
        current = Some(name);
        pos = end;
    }
    if pos < text.len() {
        segs.push((current, text[pos..].to_string()));
    }
    segs.retain(|(_, s)| !s.trim().is_empty());
    (!segs.is_empty()).then_some(segs)
}

fn find_proxy_markers(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 == '代' {
            let mut j = i + 1;
            while j < chars.len()
                && !matches!(
                    chars[j].1,
                    '排' | ' ' | ':' | '：' | '\n' | '\t' | '，' | ','
                )
                && j - i <= 24
            {
                j += 1;
            }
            if j < chars.len() && chars[j].1 == '排' && j > i + 1 {
                let name: String = chars[i + 1..j].iter().map(|(_, c)| *c).collect();
                let start = chars[i].0;
                let mut end = chars[j].0 + chars[j].1.len_utf8();
                if let Some(c) = text[end..].chars().next() {
                    if c == ':' || c == '：' {
                        end += c.len_utf8();
                    }
                }
                out.push((name, start, end));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 是否疑似排谷/撤销/改单消息（决定是否走 LLM 切分）；纯闲聊/表情不触发 LLM。
fn looks_like_order(text: &str, rule: &crate::parser::parsed_event::ParsedMessage) -> bool {
    !rule.items.is_empty()
        || rule.intent != ParsedIntent::Unknown
        || text.contains('排')
        || text.contains('撤')
        || text.contains('改')
        || text.contains('代')
        || text.contains("特典")
}

pub struct Pipeline {
    cfg: Arc<ConfigStore>,
    llm: Arc<dyn LlmClient>,
    state: Mutex<State>,
    messages: Arc<MessageLog>,
    /// 当前"打开"的合并组（同发送者、相邻、同意图）。
    merge_open: Mutex<Option<OpenGroup>>,
    /// 串行化合并摄取（避免并发 `recompute` 的读改写竞态）。
    ingest_lock: Mutex<()>,
}

/// 合并组状态（实验特性 2026-10-13）。
struct OpenGroup {
    round_id: String,
    revision: u64,
    user_id: String,
    intent: GroupIntent,
    events: Vec<IncomingEvent>,
}

impl Pipeline {
    #[allow(dead_code)]
    pub fn new(cfg: Arc<ConfigStore>) -> Arc<Self> {
        Self::new_with_client(cfg, Arc::new(OpenAiClient::new()))
    }

    #[allow(dead_code)]
    pub fn new_with_client(cfg: Arc<ConfigStore>, llm: Arc<dyn LlmClient>) -> Arc<Self> {
        Self::build(cfg, llm, None)
    }

    /// 指定消息日志目录（测试隔离用）；生产默认 `PAIGU_MESSAGES_DIR` / `data/messages`。
    #[cfg(test)]
    pub fn new_with_client_dir(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages_dir: impl Into<std::path::PathBuf>,
    ) -> Arc<Self> {
        let dir: std::path::PathBuf = messages_dir.into();
        let log = MessageLog::new(dir.clone(), dir.join("events"));
        Self::build(cfg, llm, Some(log))
    }

    /// 注入共享消息日志（T-05）。
    pub fn new_with_messages(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages: Arc<MessageLog>,
    ) -> Arc<Self> {
        Self::build(cfg, llm, Some(messages))
    }

    fn build(
        cfg: Arc<ConfigStore>,
        llm: Arc<dyn LlmClient>,
        messages: Option<Arc<MessageLog>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            llm,
            state: Mutex::new(State::default()),
            messages: messages.unwrap_or_else(MessageLog::from_env),
            merge_open: Mutex::new(None),
            ingest_lock: Mutex::new(()),
        })
    }

    /// 共享消息日志句柄（Gateway / API 复用同一实例）。
    #[allow(dead_code)]
    pub fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
    }

    /// 注入式 LLM 客户端句柄（API 的别名建议等处复用；便于 Mock 测试）。
    pub fn llm_client(&self) -> Arc<dyn LlmClient> {
        self.llm.clone()
    }

    /// 代排（T-29）：把一条消息按 `代<名称>排:` 分段，逐段复用 `process`。
    /// 发信人段归属发信人；被代理人段经成员缓存反查 QQ（群内）→ 归属被代理人，
    /// 查不到（群外）→ 记为 `发信人(代名称)`。
    async fn process_proxy_segments(
        &self,
        ev: &IncomingEvent,
        segments: Vec<(Option<String>, String)>,
    ) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        let sender_name = crate::settings::resolve_cn(&cfg, &ev.user_id, &ev.nickname)
            .unwrap_or_else(|| ev.user_id.clone());
        let mut statuses: Vec<String> = Vec::new();
        let mut details: Vec<String> = Vec::new();
        let mut version = 0i64;
        let mut snapshot: Option<Value> = None;
        for (idx, (target, text)) in segments.into_iter().enumerate() {
            let mut seg = ev.clone();
            seg.text = text;
            seg.message_id = format!("{}#proxy{}", ev.message_id, idx);
            seg.raw = None;
            if let Some(name) = target {
                match crate::settings::member_user_id_by_name(&cfg, &name) {
                    Some(uid) => {
                        seg.user_id = uid;
                        seg.nickname = String::new();
                    }
                    None => {
                        seg.user_id = format!("{sender_name}(代{name})");
                        seg.nickname = String::new();
                    }
                }
            }
            let outcome = Box::pin(self.process(seg)).await;
            statuses.push(outcome.status.clone());
            if !outcome.detail.is_empty() {
                details.push(format!("{} [{}]", outcome.detail, outcome.status));
            }
            version = outcome.version;
            if outcome.snapshot.is_some() {
                snapshot = outcome.snapshot;
            }
        }
        let status = if statuses.iter().any(|s| s == "Applied") {
            "Applied".to_string()
        } else {
            statuses
                .last()
                .cloned()
                .unwrap_or_else(|| "Ignored".to_string())
        };
        PipelineOutcome {
            status,
            detail: format!("代排：{}", details.join("；")),
            reply: None,
            version,
            snapshot,
        }
    }

    pub async fn process(&self, ev: IncomingEvent) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        // C-3：保留成员原始事件（WS 路径由 Gateway 落盘；此处覆盖 API/脚本注入路径）
        if let Some(raw) = &ev.raw {
            let round_id = cfg.round.round_id.clone();
            if let Err(e) = self.messages.append_raw_event(&round_id, raw).await {
                warn!("原始事件写入失败: {e}");
            }
        }
        // 代排：`代<名称>排: …` → 按段归属（发信人段 + 被代理人段），逐段复用本流程。
        if let Some(segments) = proxy_segments(&ev.text) {
            return self.process_proxy_segments(&ev, segments).await;
        }
        let (identity, _display_raw) = crate::parser::normalize::clean_nickname(&ev.nickname);
        // 消息流展示名与排位表同源（CN → 归一化昵称 → 成员反查 → user_id）。
        let display = crate::settings::resolve_cn(&cfg, &ev.user_id, &ev.nickname)
            .unwrap_or_else(|| ev.user_id.clone());

        let seq = {
            let mut state = self.state.lock().await;
            let key = format!("{}::{}", ev.group_id, ev.message_id);
            if !state.seen.insert(key) {
                let detail = "重复 message_id，已按幂等忽略";
                // Duplicate inputs do not mutate the board, but they are still
                // distinct inbound messages and need a unique UI/log sequence.
                state.seq += 1;
                let seq = state.seq;
                state.messages.push(MessageRecord {
                    seq,
                    user_id: ev.user_id.clone(),
                    display: display.clone(),
                    text: ev.text.clone(),
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                });
                let version = state.version;
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .and_then(|s| serde_json::to_value(s).ok());
                drop(state);
                self.persist(&ev, "Duplicate", detail, "message").await;
                return PipelineOutcome {
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                    reply: None,
                    version,
                    snapshot,
                };
            }
            state.seq += 1;
            state.display.insert(ev.user_id.clone(), display.clone());
            state.identity.insert(ev.user_id.clone(), identity.clone());
            state.seq
        };

        let text = ev.text.trim().to_string();

        if text.is_empty() {
            return self
                .finish(&ev, &display, seq, "Ignored", "空消息", None)
                .await;
        }

        if text.starts_with('/') {
            if !ev.is_admin {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Rejected",
                        "非管理员斜杠命令",
                        Some("此命令仅管理员可用。".to_string()),
                    )
                    .await;
            }
            let cmd = text.trim_start_matches('/').trim().to_string();
            if !cfg.gateway.admin_commands_enabled {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Applied",
                        "管理员命令已记录（执行开关关闭）",
                        Some("管理员命令已记录（执行开关关闭）".to_string()),
                    )
                    .await;
            }
            return self.run_admin_command(&ev, &display, seq, &cmd).await;
        }

        let items = cfg.round.to_items();
        let rule = RuleParser::parse(&text, &items, ev.is_admin);

        let parsed = if cfg.llm.enabled && cfg.llm.segment_claims && looks_like_order(&text, &rule)
        {
            // 切分改由 LLM 自动完成（关键词提取）；失败/超时回退规则解析器。
            match self.llm_parse(&cfg, &ev).await {
                Ok(parsed) => parsed,
                Err(e) => {
                    warn!("LLM 切分失败，回退规则：{e}");
                    rule
                }
            }
        } else if rule.confidence >= RULE_CONFIDENCE && rule.intent != ParsedIntent::Unknown {
            rule
        } else if cfg.llm.enabled {
            match self.llm_parse(&cfg, &ev).await {
                Ok(parsed) => parsed,
                Err(e) => {
                    if cfg.llm.fallback_to_rules {
                        warn!("LLM 解析失败，回退规则解析器: {e}");
                        rule
                    } else {
                        return self
                            .finish(
                                &ev,
                                &display,
                                seq,
                                "Rejected",
                                "没识别成功",
                                Some("没识别成功".to_string()),
                            )
                            .await;
                    }
                }
            }
        } else if cfg.llm.fallback_to_rules {
            rule
        } else {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "LLM 未启用",
                    Some("没识别成功".to_string()),
                )
                .await;
        };

        let is_modify = parsed.intent == ParsedIntent::Modify;
        let mut parsed = parsed;
        if is_modify {
            // 改单（D-2）：按新数量重排；应用时先撤销本人该商品的既有认购
            parsed.intent = ParsedIntent::Claim;
        }

        if parsed.intent == ParsedIntent::Claim
            && parsed.items.is_empty()
            && parsed.ambiguous_parts.is_empty()
        {
            return self
                .finish(&ev, &display, seq, "Ignored", "未解析出商品", None)
                .await;
        }

        let round_id = RoundId(cfg.round.round_id.clone());
        let round_contexts = vec![RoundContext {
            round_id: round_id.clone(),
            title: cfg.round.title.clone(),
            items: items.clone(),
        }];
        let now = DateTime::<Utc>::from_timestamp_millis(ev.timestamp_ms).unwrap_or_else(Utc::now);

        // §U7 first-match：只报角色名/子项名时按目录顺序取第一个可拼团商品；
        // 全部 fail → LLM 澄清（成功后写 ParseOverride 事件）。
        if parsed.intent == ParsedIntent::Claim {
            match self
                .resolve_bare_subitem_claims(&cfg, &ev, parsed, &round_contexts, now, seq)
                .await
            {
                BareSubitemOutcome::Resolved(p) => parsed = p,
                BareSubitemOutcome::FallbackIgnore => {
                    return self
                        .finish(
                            &ev,
                            &display,
                            seq,
                            "Ignored",
                            "商品未能 first-match，未记录",
                            None,
                        )
                        .await;
                }
                BareSubitemOutcome::FallbackReject => {
                    return self
                        .finish(
                            &ev,
                            &display,
                            seq,
                            "Rejected",
                            "没识别成功",
                            Some("没识别成功".to_string()),
                        )
                        .await;
                }
            }
        }

        let user_id = UserId(ev.user_id.clone());
        let validator = EventValidator::new(CONFIDENCE_THRESHOLD);
        let box_map = crate::settings::box_redirect_map(&cfg.round.items);

        let event = match validator
            .validate(
                parsed,
                ValidateContext {
                    user_id: &user_id,
                    group_id: &ev.group_id,
                    raw_message_id: Some(ev.message_id.clone()),
                    active_rounds: &round_contexts,
                    box_map: &box_map,
                    now,
                    sequence: seq,
                },
            )
            .await
        {
            Ok(ValidationOutcome::Ok(event)) => *event,
            Ok(ValidationOutcome::NeedConfirm(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "NeedConfirm", "需要确认", reply)
                    .await;
            }
            Ok(ValidationOutcome::Reject(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "Rejected", "校验拒绝", reply)
                    .await;
            }
            Ok(ValidationOutcome::Ignore) => {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Ignored",
                        "无法识别为排谷/撤销意图",
                        None,
                    )
                    .await;
            }
            Err(e) => {
                return self
                    .finish(&ev, &display, seq, "Error", &format!("处理失败: {e}"), None)
                    .await;
            }
        };

        // U4：优先用户候选含 CN/别名；匿名代排按**发送者**判定（R8）。
        let perm_uid = ev.sender_user_id.as_deref().unwrap_or(&ev.user_id);
        let perm_nick = ev.sender_nickname.as_deref().unwrap_or(&ev.nickname);
        let is_priority = policy::is_priority(&cfg, perm_uid, perm_nick);
        if policy::in_priority_at(&cfg, ev.timestamp_ms) && !is_priority {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "优先时段仅限预存(购物金)用户",
                    Some("优先时段仅限预存(购物金)用户，请求已拒绝".to_string()),
                )
                .await;
        }

        if !cfg.round.phases.is_empty() {
            if let Some(phase) = policy::phase_at(&cfg.round.phases, ev.timestamp_ms) {
                if let Some(detail) = policy::phase_rejection(&cfg, &event, phase, is_priority) {
                    let reply = detail.clone();
                    return self
                        .finish(&ev, &display, seq, "Rejected", &detail, Some(reply))
                        .await;
                }
            }
        }

        {
            let st = self.state.lock().await;
            if st.locked {
                drop(st);
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Rejected",
                        "已锁定，不再接受排/撤/改",
                        Some("已锁定，不再接受排/撤/改".to_string()),
                    )
                    .await;
            }
        }

        let mut detail = describe_event(&event);
        if is_modify {
            detail = format!("改单：{detail}");
        }
        let (version, snapshot_value, reply) = {
            let mut state = self.state.lock().await;
            if is_priority
                && !state
                    .eligibilities
                    .iter()
                    .any(|e| e.user_id.0 == ev.user_id)
            {
                state
                    .eligibilities
                    .push(priority_eligibility(&round_id, &ev.user_id));
            }
            if is_modify {
                if let DomainEvent::ClaimCreated(created) = &event.payload {
                    let targets: Vec<_> = created.items.iter().map(|l| l.item_id.clone()).collect();
                    for item_id in targets {
                        state
                            .events
                            .push(cancel_for_modify(&round_id, &ev, seq, now, item_id));
                    }
                }
            }
            state.events.push(event);
            // U4：为分配展示名解析 CN（回退归一化昵称 → user_id）。
            let display_names: std::collections::HashMap<crate::domain::ids::UserId, String> =
                state
                    .display
                    .iter()
                    .filter_map(|(uid, raw)| {
                        crate::settings::resolve_cn(&cfg, uid, raw)
                            .map(|name| (crate::domain::ids::UserId(uid.clone()), name))
                    })
                    .collect();
            let snapshot = rebuild_allocation_snapshot(
                &items,
                &state.events,
                &state.eligibilities,
                &display_names,
            );
            let version = snapshot.version;
            let snapshot_value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            state.version = version;
            state.snapshot = Some(snapshot);
            state.messages.push(MessageRecord {
                seq,
                user_id: ev.user_id.clone(),
                display: display.clone(),
                text: ev.text.clone(),
                status: "Applied".to_string(),
                detail: detail.clone(),
            });
            (
                version,
                snapshot_value,
                format!("已记录，当前版本 #{}", version),
            )
        };

        self.persist(&ev, "Applied", &detail, "message").await;

        PipelineOutcome {
            status: "Applied".to_string(),
            detail,
            reply: Some(reply),
            version,
            snapshot: Some(snapshot_value),
        }
    }

    /// 统一接收入口（NC / 测试注入共用，R6）。
    pub async fn ingest(&self, ev: IncomingEvent) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        let events = gate_nc_multi(&cfg, ev);
        let mut last: Option<PipelineOutcome> = None;
        for e in events {
            last = Some(self.ingest_one(&cfg, e).await);
        }
        last.unwrap_or(PipelineOutcome {
            status: "Ignored".to_string(),
            detail: String::new(),
            reply: None,
            version: 0,
            snapshot: None,
        })
    }

    /// 单条入站（批次解包后逐条调用）。
    async fn ingest_one(&self, cfg: &AppConfig, mut ev: IncomingEvent) -> PipelineOutcome {
        // 非群消息（私聊/临时会话无群号）一律 Dropped，不进入处理（防御性；网关已先拦）。
        if ev.group_id.trim().is_empty() {
            self.persist(&ev, "Dropped", "非群消息", "drop:not_group_message")
                .await;
            let (version, snapshot) = self.board().await;
            return PipelineOutcome {
                status: "Dropped".to_string(),
                detail: "非群消息".to_string(),
                reply: None,
                version,
                snapshot: Some(snapshot),
            };
        }
        // 匿名代排 `X(代)`：席位填 `发送者(代)`，权限按发送者（`proxy_base`）。
        if crate::parser::coalesce::is_anonymous_proxy(&ev.text)
            && crate::settings::proxy_base(&ev.user_id).is_none()
        {
            let base = crate::settings::resolve_cn(cfg, &ev.user_id, &ev.nickname)
                .unwrap_or_else(|| ev.user_id.clone());
            ev.sender_user_id = Some(ev.user_id.clone());
            ev.sender_nickname = Some(ev.nickname.clone());
            ev.user_id = format!("{base}(代)");
            ev.nickname = String::new();
        }
        if !(cfg.merge.enabled && cfg.merge.same_sender) {
            return self.process(ev).await;
        }
        self.ingest_merged(cfg.clone(), ev).await
    }

    /// 合并摄取（急切 + 更新，R5）：以日志为准 `recompute`，保证 测试==实际、追加==新消息。
    async fn ingest_merged(&self, cfg: AppConfig, ev: IncomingEvent) -> PipelineOutcome {
        let _serial = self.ingest_lock.lock().await; // 串行化（C8）
        // 锁定：仍按旧路径拒绝，不触发 recompute。
        if self.state.lock().await.locked {
            return self.process(ev).await;
        }
        let items = cfg.round.to_items();
        let kind = {
            let cand = crate::parser::coalesce::Candidate {
                user_id: &ev.user_id,
                text: &ev.text,
                is_admin: ev.is_admin,
                force_boundary: false,
            };
            crate::parser::coalesce::classify(&cand, &items)
        };

        let Classification::Join(intent) = kind else {
            // 边界 / 闲聊：断开当前组，走旧路径。
            *self.merge_open.lock().await = None;
            return self.process(ev).await;
        };

        // 追加或新建当前组；代表 = 组内 `timestamp` 最大者（并列取较晚到达）。
        let (joined, rep_message_id, rep_ts, rep_nick) = {
            let mut open = self.merge_open.lock().await;
            let stale = open
                .as_ref()
                .map(|g| {
                    g.round_id != cfg.round.round_id
                        || g.revision != cfg.revision
                        || g.user_id != ev.user_id
                        || g.intent != intent
                })
                .unwrap_or(true);
            if stale {
                *open = Some(OpenGroup {
                    round_id: cfg.round.round_id.clone(),
                    revision: cfg.revision,
                    user_id: ev.user_id.clone(),
                    intent,
                    events: Vec::new(),
                });
            }
            let g = open.as_mut().unwrap();
            g.events.push(ev.clone());
            let rep_idx = g
                .events
                .iter()
                .enumerate()
                .max_by_key(|(i, e)| (e.timestamp_ms, *i))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let rep = g.events[rep_idx].clone();
            let cands: Vec<crate::parser::coalesce::Candidate> = g
                .events
                .iter()
                .map(|e| crate::parser::coalesce::Candidate {
                    user_id: &e.user_id,
                    text: &e.text,
                    is_admin: e.is_admin,
                    force_boundary: false,
                })
                .collect();
            let all: Vec<usize> = (0..cands.len()).collect();
            let joined = crate::parser::coalesce::joined_text(&cands, &all);
            (joined, rep.message_id.clone(), rep.timestamp_ms, rep.nickname.clone())
        };

        // C-3：保留原始事件。
        if let Some(raw) = &ev.raw {
            if let Err(e) = self.messages.append_raw_event(&cfg.round.round_id, raw).await {
                warn!("原始事件写入失败: {e}");
            }
        }
        // 落原始记录（状态占位，recompute 回写）。
        self.persist(&ev, "Pending", "pending", "message").await;

        // LLM 解析合并文本 → ParseOverride（键=代表 message_id），保证重放一致。
        if cfg.llm.enabled {
            let rule = RuleParser::parse(&joined, &items, ev.is_admin);
            if looks_like_order(&joined, &rule) {
                let mut synthetic = ev.clone();
                synthetic.text = joined.clone();
                if let Ok(parsed) = self.llm_parse(&cfg, &synthetic).await {
                    let mut rep_ev = ev.clone();
                    rep_ev.message_id = rep_message_id.clone();
                    rep_ev.nickname = rep_nick.clone();
                    rep_ev.timestamp_ms = rep_ts;
                    self.emit_parse_override(
                        &cfg,
                        &rep_ev,
                        crate::messages::next_seq(),
                        Utc::now(),
                        &parsed,
                        "合并代表消息 LLM 澄清",
                    )
                    .await;
                }
            }
        }

        // 权威重算（追加更新 == 新消息更新）。
        let _ = self.recompute().await;

        let state = self.state.lock().await;
        let last = state.messages.last();
        let (status, detail) = last
            .map(|m| (m.status.clone(), m.detail.clone()))
            .unwrap_or_else(|| ("Ignored".to_string(), String::new()));
        PipelineOutcome {
            status,
            detail,
            reply: None,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }

    /// 以消息日志为准的权威重算（实时/重放同源）；返回重放结果。
    pub async fn recompute(&self) -> anyhow::Result<ReplayResult> {
        let cfg = self.cfg.get().await;
        let result =
            crate::replay::session::replay(&self.messages, &cfg, ReplayOverrides::default()).await?;
        let mut records = self.messages.read_all(&cfg.round.round_id).await?;
        let outcomes: std::collections::HashMap<&str, (&str, &str)> = result
            .outcomes
            .iter()
            .map(|o| (o.record_id.as_str(), (o.status.as_str(), o.detail.as_str())))
            .collect();
        for record in &mut records {
            if let Some((status, detail)) = outcomes.get(record.record_id.as_str()) {
                record.status = (*status).to_string();
                record.detail = (*detail).to_string();
            }
        }
        self.messages.replace_all(&cfg.round.round_id, &records).await?;
        self.apply_replay(&result, &records).await;
        Ok(result)
    }

    /// 持久化一条入站消息日志（`routed=message`，`status` 记处理结果）。
    async fn persist(&self, ev: &IncomingEvent, status: &str, detail: &str, routed: &str) {
        let round_id = self.cfg.get().await.round.round_id.clone();
        let rec = LogMessageRecord {
            record_id: crate::messages::new_record_id(),
            record_revision: 1,
            seq: crate::messages::next_seq(),
            group_id: ev.group_id.clone(),
            user_id: ev.user_id.clone(),
            nickname: ev.nickname.clone(),
            message_id: ev.message_id.clone(),
            text: ev.text.clone(),
            timestamp_ms: ev.timestamp_ms,
            is_admin: ev.is_admin,
            routed: routed.to_string(),
            status: status.to_string(),
            detail: detail.to_string(),
            sender_user_id: ev.sender_user_id.clone(),
            sender_nickname: ev.sender_nickname.clone(),
        };
        if let Err(e) = self.messages.append(&round_id, &rec).await {
            warn!("消息日志写入失败: {e}");
        }
    }

    async fn finish(
        &self,
        ev: &IncomingEvent,
        display: &str,
        seq: i64,
        status: &str,
        detail: &str,
        reply: Option<String>,
    ) -> PipelineOutcome {
        self.persist(ev, status, detail, "message").await;
        let mut state = self.state.lock().await;
        state.messages.push(MessageRecord {
            seq,
            user_id: ev.user_id.clone(),
            display: display.to_string(),
            text: ev.text.clone(),
            status: status.to_string(),
            detail: detail.to_string(),
        });
        PipelineOutcome {
            status: status.to_string(),
            detail: detail.to_string(),
            reply,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }
}

#[async_trait]
impl EventSink for Pipeline {
    async fn handle(&self, ev: IncomingEvent) {
        self.ingest(ev).await;
    }

    fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
    }
}
