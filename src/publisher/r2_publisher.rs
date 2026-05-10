use async_trait::async_trait;

use crate::domain::ids::RoundId;
use crate::domain::snapshot::PublicSnapshot;
use crate::error::PublishError;

#[async_trait]
pub trait SnapshotPublisher: Send + Sync {
    async fn publish_current(&self, round_id: &RoundId, snapshot: &PublicSnapshot) -> Result<(), PublishError>;
    async fn publish_versioned(&self, round_id: &RoundId, version: i64, snapshot: &PublicSnapshot) -> Result<(), PublishError>;
}

pub struct R2Publisher {
    pub bucket: String,
    pub client: Option<aws_sdk_s3::Client>,
}

impl R2Publisher {
    pub fn new(bucket: String) -> Self {
        Self {
            bucket,
            client: None,
        }
    }
}

#[async_trait]
impl SnapshotPublisher for R2Publisher {
    async fn publish_current(&self, round_id: &RoundId, snapshot: &PublicSnapshot) -> Result<(), PublishError> {
        let key = format!("rounds/{}/current.json", round_id.0);
        let body = serde_json::to_vec(snapshot).map_err(|e| PublishError::Serialization(e))?;

        if let Some(ref client) = self.client {
            client.put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(aws_sdk_s3::primitives::ByteStream::from(body))
                .content_type("application/json")
                .send()
                .await
                .map_err(|e| PublishError::S3(e.to_string()))?;
        }

        Ok(())
    }

    async fn publish_versioned(&self, round_id: &RoundId, version: i64, snapshot: &PublicSnapshot) -> Result<(), PublishError> {
        let key = format!("rounds/{}/snapshots/{}.json", round_id.0, version);
        let body = serde_json::to_vec(snapshot).map_err(|e| PublishError::Serialization(e))?;

        if let Some(ref client) = self.client {
            client.put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(aws_sdk_s3::primitives::ByteStream::from(body))
                .content_type("application/json")
                .send()
                .await
                .map_err(|e| PublishError::S3(e.to_string()))?;
        }

        Ok(())
    }
}
