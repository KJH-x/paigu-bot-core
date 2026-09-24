use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};

use crate::domain::claim::Eligibility;
use crate::domain::event::EventEnvelope;
use crate::domain::snapshot::AllocationSnapshot;

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
    pub(super) display: String,
    pub(super) text: String,
    pub(super) status: String,
    pub(super) detail: String,
}

impl Pipeline {
    pub async fn reset(&self) {
        crate::messages::reset_seq();
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
