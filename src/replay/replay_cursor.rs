use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCursor {
    pub replay_id: String,
    pub round_id: String,
    pub current_step: u64,
    pub total_steps: u64,
    pub current_timestamp_ms: i64,
}

impl ReplayCursor {
    pub fn new(replay_id: String, round_id: String) -> Self {
        Self {
            replay_id,
            round_id,
            current_step: 0,
            total_steps: 0,
            current_timestamp_ms: 0,
        }
    }
}
