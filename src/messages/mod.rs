use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub seq: i64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    pub routed: String,
    pub status: String,
    pub detail: String,
}

#[async_trait::async_trait]
pub trait MessageStore: Send + Sync {
    async fn append(&self, rec: &MessageRecord) -> anyhow::Result<()>;
    async fn read_all(&self) -> anyhow::Result<Vec<MessageRecord>>;
    async fn replace_all(&self, recs: &[MessageRecord]) -> anyhow::Result<()>;
}
