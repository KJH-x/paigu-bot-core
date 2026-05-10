use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditDecisionTrace {
    pub step_index: u64,
    pub event_id: String,
    pub decision_type: String,
    pub raw_message: Option<String>,
    pub parsed_intent: Option<String>,
    pub validation_result: Option<String>,
    pub allocation_result: Option<String>,
    pub explanation: String,
}

impl AuditDecisionTrace {
    pub fn new(
        step_index: u64,
        event_id: String,
        decision_type: String,
        explanation: String,
    ) -> Self {
        Self {
            step_index,
            event_id,
            decision_type,
            raw_message: None,
            parsed_intent: None,
            validation_result: None,
            allocation_result: None,
            explanation,
        }
    }
}
