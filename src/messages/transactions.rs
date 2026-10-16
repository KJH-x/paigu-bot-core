use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::domain::event::EventEnvelope;
use crate::domain::snapshot::AllocationSnapshot;
use crate::messages::processing::ProcessingMember;
use crate::settings::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub value: Value,
    pub reason: String,
    #[serde(default)]
    pub absorbed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub record_id: String,
    pub revision: u64,
    pub before: Option<Choice>,
    pub after: Option<Choice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub policies: Value,
    pub policy_hash: String,
    pub message_refs: Vec<ProcessingMember>,
    pub automatic_refs: BTreeMap<String, Vec<String>>,
    pub event_cursor: usize,
    pub event_prefix_hash: String,
    pub phase_hash: String,
    pub selections: BTreeMap<String, Choice>,
    pub events: Vec<EventEnvelope>,
    pub board: AllocationSnapshot,
    pub settlement: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub action_transaction_version: u8,
    pub transaction_id: String,
    pub request_hash: String,
    pub kind: String,
    pub main_record_id: String,
    pub config_revision: u64,
    pub undo_of: Option<String>,
    pub changes: Vec<Change>,
    pub checkpoint: Checkpoint,
}

pub fn hash(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

pub fn policies(cfg: &AppConfig) -> Value {
    json!({"round":cfg.round,"whitelist_groups":cfg.gateway.whitelist_groups,
        "whitelist_members":cfg.gateway.whitelist_members,"cn_overrides":cfg.members.cn_overrides,"merge":cfg.merge})
}

pub fn decode(event: &EventEnvelope) -> Option<Transaction> {
    if let crate::domain::event::DomainEvent::ParseOverride(o) = &event.payload {
        let t: Transaction = serde_json::from_value(o.corrected_parsed_message.clone()).ok()?;
        if t.action_transaction_version == 1 {
            return Some(t);
        }
    }
    None
}

pub fn phase_hash(events: &[EventEnvelope]) -> anyhow::Result<String> {
    hash(
        &events
            .iter()
            .filter(|e| {
                matches!(
                    e.payload,
                    crate::domain::event::DomainEvent::RoundPhaseSet(_)
                )
            })
            .collect::<Vec<_>>(),
    )
}
