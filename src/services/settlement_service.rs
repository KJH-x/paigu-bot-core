use std::sync::Arc;

use crate::domain::ids::RoundId;
use crate::domain::settlement::SettlementSnapshot;
use crate::engine::settlement_engine::SettlementEngine;
use crate::repo::round_repo::SnapshotRepo;
use crate::error::AppResult;

pub struct SettlementService {
    pub settlement_engine: SettlementEngine,
    pub snapshot_repo: Arc<dyn SnapshotRepo>,
}

impl SettlementService {
    pub fn new(snapshot_repo: Arc<dyn SnapshotRepo>) -> Self {
        Self {
            settlement_engine: SettlementEngine::new(),
            snapshot_repo,
        }
    }

    pub async fn get_latest_settlement(&self, round_id: &RoundId) -> AppResult<Option<SettlementSnapshot>> {
        self.snapshot_repo.get_latest_settlement(round_id).await
    }
}
