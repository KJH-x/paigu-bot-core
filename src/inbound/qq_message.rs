use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingQqMessage {
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub is_admin: bool,
    pub attachments: Vec<serde_json::Value>,
}

impl IncomingQqMessage {
    pub fn is_admin_command_candidate(&self) -> bool {
        self.is_admin && (self.text.starts_with('/') || self.text.starts_with('#'))
    }

    pub fn is_admin(&self) -> bool {
        self.is_admin
    }
}
