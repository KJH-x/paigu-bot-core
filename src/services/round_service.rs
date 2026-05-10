use std::sync::Arc;
use chrono::Utc;

use crate::domain::ids::RoundId;
use crate::domain::round::{Round, RoundStatus};
use crate::repo::round_repo::RoundRepo;
use crate::error::AppResult;

pub struct RoundService {
    pub round_repo: Arc<dyn RoundRepo>,
}

impl RoundService {
    pub fn new(round_repo: Arc<dyn RoundRepo>) -> Self {
        Self { round_repo }
    }

    pub async fn create_round(
        &self,
        title: String,
        group_id: String,
        created_by: String,
        start_at: Option<chrono::DateTime<chrono::Utc>>,
        end_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Round> {
        let round_id = RoundId(uuid::Uuid::new_v4().to_string());
        let now = Utc::now();
        let status = if start_at.is_some() {
            RoundStatus::Scheduled
        } else {
            RoundStatus::Active
        };

        let round = Round {
            round_id,
            group_id,
            title,
            status,
            start_at,
            end_at,
            allow_cancel: true,
            allow_modify: true,
            default_timezone: "Asia/Shanghai".to_string(),
            created_by,
            created_at: now,
            updated_at: now,
        };

        self.round_repo.insert(&round).await
    }

    pub async fn close_round(&self, round_id: &RoundId) -> AppResult<()> {
        self.round_repo.update_status(round_id, RoundStatus::Closed).await
    }

    pub async fn get_active_rounds(&self, group_id: &str) -> AppResult<Vec<Round>> {
        self.round_repo.find_active_by_group(group_id).await
    }
}
