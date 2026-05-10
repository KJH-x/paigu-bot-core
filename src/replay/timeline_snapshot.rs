use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::settlement::SettlementSnapshot;
use crate::replay::state_diff::StateDiff;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSnapshotRecord {
    pub replay_id: String,
    pub round_id: String,
    pub step_index: u64,
    pub snapshot_kind: SnapshotKind,
    pub full_snapshot: Option<AllocationSnapshot>,
    pub state_diff: Option<StateDiff>,
    pub settlement_snapshot: Option<SettlementSnapshot>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotKind {
    Full,
    DiffOnly,
}

impl TimelineSnapshotRecord {
    pub fn full(replay_id: String, round_id: String, step_index: u64, full_snapshot: AllocationSnapshot) -> Self {
        Self {
            replay_id,
            round_id,
            step_index,
            snapshot_kind: SnapshotKind::Full,
            full_snapshot: Some(full_snapshot),
            state_diff: None,
            settlement_snapshot: None,
            created_at: Utc::now(),
        }
    }

    pub fn diff_only(replay_id: String, round_id: String, step_index: u64, diff: StateDiff) -> Self {
        Self {
            replay_id,
            round_id,
            step_index,
            snapshot_kind: SnapshotKind::DiffOnly,
            full_snapshot: None,
            state_diff: Some(diff),
            settlement_snapshot: None,
            created_at: Utc::now(),
        }
    }
}
