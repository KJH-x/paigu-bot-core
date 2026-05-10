use async_trait::async_trait;
use std::path::PathBuf;

use crate::domain::ids::RoundId;
use crate::domain::snapshot::PublicSnapshot;
use crate::error::PublishError;

pub struct LocalPublisher {
    pub base_path: PathBuf,
}

impl LocalPublisher {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
}

#[async_trait]
impl super::r2_publisher::SnapshotPublisher for LocalPublisher {
    async fn publish_current(&self, round_id: &RoundId, snapshot: &PublicSnapshot) -> Result<(), PublishError> {
        let dir = self.base_path.join("rounds").join(&round_id.0);
        tokio::fs::create_dir_all(&dir).await.map_err(|e| PublishError::S3(e.to_string()))?;

        let path = dir.join("current.json");
        let content = serde_json::to_string_pretty(snapshot)
            .map_err(|e| PublishError::Serialization(e))?;
        tokio::fs::write(&path, content).await.map_err(|e| PublishError::S3(e.to_string()))?;

        Ok(())
    }

    async fn publish_versioned(&self, round_id: &RoundId, version: i64, snapshot: &PublicSnapshot) -> Result<(), PublishError> {
        let dir = self.base_path.join("rounds").join(&round_id.0).join("snapshots");
        tokio::fs::create_dir_all(&dir).await.map_err(|e| PublishError::S3(e.to_string()))?;

        let path = dir.join(format!("{}.json", version));
        let content = serde_json::to_string_pretty(snapshot)
            .map_err(|e| PublishError::Serialization(e))?;
        tokio::fs::write(&path, content).await.map_err(|e| PublishError::S3(e.to_string()))?;

        Ok(())
    }
}
