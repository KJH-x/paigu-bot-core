use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationReport {
    pub replay_id: String,
    pub round_id: String,
    pub input_message_count: u64,
    pub parsed_event_count: u64,
    pub applied_event_count: u64,
    pub parse_failure_count: u64,
    pub validation_failure_count: u64,
    pub allocation_warning_count: u64,
    pub final_allocated_claim_count: u64,
    pub final_unallocated_claim_count: u64,
    pub final_total_amount: i64,
    pub manifest_path: String,
    pub final_snapshot_path: String,
    pub failed_messages_path: String,
    pub warnings_path: String,
}
