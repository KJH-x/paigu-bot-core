use async_trait::async_trait;
use std::collections::HashMap;

use crate::replay::timeline_snapshot::TimelineSnapshotRecord;
use crate::replay::replay_engine::ReplayResult;
use crate::error::AppResult;

#[async_trait]
pub trait TimelineStore: Send + Sync {
    async fn save_step(&self, record: &TimelineSnapshotRecord) -> AppResult<()>;
    async fn get_step(&self, replay_id: &str, step_index: u64) -> AppResult<Option<TimelineSnapshotRecord>>;
    async fn list_steps(&self, replay_id: &str) -> AppResult<Vec<(u64, TimelineSnapshotRecord)>>;
    async fn save_replay_result(&self, result: &ReplayResult) -> AppResult<()>;
}

pub struct InMemoryTimelineStore {
    records: tokio::sync::RwLock<HashMap<String, Vec<TimelineSnapshotRecord>>>,
}

impl InMemoryTimelineStore {
    pub fn new() -> Self {
        Self {
            records: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TimelineStore for InMemoryTimelineStore {
    async fn save_step(&self, record: &TimelineSnapshotRecord) -> AppResult<()> {
        let mut records = self.records.write().await;
        records.entry(record.replay_id.clone())
            .or_default()
            .push(record.clone());
        Ok(())
    }

    async fn get_step(&self, replay_id: &str, step_index: u64) -> AppResult<Option<TimelineSnapshotRecord>> {
        let records = self.records.read().await;
        if let Some(replay_records) = records.get(replay_id) {
            Ok(replay_records.get(step_index as usize).cloned())
        } else {
            Ok(None)
        }
    }

    async fn list_steps(&self, replay_id: &str) -> AppResult<Vec<(u64, TimelineSnapshotRecord)>> {
        let records = self.records.read().await;
        if let Some(replay_records) = records.get(replay_id) {
            Ok(replay_records.iter().enumerate().map(|(i, r)| (i as u64, r.clone())).collect())
        } else {
            Ok(vec![])
        }
    }

    async fn save_replay_result(&self, _result: &ReplayResult) -> AppResult<()> {
        Ok(())
    }
}
