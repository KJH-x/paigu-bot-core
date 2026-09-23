use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::warn;

use crate::bus::{EventSink, IncomingEvent, PipelineOutcome};
use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{EligibilityId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::replay::{describe_event, rebuild_allocation_snapshot};
use crate::messages::{MessageLog, MessageRecord as LogMessageRecord};
use crate::parser::parsed_event::{ParsedClaimItem, ParsedIntent, ParsedMessage};
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::round::{can_cancel, can_claim, phase_at, RoundPhase};
use crate::settings::{AppConfig, ConfigStore};

use super::client::{LlmClient, OpenAiClient};
use super::prompt;

const RULE_CONFIDENCE: f32 = 0.9;
const CONFIDENCE_THRESHOLD: f32 = 0.65;

pub struct Pipeline {
    cfg: Arc<ConfigStore>,
    llm: Arc<dyn LlmClient>,
    state: Mutex<State>,
    messages: Arc<MessageLog>,
}

#[derive(Default)]
struct State {
    events: Vec<EventEnvelope>,
    messages: Vec<MessageRecord>,
    seen: HashSet<String>,
    eligibilities: Vec<Eligibility>,
    display: std::collections::HashMap<String, String>,
    identity: std::collections::HashMap<String, String>,
    seq: i64,
    version: i64,
    snapshot: Option<AllocationSnapshot>,
    /// 管理员 `/锁位` `/结团` 后为 true，不再接受排/撤/改（`/开团` 解锁）。
    locked: bool,
}

struct MessageRecord {
    seq: i64,
    display: String,
    text: String,
    status: String,
    detail: String,
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
        })
    }

    /// 共享消息日志句柄（Gateway / API 复用同一实例）。
    #[allow(dead_code)]
    pub fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
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
        let (identity, display_raw) = crate::gateway::onebot::clean_nickname(&ev.nickname);
        let display = if display_raw.trim().is_empty() {
            ev.user_id.clone()
        } else {
            display_raw
        };

        let seq = {
            let mut state = self.state.lock().await;
            let key = format!("{}::{}", ev.group_id, ev.message_id);
            if !state.seen.insert(key) {
                let detail = "重复 message_id，已按幂等忽略";
                let seq = state.seq;
                state.messages.push(MessageRecord {
                    seq,
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

        let parsed = if rule.confidence >= RULE_CONFIDENCE && rule.intent != ParsedIntent::Unknown {
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
        let user_id = UserId(ev.user_id.clone());
        let validator = EventValidator::new(CONFIDENCE_THRESHOLD);

        let event = match validator
            .validate(
                parsed,
                &user_id,
                &ev.group_id,
                Some(ev.message_id.clone()),
                &round_contexts,
                now,
                seq,
            )
            .await
        {
            Ok(ValidationOutcome::Ok(event)) => event,
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

        let is_priority = crate::settings::is_priority_user(
            &cfg.round.priority_users,
            &[
                ev.user_id.as_str(),
                ev.nickname.as_str(),
                identity.as_str(),
                display.as_str(),
            ],
        );
        let window = cfg
            .round
            .priority_window
            .as_ref()
            .map(|w| (w.start_ms, w.end_ms));
        if crate::settings::in_priority_window(window, ev.timestamp_ms) && !is_priority {
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
            if let Some(phase) = phase_at(&cfg.round.phases, ev.timestamp_ms) {
                if let Some(detail) = phase_rejection(&cfg, &event, phase, is_priority) {
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
            if is_priority && !state.eligibilities.iter().any(|e| e.user_id.0 == ev.user_id) {
                state
                    .eligibilities
                    .push(priority_eligibility(&round_id, &ev.user_id));
            }
            if is_modify {
                if let DomainEvent::ClaimCreated(created) = &event.payload {
                    let targets: Vec<_> = created.items.iter().map(|l| l.item_id.clone()).collect();
                    for item_id in targets {
                        state.events.push(cancel_for_modify(
                            &round_id,
                            &ev,
                            seq,
                            now,
                            item_id,
                        ));
                    }
                }
            }
            state.events.push(event);
            let snapshot =
                rebuild_allocation_snapshot(&items, &state.events, &state.eligibilities);
            let version = snapshot.version;
            let snapshot_value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            state.version = version;
            state.snapshot = Some(snapshot);
            state.messages.push(MessageRecord {
                seq,
                display: display.clone(),
                text: ev.text.clone(),
                status: "Applied".to_string(),
                detail: detail.clone(),
            });
            (version, snapshot_value, format!("已记录，当前版本 #{}", version))
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

    /// 管理员斜杠命令**落地执行**（D-1，仅在 `gateway.admin_commands_enabled` 开启时调用）。
    async fn run_admin_command(
        &self,
        ev: &IncomingEvent,
        display: &str,
        seq: i64,
        cmd: &str,
    ) -> PipelineOutcome {
        let head = cmd.split_whitespace().next().unwrap_or("").to_string();
        let (status, detail, reply): (&str, String, String) = match head.as_str() {
            "开团" | "解锁" | "open" | "unlock" => {
                let mut st = self.state.lock().await;
                st.locked = false;
                let v = st.version;
                (
                    "Applied",
                    "已开团（解锁）".to_string(),
                    format!("已开团（解锁），当前版本 #{v}"),
                )
            }
            "锁位" | "锁定" | "结团" | "结束" | "lock" | "close" => {
                let mut st = self.state.lock().await;
                st.locked = true;
                let v = st.version;
                (
                    "Applied",
                    "已锁定".to_string(),
                    format!("已锁定，当前版本 #{v}"),
                )
            }
            "状态" | "status" => {
                let st = self.state.lock().await;
                (
                    "Applied",
                    "状态".to_string(),
                    format!("版本 #{}，锁定={}", st.version, st.locked),
                )
            }
            "导出" | "export" => self.export_snapshot().await,
            other => (
                "Rejected",
                format!("未支持的管理员命令：{other}"),
                format!("未支持的管理员命令：{other}"),
            ),
        };

        self.persist(ev, status, &detail, "admin").await;
        let (version, snapshot_value) = {
            let mut st = self.state.lock().await;
            st.messages.push(MessageRecord {
                seq,
                display: display.to_string(),
                text: ev.text.clone(),
                status: status.to_string(),
                detail: detail.clone(),
            });
            (
                st.version,
                st.snapshot
                    .as_ref()
                    .and_then(|s| serde_json::to_value(s).ok())
                    .unwrap_or(Value::Null),
            )
        };

        PipelineOutcome {
            status: status.to_string(),
            detail,
            reply: Some(reply),
            version,
            snapshot: Some(snapshot_value),
        }
    }

    /// `/导出`：导出**单一 JSON 快照**（C-2）到 `data/snapshots/<round>.snapshot.json`。
    async fn export_snapshot(&self) -> (&'static str, String, String) {
        let cfg = self.cfg.get().await;
        let round_id = cfg.round.round_id.clone();
        let records = self.messages.read_all(&round_id).await.unwrap_or_default();
        let events = self
            .messages
            .read_raw_events(&round_id)
            .await
            .unwrap_or_default();
        let board = {
            let st = self.state.lock().await;
            st.snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok())
                .unwrap_or(Value::Null)
        };

        let bundle = match crate::snapshot_bundle::SnapshotBundle::seal(
            round_id.clone(),
            cfg.revision,
            chrono::Utc::now().to_rfc3339(),
            serde_json::to_value(&cfg).unwrap_or(Value::Null),
            serde_json::to_value(&records).unwrap_or(Value::Null),
            serde_json::to_value(&events).unwrap_or(Value::Null),
            board,
            Value::Null,
        ) {
            Ok(bundle) => bundle,
            Err(e) => {
                let msg = format!("导出失败: {e}");
                return ("Error", msg.clone(), msg);
            }
        };

        let path = std::path::PathBuf::from("data/snapshots")
            .join(format!("{round_id}.snapshot.json"));
        match bundle.export_file(&path) {
            Ok(path) => (
                "Applied",
                "已导出快照".to_string(),
                format!("已导出快照：{}", path.display()),
            ),
            Err(e) => {
                let msg = format!("导出失败: {e}");
                ("Error", msg.clone(), msg)
            }
        }
    }

    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = State::default();
    }

    pub async fn board(&self) -> (i64, Value) {
        let state = self.state.lock().await;
        let snapshot = state
            .snapshot
            .as_ref()
            .and_then(|s| serde_json::to_value(s).ok())
            .unwrap_or(Value::Null);
        (state.version, snapshot)
    }

    pub async fn messages_since(&self, since: i64) -> Vec<Value> {
        let state = self.state.lock().await;
        state
            .messages
            .iter()
            .filter(|m| m.seq > since)
            .map(|m| {
                json!({
                    "seq": m.seq,
                    "display": m.display,
                    "text": m.text,
                    "status": m.status,
                    "detail": m.detail,
                })
            })
            .collect()
    }

    pub async fn who_whats(&self) -> Vec<Value> {
        let state = self.state.lock().await;
        let Some(snapshot) = state.snapshot.as_ref() else {
            return Vec::new();
        };

        let mut grouped: BTreeMap<String, (String, BTreeMap<String, u32>)> = BTreeMap::new();
        for summary in &snapshot.user_summaries {
            let uid = summary.user_id.0.clone();
            let display = state.display.get(&uid).cloned().unwrap_or_else(|| uid.clone());
            let identity = state.identity.get(&uid).cloned().unwrap_or_else(|| display.clone());
            let entry = grouped
                .entry(display)
                .or_insert_with(|| (identity, BTreeMap::new()));
            for item in &summary.items {
                *entry.1.entry(item.item_name.clone()).or_insert(0) += item.quantity;
            }
        }

        grouped
            .into_iter()
            .map(|(display, (identity, items))| {
                let items: Vec<Value> = items
                    .into_iter()
                    .map(|(name, qty)| json!({ "name": name, "qty": qty }))
                    .collect();
                json!({ "display": display, "identity": identity, "items": items })
            })
            .collect()
    }

    /// 持久化一条入站消息日志（`routed=message`，`status` 记处理结果）。
    async fn persist(&self, ev: &IncomingEvent, status: &str, detail: &str, routed: &str) {
        let round_id = self.cfg.get().await.round.round_id.clone();
        let rec = LogMessageRecord {
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

    async fn llm_parse(&self, cfg: &AppConfig, ev: &IncomingEvent) -> anyhow::Result<ParsedMessage> {
        let system = prompt::build_system_prompt(cfg);
        let user = prompt::build_user_prompt(ev);
        let raw = self.llm.complete(&cfg.llm, &system, &user).await?;
        let items = cfg.round.to_items();
        parse_llm_json(&raw, &items, &cfg.round.round_id)
    }
}

#[async_trait]
impl EventSink for Pipeline {
    async fn handle(&self, ev: IncomingEvent) {
        self.process(ev).await;
    }

    fn messages(&self) -> Arc<MessageLog> {
        self.messages.clone()
    }
}

fn cancel_for_modify(
    round_id: &RoundId,
    ev: &IncomingEvent,
    seq: i64,
    now: DateTime<Utc>,
    item_id: crate::domain::ids::ItemId,
) -> EventEnvelope {
    EventEnvelope {
        event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        group_id: ev.group_id.clone(),
        user_id: UserId(ev.user_id.clone()),
        raw_message_id: Some(ev.message_id.clone()),
        event_type: "claim_cancelled".to_string(),
        effective_at: now,
        sequence: seq,
        payload: DomainEvent::ClaimCancelled(crate::domain::event::ClaimCancelled {
            target_claim_id: None,
            target_item_id: Some(item_id),
            quantity: None,
            reason: Some("改单".to_string()),
            parse_trace: None,
            validation_trace: vec![],
        }),
        status: crate::domain::event::EventStatus::Active,
    }
}

fn priority_eligibility(round_id: &RoundId, user_id: &str) -> Eligibility {
    Eligibility {
        eligibility_id: EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        user_id: UserId(user_id.to_string()),
        priority_type: "shopping_fund".to_string(),
        priority_level: 10,
        scope: EligibilityScope {
            item_ids: None,
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: Some(DateTime::<Utc>::from_timestamp_millis(0).unwrap_or_else(Utc::now)),
        valid_until: None,
        note: Some("预存(购物金)用户".to_string()),
    }
}

fn phase_label(phase: RoundPhase) -> &'static str {
    match phase {
        RoundPhase::Phase0 => "Phase 0",
        RoundPhase::PhaseI => "Phase I",
        RoundPhase::PhaseII => "Phase II",
        RoundPhase::PhaseIII => "Phase III",
        RoundPhase::Settling => "结算",
        RoundPhase::Locked => "锁定",
    }
}

/// 阶段越权判定；返回 `Some(detail)` 表示应拒绝（detail 含「阶段」）。
fn phase_rejection(
    cfg: &AppConfig,
    event: &EventEnvelope,
    phase: RoundPhase,
    is_priority: bool,
) -> Option<String> {
    let label = phase_label(phase);
    if phase == RoundPhase::Locked {
        return Some(format!("阶段越权：{label}阶段已锁定，禁止操作"));
    }
    match &event.payload {
        DomainEvent::ClaimCreated(c) => c.items.iter().find_map(|line| {
            let class = cfg.round.item_class(&line.item_id.0);
            (!can_claim(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许排该商品"))
        }),
        DomainEvent::ClaimCancelled(c) => c.target_item_id.as_ref().and_then(|item_id| {
            let class = cfg.round.item_class(&item_id.0);
            (!can_cancel(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许撤销该商品"))
        }),
        _ => None,
    }
}

#[derive(Deserialize)]
struct LlmOut {
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    items: Vec<LlmItem>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    ambiguous_parts: Vec<String>,
    #[serde(default)]
    cancel_target: Option<String>,
}

#[derive(Deserialize)]
struct LlmItem {
    #[serde(default)]
    item: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    quantity: Option<u32>,
    #[serde(default)]
    claim_type: Option<String>,
    #[serde(default)]
    slot_policy: Option<String>,
    #[serde(default)]
    is_proxy_card: Option<bool>,
    #[serde(default)]
    notes: Option<String>,
}

fn parse_llm_json(raw: &str, items: &[Item], round_id: &str) -> anyhow::Result<ParsedMessage> {
    let json_text = extract_json(raw);
    let out: LlmOut = serde_json::from_str(&json_text).map_err(|e| {
        anyhow::anyhow!(
            "LLM JSON 解析失败: {e}; raw={}",
            super::truncate(raw, 200)
        )
    })?;

    let intent = match out
        .intent
        .as_deref()
        .unwrap_or("unknown")
        .to_ascii_lowercase()
        .as_str()
    {
        "claim" => ParsedIntent::Claim,
        "cancel" => ParsedIntent::Cancel,
        "modify" => ParsedIntent::Modify,
        "admin" | "admincommand" | "admin_command" => ParsedIntent::AdminCommand,
        _ => ParsedIntent::Unknown,
    };

    let mut parsed_items = Vec::new();
    let mut ambiguous = out.ambiguous_parts;
    for item in &out.items {
        let (parsed_item, ambiguity) = resolve_llm_item(item, items, round_id);
        if let Some(message) = ambiguity {
            ambiguous.push(message);
        }
        parsed_items.push(parsed_item);
    }

    Ok(ParsedMessage {
        intent,
        round_hint: None,
        items: parsed_items,
        cancel_target_hint: out.cancel_target,
        admin_command: None,
        confidence: out.confidence.unwrap_or(0.9),
        ambiguous_parts: ambiguous,
    })
}

fn resolve_llm_item(item: &LlmItem, items: &[Item], round_id: &str) -> (ParsedClaimItem, Option<String>) {
    let item_tok = item
        .item
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| item.name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("");
    let variant_tok = item
        .variant
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut candidates: Vec<(&Item, Option<String>)> = Vec::new();
    for candidate in items {
        let item_ok = item_tok.is_empty()
            || candidate.exact_matches_name(item_tok)
            || candidate.item_id.0 == item_tok
            || candidate.matches_name_or_alias(item_tok);
        if !item_ok {
            continue;
        }
        match variant_tok {
            None => candidates.push((candidate, None)),
            Some(variant) => {
                if let Some(found) = candidate.find_variant_by_name(variant) {
                    candidates.push((candidate, Some(found.variant_id.clone())));
                }
            }
        }
    }

    if candidates.is_empty() {
        if let Some(variant) = variant_tok {
            for candidate in items {
                if let Some(found) = candidate.find_variant_by_name(variant) {
                    candidates.push((candidate, Some(found.variant_id.clone())));
                }
            }
        }
    }

    let name = variant_tok.unwrap_or(item_tok).to_string();
    let mut parsed = ParsedClaimItem {
        name,
        category_hint: if item_tok.is_empty() {
            None
        } else {
            Some(item_tok.to_string())
        },
        quantity: item.quantity.unwrap_or(1),
        claim_type: item.claim_type.clone(),
        is_proxy_card: item.is_proxy_card,
        slot_policy: item.slot_policy.clone(),
        notes: item.notes.clone(),
        resolved_item_id: None,
        resolved_variant_id: None,
        resolved_round_id: None,
    };

    if candidates.len() == 1 {
        let (candidate, variant_id) = &candidates[0];
        parsed.resolved_item_id = Some(candidate.item_id.0.clone());
        parsed.resolved_variant_id = variant_id.clone();
        parsed.resolved_round_id = Some(round_id.to_string());
        (parsed, None)
    } else if candidates.is_empty() {
        (parsed, None)
    } else {
        let names: Vec<String> = candidates.iter().map(|(c, _)| c.name.clone()).collect();
        (parsed, Some(format!("商品歧义：{}", names.join("、"))))
    }
}

fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (stripped.find('{'), stripped.rfind('}')) {
        (Some(start), Some(end)) if end > start => stripped[start..=end].to_string(),
        _ => stripped.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{JsonlMessageStore, MessageStore};
    use std::path::PathBuf;
    use crate::round::PhaseWindow;
    use crate::settings::{default_config, LlmSettings};

    struct MockClient {
        reply: std::sync::Mutex<MockReply>,
    }

    enum MockReply {
        Ok(String),
        Err(String),
    }

    impl MockClient {
        fn ok(body: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: std::sync::Mutex::new(MockReply::Ok(body.to_string())),
            })
        }

        fn err(message: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: std::sync::Mutex::new(MockReply::Err(message.to_string())),
            })
        }
    }

    #[async_trait]
    impl LlmClient for MockClient {
        async fn complete(
            &self,
            _settings: &LlmSettings,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> anyhow::Result<String> {
            match &*self.reply.lock().unwrap() {
                MockReply::Ok(body) => Ok(body.clone()),
                MockReply::Err(message) => Err(anyhow::anyhow!(message.clone())),
            }
        }
    }

    fn base_config() -> AppConfig {
        let mut cfg = default_config();
        cfg.gateway.reply_enabled = false;
        cfg.llm.enabled = true;
        cfg.llm.fallback_to_rules = true;
        cfg.llm.api_key_env = "PAIGU_TEST_KEY_UNSET".to_string();
        cfg.round.priority_users = vec!["prio_user".to_string()];
        cfg.round.priority_window = None;
        cfg
    }

    fn store(cfg: &AppConfig) -> Arc<ConfigStore> {
        let path = std::env::temp_dir().join(format!(
            "paigu-llm-test-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
        Arc::new(ConfigStore::load(path).unwrap())
    }

    fn test_pipeline_with_dir(
        cfg: &AppConfig,
        llm: Arc<dyn LlmClient>,
    ) -> (Arc<Pipeline>, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "paigu-msgs-test-{}",
            uuid::Uuid::new_v4()
        ));
        (Pipeline::new_with_client_dir(store(cfg), llm, dir.clone()), dir)
    }

    fn test_pipeline(cfg: &AppConfig, llm: Arc<dyn LlmClient>) -> Arc<Pipeline> {
        test_pipeline_with_dir(cfg, llm).0
    }

    fn set_class(cfg: &mut AppConfig, item_id: &str, class: &str) {
        for it in &mut cfg.round.items {
            if it.item_id == item_id {
                it.class = Some(class.to_string());
            }
        }
    }

    fn phase_window(phase: RoundPhase, start_ms: i64, end_ms: i64) -> PhaseWindow {
        PhaseWindow { phase, start_ms, end_ms }
    }

    fn event(user_id: &str, nickname: &str, text: &str, timestamp_ms: i64) -> IncomingEvent {
        IncomingEvent {
            group_id: "123456789".to_string(),
            user_id: user_id.to_string(),
            nickname: nickname.to_string(),
            message_id: format!("{}::{}", user_id, text),
            text: text.to_string(),
            timestamp_ms,
            is_admin: false,
            raw: None,
        }
    }

    fn admin_event(text: &str, timestamp_ms: i64) -> IncomingEvent {
        let mut ev = event("admin1", "管理员", text, timestamp_ms);
        ev.is_admin = true;
        ev
    }

    #[tokio::test]
    async fn admin_commands_recorded_only_when_disabled() {
        let mut cfg = base_config();
        cfg.llm.enabled = false;
        cfg.gateway.admin_commands_enabled = false;
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));

        let out = pipeline.process(admin_event("/锁位", 1_000)).await;
        assert_eq!(out.status, "Applied");
        assert!(out.detail.contains("开关关闭"), "{}", out.detail);

        // 未执行：仍可排
        let claim = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 2_000))
            .await;
        assert_eq!(claim.status, "Applied");
    }

    #[tokio::test]
    async fn admin_command_lock_blocks_claims_when_enabled() {
        let mut cfg = base_config();
        cfg.llm.enabled = false;
        cfg.gateway.admin_commands_enabled = true;
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));

        let lock = pipeline.process(admin_event("/锁位", 1_000)).await;
        assert_eq!(lock.status, "Applied");
        assert!(lock.detail.contains("已锁定"), "{}", lock.detail);

        let blocked = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 2_000))
            .await;
        assert_eq!(blocked.status, "Rejected");
        assert!(blocked.detail.contains("已锁定"), "{}", blocked.detail);

        let open = pipeline.process(admin_event("/开团", 3_000)).await;
        assert_eq!(open.status, "Applied");

        let ok = pipeline
            .process(event("u1", "小明", "排 通行证 岳羽由加莉 1", 4_000))
            .await;
        assert_eq!(ok.status, "Applied");
    }

    #[tokio::test]
    async fn modify_replaces_previous_claim_quantity() {
        let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));

        let first = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(first.status, "Applied");

        let modified = pipeline
            .process(event("u1", "小明", "改 通行证 结城理 2", 2_000))
            .await;
        assert_eq!(modified.status, "Applied");
        assert!(modified.detail.starts_with("改单"), "{}", modified.detail);

        let who = pipeline.who_whats().await;
        assert_eq!(who.len(), 1);
        let qty = who[0]["items"][0]["qty"].as_i64().unwrap_or(0);
        assert_eq!(qty, 2, "改单应替换为 2：{who:?}");
    }

    fn slot_user(snapshot: &Value, item_id: &str, variant_id: &str) -> Option<String> {
        let allocations = snapshot.get("item_allocations")?.as_array()?;
        for allocation in allocations {
            if allocation.get("item_id").and_then(|v| v.as_str()) == Some(item_id)
                && allocation.get("variant_id").and_then(|v| v.as_str()) == Some(variant_id)
            {
                let slots = allocation
                    .get("boxes")?
                    .as_array()?
                    .first()?
                    .get("slots")?
                    .as_array()?;
                return slots
                    .first()
                    .and_then(|s| s.get("user_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
        }
        None
    }

    #[tokio::test]
    async fn rule_claim_success() {
        let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Applied");
        assert!(outcome.reply.unwrap().contains("已记录"));
        assert_eq!(outcome.version, 1);
        assert!(!pipeline.messages_since(0).await.is_empty());
    }

    #[tokio::test]
    async fn llm_claim_success() {
        let body = r#"{"intent":"claim","items":[{"item":"通行认证SP-月行水上","variant":"岳羽由加莉","quantity":1,"claim_type":"split","slot_policy":"normal"}],"confidence":0.95,"ambiguous_parts":[]}"#;
        let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
        let outcome = pipeline
            .process(event("u1", "小明", "帮我抢个yukari", 1_000))
            .await;
        assert_eq!(outcome.status, "Applied");
        let who = pipeline.who_whats().await;
        assert_eq!(who.len(), 1);
        assert_eq!(who[0]["display"], "小明");
    }

    #[tokio::test]
    async fn non_claim_ignored() {
        let body = r#"{"intent":"unknown","items":[],"confidence":0.2,"ambiguous_parts":[]}"#;
        let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
        let outcome = pipeline
            .process(event("u1", "小明", "今天天气不错", 1_000))
            .await;
        assert_eq!(outcome.status, "Ignored");
    }

    #[tokio::test]
    async fn ambiguous_need_confirm() {
        let body = r#"{"intent":"claim","items":[{"variant":"结城理","quantity":1,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
        let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
        let outcome = pipeline
            .process(event("u1", "小明", "帮我留一份yukari", 1_000))
            .await;
        assert_eq!(outcome.status, "NeedConfirm");
    }

    #[tokio::test]
    async fn priority_window_rejected() {
        let mut cfg = base_config();
        cfg.round.priority_window = Some(crate::settings::PriorityWindow {
            start_ms: 0,
            end_ms: i64::MAX,
        });
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert!(outcome.detail.contains("预存"));
    }

    #[tokio::test]
    async fn priority_user_sorts_first() {
        let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));
        let first = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(first.status, "Applied");
        let second = pipeline
            .process(event("prio_user", "prio_user", "排 通行证 结城理 1", 2_000))
            .await;
        assert_eq!(second.status, "Applied");
        let (_, snapshot) = pipeline.board().await;
        assert_eq!(
            slot_user(&snapshot, "pass_sp", "v_jcl"),
            Some("prio_user".to_string())
        );
    }

    #[tokio::test]
    async fn llm_failure_falls_back_to_rules() {
        let pipeline = test_pipeline(&base_config(), MockClient::err("timeout"));
        let outcome = pipeline
            .process(event("u1", "小明", "嗯嗯好的", 1_000))
            .await;
        assert_eq!(outcome.status, "Ignored");
    }

    #[tokio::test]
    async fn llm_failure_without_fallback_rejected() {
        let mut cfg = base_config();
        cfg.llm.fallback_to_rules = false;
        let pipeline = test_pipeline(&cfg, MockClient::err("timeout"));
        let outcome = pipeline
            .process(event("u1", "小明", "嗯嗯好的", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert_eq!(outcome.detail, "没识别成功");
    }

    #[tokio::test]
    async fn phase_ii_non_priority_class_a_rejected() {
        let mut cfg = base_config();
        cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
        set_class(&mut cfg, "pass_sp", "A");
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert!(outcome.detail.contains("阶段"), "detail={}", outcome.detail);
    }

    #[tokio::test]
    async fn phase_ii_priority_class_a_applied() {
        let mut cfg = base_config();
        cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
        set_class(&mut cfg, "pass_sp", "A");
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("prio_user", "成员09", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Applied");
    }

    #[tokio::test]
    async fn phase_ii_class_b_allowed_for_non_priority() {
        let mut cfg = base_config();
        cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
        set_class(&mut cfg, "pass_sp", "B");
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Applied");
    }

    #[tokio::test]
    async fn phase_i_class_a_rejected() {
        let mut cfg = base_config();
        cfg.round.phases = vec![phase_window(RoundPhase::PhaseI, 0, i64::MAX)];
        set_class(&mut cfg, "pass_sp", "A");
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert!(outcome.detail.contains("阶段"));
    }

    #[tokio::test]
    async fn locked_phase_rejects_claim() {
        let mut cfg = base_config();
        cfg.round.phases = vec![phase_window(RoundPhase::Locked, 0, i64::MAX)];
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("prio_user", "成员09", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert!(outcome.detail.contains("阶段"));
    }

    #[tokio::test]
    async fn empty_phases_are_unrestricted() {
        let mut cfg = base_config();
        cfg.round.phases = Vec::new();
        set_class(&mut cfg, "pass_sp", "A");
        let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Applied");
    }

    #[tokio::test]
    async fn message_log_records_every_inbound() {
        let cfg = base_config();
        let (pipeline, dir) = test_pipeline_with_dir(&cfg, MockClient::ok("{}"));
        pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await;
        pipeline
            .process(event("u2", "成员02", "今天天气不错", 2_000))
            .await;

        let store = JsonlMessageStore::new(&dir, &cfg.round.round_id);
        let recs = store.read_all().await.unwrap();
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].status, "Applied");
        assert_eq!(recs[1].status, "Duplicate");
        assert_eq!(recs[2].status, "Ignored");
        assert!(recs.iter().all(|r| r.routed == "message"));
        assert_eq!(recs[0].nickname, "成员01");
        assert_eq!(recs[0].timestamp_ms, 1_000);
        assert!(recs.windows(2).all(|w| w[1].seq > w[0].seq));
    }
}
