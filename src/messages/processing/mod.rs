use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{ClaimId, EventId};
use crate::parser::parsed_event::ParsedMessage;
use crate::round::RoundPhase;

use super::MessageRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnderstandingSource {
    Rules,
    Llm,
    RulesFallback,
    FirstMatchLlm,
    Control,
    Duplicate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingOutcome {
    pub status: String,
    pub detail: String,
    pub reply: Option<String>,
}

impl ProcessingOutcome {
    pub fn new(status: &str, detail: impl Into<String>, reply: Option<String>) -> Self {
        Self {
            status: status.to_string(),
            detail: detail.into(),
            reply,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Understanding {
    pub source: UnderstandingSource,
    pub parsed: Option<ParsedMessage>,
    /// Canonical business events before permission checks; never accepted in an HTTP edit DTO.
    pub events: Vec<EventEnvelope>,
    pub validation: ProcessingOutcome,
    pub is_modify: bool,
}

impl Understanding {
    pub fn terminal(
        source: UnderstandingSource,
        status: &str,
        detail: impl Into<String>,
        reply: Option<String>,
    ) -> Self {
        Self {
            source,
            parsed: None,
            events: vec![],
            validation: ProcessingOutcome::new(status, detail, reply),
            is_modify: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessingMember {
    pub record_id: String,
    pub input_hash: String,
}

impl ProcessingMember {
    pub fn from_record(record: &MessageRecord) -> Self {
        // Outcome/revision/routing are projections and must not invalidate the understood input.
        let input = serde_json::json!([
            record.record_id,
            record.seq,
            record.group_id,
            record.user_id,
            record.nickname,
            record.message_id,
            record.text,
            record.timestamp_ms,
            record.is_admin,
            record.sender_user_id,
            record.sender_nickname
        ]);
        Self {
            record_id: record.record_id.clone(),
            input_hash: format!("{:x}", Sha256::digest(input.to_string().as_bytes())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomaticResult {
    pub format_version: u8,
    pub result_id: String,
    pub created_at_ms: i64,
    pub configuration_revision: u64,
    pub members: Vec<ProcessingMember>,
    pub understanding: Understanding,
    pub original_outcome: ProcessingOutcome,
    pub manual_phase_at_ingest: Option<RoundPhase>,
}

impl AutomaticResult {
    pub fn matches(&self, members: &[&MessageRecord]) -> bool {
        self.members == processing_members(members)
    }
}

pub fn processing_members(records: &[&MessageRecord]) -> Vec<ProcessingMember> {
    let mut ordered = records.to_vec();
    ordered.sort_by_key(|record| (record.timestamp_ms, record.seq));
    ordered
        .into_iter()
        .map(ProcessingMember::from_record)
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessingHistory {
    #[serde(default)]
    pub results: Vec<AutomaticResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomaticSummary {
    pub source: UnderstandingSource,
    pub outcome: ProcessingOutcome,
}

pub fn original_summary(record: &MessageRecord) -> Option<AutomaticSummary> {
    record
        .processing
        .as_ref()?
        .results
        .first()
        .map(|result| AutomaticSummary {
            source: result.understanding.source.clone(),
            outcome: result.original_outcome.clone(),
        })
}

impl ProcessingHistory {
    pub fn latest_for(&self, members: &[&MessageRecord]) -> Option<&AutomaticResult> {
        if self.unreadable.is_some() || self.results.last().is_some_and(|r| r.format_version != 1) {
            return None;
        }
        self.results
            .iter()
            .rev()
            .find(|result| result.format_version == 1 && result.matches(members))
    }
}

pub fn deserialize_history<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ProcessingHistory>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(|value| {
        serde_json::from_value(value.clone()).unwrap_or_else(|_| ProcessingHistory {
            results: vec![],
            unreadable: Some(value),
        })
    }))
}

/// A message-owned claim remains addressable after recompute and manual correction.
pub fn stabilize_events(events: &mut [EventEnvelope], record: &MessageRecord) {
    for (index, event) in events.iter_mut().enumerate() {
        event.event_id = EventId(format!("evt-{}-{index}", record.record_id));
        if let DomainEvent::ClaimCreated(claim) = &mut event.payload {
            claim.claim_id = ClaimId(format!("claim-{}", record.record_id));
        }
    }
}

#[cfg(test)]
mod tests;
