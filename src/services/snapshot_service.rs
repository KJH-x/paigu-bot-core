use std::sync::Arc;

use crate::domain::ids::RoundId;
use crate::domain::snapshot::AllocationSnapshot;
use crate::repo::round_repo::SnapshotRepo;
use crate::error::AppResult;
use crate::publisher::r2_publisher::SnapshotPublisher;

pub struct SnapshotService {
    pub snapshot_repo: Arc<dyn SnapshotRepo>,
    pub publisher: Option<Arc<dyn SnapshotPublisher>>,
}

impl SnapshotService {
    pub fn new(snapshot_repo: Arc<dyn SnapshotRepo>, publisher: Option<Arc<dyn SnapshotPublisher>>) -> Self {
        Self { snapshot_repo, publisher }
    }

    pub async fn save_and_publish(
        &self,
        snapshot: &AllocationSnapshot,
        title: &str,
        status: &str,
    ) -> AppResult<()> {
        self.snapshot_repo.save_allocation(snapshot).await?;

        if let Some(ref publisher) = self.publisher {
            let public = snapshot.to_public(title, status);
            publisher.publish_current(&snapshot.round_id, &public).await
                .map_err(|e| crate::error::AppError::Publish(e))?;
        }

        Ok(())
    }

    pub async fn get_latest(&self, round_id: &RoundId) -> AppResult<Option<AllocationSnapshot>> {
        self.snapshot_repo.get_latest_allocation(round_id).await
    }
}
