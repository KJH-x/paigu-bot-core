use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub replay_id: String,
    pub round_id: String,
    pub total_steps: u64,
    pub successful_steps: u64,
    pub failed_steps: u64,
    pub warnings: Vec<String>,
    pub final_summary: String,
}

impl ReplayReport {
    pub fn new(replay_id: String, round_id: String) -> Self {
        Self {
            replay_id,
            round_id,
            total_steps: 0,
            successful_steps: 0,
            failed_steps: 0,
            warnings: vec![],
            final_summary: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStepSummary {
    pub step_index: u64,
    pub event_id: String,
    pub raw_message_id: Option<String>,
    pub occurred_at_ms: i64,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub event_kind: String,
    pub summary: String,
    pub status: ReplayStepStatus,
    pub changed_slot_count: u32,
    pub warning_count: u32,
    pub error_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayStepStatus {
    Applied,
    PartiallyApplied,
    Ignored,
    ParseFailed,
    ValidationFailed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedReplayStep {
    pub replay_id: String,
    pub step_index: u64,
    pub raw_message_id: String,
    pub raw_text: String,
    pub failure_stage: FailureStage,
    pub error_code: String,
    pub error_message: String,
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureStage {
    Parse,
    Validation,
    Allocation,
    Settlement,
}
