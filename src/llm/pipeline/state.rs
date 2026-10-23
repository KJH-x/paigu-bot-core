use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use crate::domain::claim::Eligibility;
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::snapshot::AllocationSnapshot;
use crate::messages::MessageRecord as LogMessageRecord;
use crate::replay::session::ReplayResult;

use super::Pipeline;

#[derive(Default, Clone)]
pub(super) struct State {
    pub(super) source_cut: Option<HashSet<String>>,
    pub(super) memo: crate::replay::session::Memo,
    pub(super) ingress_seen: HashSet<crate::bus::IngressSource>,
    pub(super) ingress_pending:
        std::collections::HashMap<crate::bus::IngressSource, LogMessageRecord>,
    pub(super) ingress_loaded: bool,
    pub(super) ingress_dirty: bool,
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

#[derive(Clone)]
pub(super) struct MessageRecord {
    pub(super) delegation: Option<crate::bus::Delegation>,
    pub(super) original_automatic: Option<crate::messages::processing::AutomaticSummary>,
    pub(super) manual_note: Option<String>,
    pub(super) record_id: String,
    pub(super) record_revision: u64,
    pub(super) round_id: String,
    pub(super) seq: i64,
    pub(super) user_id: String,
    pub(super) display: String,
    pub(super) text: String,
    pub(super) status: String,
    pub(super) detail: String,
}

impl MessageRecord {
    pub(super) fn from_log(record: &LogMessageRecord, round_id: &str, display: &str) -> Self {
        Self {
            delegation: record.delegation.clone(),
            original_automatic: crate::messages::processing::original_summary(record),
            manual_note: None,
            record_id: record.record_id.clone(),
            record_revision: record.record_revision,
            round_id: round_id.to_string(),
            seq: record.seq,
            user_id: record.user_id.clone(),
            display: display.to_string(),
            text: record.text.clone(),
            status: record.status.clone(),
            detail: record.detail.clone(),
        }
    }
}

impl Pipeline {
    #[cfg(test)]
    pub async fn reset(&self) {
        let _serial = self.message_write_guard().await;
        self.reset_locked().await;
    }

    #[cfg(test)]
    pub(crate) async fn reset_locked(&self) {
        let mut state = self.state.lock().await;
        *state = State::default();
    }

    pub(crate) async fn fresh_cut_locked(&self, records: &[LogMessageRecord]) {
        *self.state.lock().await = State {
            source_cut: (!records.is_empty())
                .then(|| records.iter().map(|r| r.record_id.clone()).collect()),
            ..State::default()
        };
    }

    /// Continue switches use the selected round's own in-process cache, never the
    /// previous round's board. Replay remains an explicit, separate operation.
    pub(crate) async fn has_parked_round(&self, round: &str) -> bool {
        self.parked.lock().await.contains_key(round)
    }

    pub(crate) async fn switch_round_locked(&self, previous: &str, next: &str, fresh: bool) {
        let mut parked = self.parked.lock().await;
        let mut state = self.state.lock().await;
        if previous != next {
            parked.insert(previous.into(), state.clone());
            *state = if fresh {
                State::default()
            } else {
                parked.remove(next).unwrap_or_default()
            };
        } else if fresh {
            *state = State::default();
        }
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

    /// Restore a persisted settlement lock without adding an artificial phase event.
    pub(crate) async fn restore_settlement_lock(&self, locked: bool) {
        if locked {
            self.state.lock().await.locked = true;
        }
    }

    /// 锁/解锁：写 `RoundPhaseSet` 事件（事件溯源的锁=阶段），并即时更新内存缓存。
    /// `true` → 阶段覆盖为 `Settling`（锁定）；`false` → 清除覆盖。
    #[cfg(test)]
    pub async fn set_locked(&self, locked: bool) -> anyhow::Result<()> {
        let _serial = self.message_write_guard().await;
        let cfg = self.cfg.get().await;
        self.set_locked_locked(&cfg, locked).await
    }

    pub(crate) async fn set_locked_locked(
        &self,
        cfg: &crate::settings::AppConfig,
        locked: bool,
    ) -> anyhow::Result<()> {
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
        self.messages
            .append_durable_event(&cfg.round.round_id, &serde_json::to_value(&event)?)
            .await?;
        self.state.lock().await.locked = locked;
        Ok(())
    }

    /// 将持久化日志的权威重放结果一次性应用到实时管线。
    pub async fn apply_replay(&self, result: &ReplayResult, records: &[LogMessageRecord]) {
        let mut state = self.state.lock().await;
        let locked = result.locked;
        let mut seen = HashSet::new();
        let mut identity = std::collections::HashMap::new();
        for record in records {
            if record.status != "Pending" {
                seen.insert(format!("{}::{}", record.group_id, record.message_id));
            }
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
                delegation: message.delegation.clone(),
                original_automatic: message.original_automatic.clone(),
                manual_note: message.manual_note.clone(),
                record_id: message.record_id.clone(),
                record_revision: message.record_revision,
                round_id: result.round_id.clone(),
                seq: message.seq,
                user_id: message.user_id.clone(),
                display: message.nickname.clone(),
                text: message.text.clone(),
                status: message.status.clone(),
                detail: message.detail.clone(),
            })
            .collect();
        *state = State {
            source_cut: None,
            memo: Default::default(),
            ingress_seen: records
                .iter()
                .filter(|r| r.status != "Pending")
                .filter_map(|r| r.ingress.clone())
                .collect(),
            ingress_pending: records
                .iter()
                .filter(|r| r.status == "Pending")
                .filter_map(|r| r.ingress.clone().map(|source| (source, r.clone())))
                .collect(),
            ingress_loaded: true,
            ingress_dirty: false,
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
                    "original_automatic":m.original_automatic,
                    "manual_note":m.manual_note,
                    "record_id": m.record_id,
                    "record_revision": m.record_revision,
                    "round_id": m.round_id,
                    "seq": m.seq,
                    "user_id": m.user_id,
                    "delegation": m.delegation,
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

        let mut grouped: BTreeMap<(String, String), (String, BTreeMap<String, u32>)> =
            BTreeMap::new();
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
                .entry((display, uid))
                .or_insert_with(|| (identity, BTreeMap::new()));
            for item in &summary.items {
                *entry.1.entry(item.item_name.clone()).or_insert(0) += item.quantity;
            }
        }

        grouped
            .into_iter()
            .map(|((display, uid), (identity, items))| {
                let items: Vec<Value> = items
                    .into_iter()
                    .map(|(name, qty)| json!({ "name": name, "qty": qty }))
                    .collect();
                json!({ "user_id": uid, "display": display, "identity": identity, "items": items })
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
