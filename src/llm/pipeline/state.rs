use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use crate::domain::claim::Eligibility;
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::snapshot::AllocationSnapshot;
use crate::messages::MessageRecord as LogMessageRecord;
use crate::replay::session::ReplayResult;

use super::Pipeline;

#[derive(Default)]
pub(super) struct State {
    pub(super) events: Vec<EventEnvelope>,
    pub(super) messages: Vec<MessageRecord>,
    pub(super) seen: HashSet<String>,
    pub(super) eligibilities: Vec<Eligibility>,
    pub(super) display: std::collections::HashMap<String, String>,
    pub(super) identity: std::collections::HashMap<String, String>,
    pub(super) seq: i64,
    pub(super) version: i64,
    pub(super) snapshot: Option<AllocationSnapshot>,
    /// 管理员 `/锁位` `/结团` 后为 true，不再接受排/撤/改（`/开团` 解锁）。
    pub(super) locked: bool,
}

pub(super) struct MessageRecord {
    pub(super) seq: i64,
    pub(super) user_id: String,
    pub(super) display: String,
    pub(super) text: String,
    pub(super) status: String,
    pub(super) detail: String,
}

impl Pipeline {
    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = State::default();
        *self.merge_open.lock().await = None;
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

    pub async fn is_locked(&self) -> bool {
        self.state.lock().await.locked
    }

    /// 锁/解锁：写 `RoundPhaseSet` 事件（事件溯源的锁=阶段），并即时更新内存缓存。
    /// `true` → 阶段覆盖为 `Settling`（锁定）；`false` → 清除覆盖。
    pub async fn set_locked(&self, locked: bool) {
        let cfg = self.cfg.get().await;
        let by = crate::domain::ids::UserId("admin".to_string());
        let event = EventEnvelope {
            event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
            round_id: crate::domain::ids::RoundId(cfg.round.round_id.clone()),
            group_id: cfg.round.group_id.clone(),
            user_id: by.clone(),
            raw_message_id: None,
            event_type: "round_phase_set".to_string(),
            effective_at: chrono::Utc::now(),
            sequence: crate::messages::next_seq(),
            payload: DomainEvent::RoundPhaseSet(crate::domain::event::RoundPhaseSet {
                phase: if locked {
                    Some(crate::round::RoundPhase::Settling)
                } else {
                    None
                },
                set_by: by,
            }),
            status: crate::domain::event::EventStatus::Active,
        };
        if let Ok(value) = serde_json::to_value(&event) {
            if let Err(e) = self.messages.append_raw_event(&cfg.round.round_id, &value).await {
                tracing::warn!("RoundPhaseSet 事件写入失败: {e}");
            }
        }
        self.state.lock().await.locked = locked;
    }

    /// 将持久化日志的权威重放结果一次性应用到实时管线。
    pub async fn apply_replay(&self, result: &ReplayResult, records: &[LogMessageRecord]) {
        let mut state = self.state.lock().await;
        let locked = result.locked;
        let mut seen = HashSet::new();
        let mut identity = std::collections::HashMap::new();
        for record in records {
            seen.insert(format!("{}::{}", record.group_id, record.message_id));
            let (clean, _) = crate::parser::normalize::clean_nickname(&record.nickname);
            identity.insert(record.user_id.clone(), clean);
        }
        let display = result
            .display_names
            .iter()
            .map(|(user_id, name)| (user_id.0.clone(), name.clone()))
            .collect();
        let messages = result
            .messages
            .iter()
            .map(|message| MessageRecord {
                seq: message.seq,
                user_id: message.user_id.clone(),
                display: message.nickname.clone(),
                text: message.text.clone(),
                status: message.status.clone(),
                detail: message.detail.clone(),
            })
            .collect();
        *state = State {
            events: result.events.clone(),
            messages,
            seen,
            eligibilities: result.eligibilities.clone(),
            display,
            identity,
            seq: records.iter().map(|record| record.seq).max().unwrap_or(0),
            version: result.version,
            snapshot: Some(result.board.clone()),
            locked,
        };
    }

    pub async fn messages_since(&self, since: i64) -> Vec<Value> {
        let state = self.state.lock().await;
        state
            .messages
            .iter()
            // 消息流不展示被丢弃（非白名单群/非群消息等）记录，避免刷屏。
            .filter(|m| m.seq > since && m.status != "Dropped")
            .map(|m| {
                json!({
                    "seq": m.seq,
                    "user_id": m.user_id,
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
            let display = state
                .display
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| uid.clone());
            let identity = state
                .identity
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| display.clone());
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

    /// 工作流只读快照（供 `GET /api/workflow` 与前端 Stepper）。
    pub async fn workflow(&self) -> Value {
        let state = self.state.lock().await;
        let claims: u64 = state
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .user_summaries
                    .iter()
                    .flat_map(|summary| summary.items.iter())
                    .map(|item| item.quantity as u64)
                    .sum()
            })
            .unwrap_or(0);
        json!({
            "locked": state.locked,
            "version": state.version,
            "events": state.events.len(),
            "messages": state.messages.len(),
            "claims": claims,
            "eligibilities": state.eligibilities.len(),
        })
    }
}
