use std::sync::Arc;

use async_trait::async_trait;
use chrono::TimeZone;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::bus::{EventSink, IncomingEvent, PipelineOutcome};
use crate::domain::ids::{RoundId, UserId};
use crate::engine::replay::rebuild_allocation_snapshot;
use crate::messages::{MessageLog, MessageRecord as LogMessageRecord};
use crate::parser::coalesce::Classification;
use crate::parser::parsed_event::ParsedIntent;
use crate::replay::session::{ReplayOverrides, ReplayResult};
use crate::settings::{AppConfig, ConfigStore};

use super::client::{LlmClient, OpenAiClient};

mod admin;
mod export;
mod llm_parse;
mod replay_view;
mod state;
mod understanding;
mod view;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod automatic_tests;
#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod suggest_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transaction_tests;

use admin::priority_eligibility;
use state::{MessageRecord, State};

const RULE_CONFIDENCE: f32 = 0.9;

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
        let outer_ts = ev.timestamp_ms;
        // 解包后**按时间戳升序**处理（同刻按给定顺序稳定排序）；索引 `#i` 随排序结果重排。
        let mut entries: Vec<(i64, usize, ForgedEnvelope)> = list
            .into_iter()
            .enumerate()
            .map(|(i, e)| (e.timestamp_ms.unwrap_or(outer_ts), i, e))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        return entries
            .into_iter()
            .enumerate()
            .map(|(i, (_, _, e))| IncomingEvent {
                group_id: ev.group_id.clone(),
                user_id: e.id.clone(),
                nickname: e.nickname.unwrap_or_else(|| e.id.clone()),
                message_id: format!("{}#{}", ev.message_id, i),
                text: e.text,
                timestamp_ms: e.timestamp_ms.unwrap_or(outer_ts),
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
    view_clock: Mutex<view::ViewClock>,
    parked: Mutex<std::collections::HashMap<String, State>>,
}

impl Pipeline {
    pub(crate) async fn complete_action_draft(
        &self,
        cfg: &AppConfig,
        system: &str,
        user: &str,
    ) -> anyhow::Result<String> {
        self.llm.complete(&cfg.llm, system, user).await
    }
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
            view_clock: Mutex::new(view::ViewClock::new()),
            parked: Mutex::new(std::collections::HashMap::new()),
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
            let outcome = Box::pin(self.process_inner(seg)).await;
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
        let _serial = self.message_write_guard().await;
        self.process_inner(ev).await
    }

    async fn process_inner(&self, ev: IncomingEvent) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        if let Some(raw) = &ev.raw {
            if let Err(error) = self
                .messages
                .append_raw_event(&cfg.round.round_id, raw)
                .await
            {
                warn!("原始事件写入失败: {error}");
            }
        }
        if let Some(segments) = proxy_segments(&ev.text) {
            return self.process_proxy_segments(&ev, segments).await;
        }
        let (identity, _) = crate::parser::normalize::clean_nickname(&ev.nickname);
        let display = crate::settings::resolve_cn(&cfg, &ev.user_id, &ev.nickname)
            .unwrap_or_else(|| ev.user_id.clone());
        let mut context = self.processing_context(&cfg, &ev).await;
        let seq = context.record.seq;
        {
            let mut state = self.state.lock().await;
            state.seq = seq;
            if !state
                .seen
                .insert(format!("{}::{}", ev.group_id, ev.message_id))
            {
                drop(state);
                context.understanding = crate::messages::processing::Understanding::terminal(
                    crate::messages::processing::UnderstandingSource::Duplicate,
                    "Duplicate",
                    "重复 message_id，已按幂等忽略",
                    None,
                );
                return self
                    .finish(
                        &cfg.round.round_id,
                        &context,
                        &ev,
                        &display,
                        context.understanding.validation.clone(),
                    )
                    .await;
            }
            state.display.insert(ev.user_id.clone(), display.clone());
            state.identity.insert(ev.user_id.clone(), identity);
        }
        let text = ev.text.trim();
        if text.is_empty() || text.starts_with('/') {
            if text.starts_with('/') && ev.is_admin && cfg.gateway.admin_commands_enabled {
                return self
                    .run_admin_command(
                        &cfg.round.round_id,
                        &ev,
                        &display,
                        seq,
                        text.trim_start_matches('/').trim(),
                    )
                    .await;
            }
            let (status, detail) = if text.is_empty() {
                ("Ignored", "空消息")
            } else if !ev.is_admin {
                ("Rejected", "非管理员斜杠命令")
            } else {
                ("Applied", "管理员命令已记录（执行开关关闭）")
            };
            context.understanding = crate::messages::processing::Understanding::terminal(
                crate::messages::processing::UnderstandingSource::Control,
                status,
                detail,
                text.starts_with('/').then(|| detail.to_string()),
            );
            return self
                .finish(
                    &cfg.round.round_id,
                    &context,
                    &ev,
                    &display,
                    context.understanding.validation.clone(),
                )
                .await;
        }
        context.understanding = self.understand(&cfg, &ev, &context, false).await;
        let evaluated = crate::replay::session::evaluate_understanding(
            &cfg,
            &context.record,
            &context.understanding,
            context.manual_phase,
        );
        if evaluated.events.is_empty() {
            return self
                .finish(
                    &cfg.round.round_id,
                    &context,
                    &ev,
                    &display,
                    evaluated.outcome,
                )
                .await;
        }
        let (events, eligibilities, snapshot) = {
            let state = self.state.lock().await;
            let mut events = state.events.clone();
            events.extend(evaluated.events);
            let mut eligibilities = state.eligibilities.clone();
            if evaluated.priority_claim && !eligibilities.iter().any(|e| e.user_id.0 == ev.user_id)
            {
                eligibilities.push(priority_eligibility(
                    &RoundId(cfg.round.round_id.clone()),
                    &ev.user_id,
                ));
            }
            let names = state
                .display
                .iter()
                .filter_map(|(uid, raw)| {
                    crate::settings::resolve_cn(&cfg, uid, raw)
                        .map(|name| (UserId(uid.clone()), name))
                })
                .collect();
            let snapshot =
                rebuild_allocation_snapshot(&cfg.round.to_items(), &events, &eligibilities, &names);
            (events, eligibilities, snapshot)
        };
        let version = snapshot.version;
        let mut outcome = evaluated.outcome;
        outcome.reply = Some(format!("已记录，当前版本 #{version}"));
        let record = match self
            .persist_understanding(&cfg.round.round_id, &context, outcome.clone())
            .await
        {
            Ok(record) => record,
            Err(error) => return self.persistence_failure(&ev, false, error).await,
        };
        let value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
        let mut state = self.state.lock().await;
        state.events = events;
        state.eligibilities = eligibilities;
        state.version = version;
        state.snapshot = Some(snapshot);
        state.messages.push(MessageRecord::from_log(
            &record,
            &cfg.round.round_id,
            &display,
        ));
        PipelineOutcome {
            status: outcome.status,
            detail: outcome.detail,
            reply: outcome.reply,
            version,
            snapshot: Some(value),
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
            if let Err(error) = self
                .persist(
                    &cfg.round.round_id,
                    &ev,
                    crate::messages::next_seq(),
                    "Dropped",
                    "非群消息",
                    "drop:not_group_message",
                )
                .await
            {
                return self.persistence_failure(&ev, true, error).await;
            }
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
    async fn ingest_merged(&self, _cfg: AppConfig, ev: IncomingEvent) -> PipelineOutcome {
        let _serial = self.message_write_guard().await;
        let cfg = self.cfg.get().await;
        let already_seen = self
            .state
            .lock()
            .await
            .seen
            .contains(&format!("{}::{}", ev.group_id, ev.message_id));
        if already_seen || self.is_locked().await || proxy_segments(&ev.text).is_some() {
            return self.process_inner(ev).await;
        }
        let candidate = crate::parser::coalesce::Candidate {
            user_id: &ev.user_id,
            text: &ev.text,
            is_admin: ev.is_admin,
            force_boundary: false,
        };
        if !matches!(
            crate::parser::coalesce::classify(&candidate, &cfg.round.to_items()),
            Classification::Join(_)
        ) {
            return self.process_inner(ev).await;
        }
        if let Some(raw) = &ev.raw {
            if let Err(error) = self
                .messages
                .append_raw_event(&cfg.round.round_id, raw)
                .await
            {
                return self.persistence_failure(&ev, false, error).await;
            }
        }
        let mut context = self.processing_context(&cfg, &ev).await;
        let new_record_id = context.record.record_id.clone();
        if let Err(error) = self
            .messages
            .append(&cfg.round.round_id, &context.record)
            .await
        {
            return self.persistence_failure(&ev, false, error).await;
        }
        let result = async {
            let records = self.messages.read_all(&cfg.round.round_id).await?;
            let mut ordered: Vec<_> = records.iter().collect();
            ordered.sort_by_key(|record| (record.timestamp_ms, record.seq));
            let overrides =
                crate::replay::session::load_parse_overrides(&self.messages, &cfg.round.round_id)
                    .await;
            let groups = crate::replay::session::prospective_groups(&cfg, &ordered, &overrides);
            let group = groups
                .iter()
                .find(|group| {
                    group
                        .iter()
                        .any(|&index| ordered[index].record_id == new_record_id)
                })
                .ok_or_else(|| anyhow::anyhow!("新消息缺少理解单元"))?;
            let members: Vec<_> = group.iter().map(|&index| ordered[index]).collect();
            let representative = members
                .iter()
                .max_by_key(|record| (record.timestamp_ms, record.seq))
                .ok_or_else(|| anyhow::anyhow!("空理解单元"))?;
            context.record = (*representative).clone();
            context.members = crate::messages::processing::processing_members(&members);
            let mut synthetic = ev.clone();
            synthetic.message_id = representative.message_id.clone();
            synthetic.nickname = representative.nickname.clone();
            synthetic.timestamp_ms = representative.timestamp_ms;
            let candidates: Vec<_> = members
                .iter()
                .map(|record| crate::parser::coalesce::Candidate {
                    user_id: &record.user_id,
                    text: &record.text,
                    is_admin: record.is_admin,
                    force_boundary: false,
                })
                .collect();
            synthetic.text = crate::parser::coalesce::joined_text(
                &candidates,
                &(0..members.len()).collect::<Vec<_>>(),
            );
            context.understanding = self.understand(&cfg, &synthetic, &context, true).await;
            let evaluated = crate::replay::session::evaluate_understanding(
                &cfg,
                &context.record,
                &context.understanding,
                context.manual_phase,
            );
            let completed = context.completed_record(evaluated.outcome);
            self.messages
                .update(&cfg.round.round_id, &completed.record_id, |record| {
                    record.processing = completed.processing.clone();
                    record.status = completed.status.clone();
                    record.detail = completed.detail.clone();
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("理解代表消息已不存在"))?;
            if completed.record_id != new_record_id {
                let mut member_result = completed
                    .processing
                    .as_ref()
                    .and_then(|history| history.results.last())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("新消息缺少原自动结果"))?;
                member_result.original_outcome =
                    crate::messages::processing::ProcessingOutcome::new(
                        "Merged",
                        format!("并入 #{}", completed.seq),
                        None,
                    );
                self.messages
                    .update(&cfg.round.round_id, &new_record_id, |record| {
                        record.status = member_result.original_outcome.status.clone();
                        record.detail = member_result.original_outcome.detail.clone();
                        record
                            .processing
                            .get_or_insert_with(Default::default)
                            .results
                            .push(member_result.clone());
                    })
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("新消息已不存在"))?;
            }
            self.recompute_locked(&cfg).await?;
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = result {
            return self.persistence_failure(&ev, false, error).await;
        }
        let state = self.state.lock().await;
        let message = state
            .messages
            .iter()
            .find(|message| message.record_id == new_record_id);
        PipelineOutcome {
            status: message
                .map(|message| message.status.clone())
                .unwrap_or_else(|| "Error".to_string()),
            detail: message
                .map(|message| message.detail.clone())
                .unwrap_or_else(|| "重算后消息缺失".to_string()),
            reply: None,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|snapshot| serde_json::to_value(snapshot).ok()),
        }
    }

    /// 以消息日志为准的权威重算（实时/重放同源）；返回重放结果。
    pub async fn recompute(&self) -> anyhow::Result<ReplayResult> {
        let _serial = self.message_write_guard().await;
        let cfg = self.cfg.get().await;
        self.messages
            .recover_replacement(&cfg.round.round_id)
            .await?;
        self.recompute_locked(&cfg).await
    }

    pub(crate) async fn message_write_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.messages.write_guard().await
    }

    /// Caller holds message_write_guard; manual writers also hold the config read guard.
    pub(crate) async fn recompute_locked(&self, cfg: &AppConfig) -> anyhow::Result<ReplayResult> {
        let result =
            crate::replay::session::replay(&self.messages, cfg, ReplayOverrides::default()).await?;
        // Inputs and original automatic outcomes are immutable. Current revisions
        // and status belong to the rebuildable projection, not a second file commit.
        let records = self.messages.read_only(&cfg.round.round_id).await?;
        self.apply_replay(&result, &records).await;
        Ok(result)
    }

    /// 持久化一条入站消息日志（`routed=message`，`status` 记处理结果）。
    async fn persist(
        &self,
        round_id: &str,
        ev: &IncomingEvent,
        seq: i64,
        status: &str,
        detail: &str,
        routed: &str,
    ) -> anyhow::Result<LogMessageRecord> {
        let rec = LogMessageRecord {
            record_id: crate::messages::new_record_id(),
            record_revision: 1,
            processing: None,
            seq,
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
        self.messages.append(round_id, &rec).await?;
        Ok(rec)
    }

    async fn persistence_failure(
        &self,
        ev: &IncomingEvent,
        duplicate: bool,
        error: anyhow::Error,
    ) -> PipelineOutcome {
        warn!("消息日志写入失败: {error}");
        let mut state = self.state.lock().await;
        if !duplicate {
            state
                .seen
                .remove(&format!("{}::{}", ev.group_id, ev.message_id));
        }
        PipelineOutcome {
            status: "Error".to_string(),
            detail: "消息日志保存失败，本次未作为成功消息发布；请检查本地存储".to_string(),
            reply: None,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }

    async fn finish(
        &self,
        round_id: &str,
        context: &understanding::ProcessingContext,
        ev: &IncomingEvent,
        display: &str,
        outcome: crate::messages::processing::ProcessingOutcome,
    ) -> PipelineOutcome {
        let record = match self
            .persist_understanding(round_id, context, outcome.clone())
            .await
        {
            Ok(record) => record,
            Err(error) => {
                return self
                    .persistence_failure(ev, outcome.status == "Duplicate", error)
                    .await
            }
        };
        let mut state = self.state.lock().await;
        state
            .messages
            .push(MessageRecord::from_log(&record, round_id, display));
        PipelineOutcome {
            status: outcome.status,
            detail: outcome.detail,
            reply: outcome.reply,
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
