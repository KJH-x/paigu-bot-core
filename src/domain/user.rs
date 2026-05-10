use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::UserId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_id: UserId,
    pub qq_id: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub fn new(user_id: UserId, qq_id: String, display_name: String) -> Self {
        let now = Utc::now();
        Self {
            user_id,
            qq_id,
            display_name,
            created_at: now,
            updated_at: now,
        }
    }
}
