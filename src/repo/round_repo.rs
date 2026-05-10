use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::ids::{UserId, RoundId, ItemId, EventId};
use crate::domain::round::{Round, RoundStatus};
use crate::domain::item::{Item, ItemAlias};
use crate::domain::claim::Eligibility;
use crate::domain::event::EventEnvelope;
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::settlement::SettlementSnapshot;
use crate::error::AppResult;

#[async_trait]
pub trait RoundRepo: Send + Sync {
    async fn find_by_id(&self, round_id: &RoundId) -> AppResult<Option<Round>>;
    async fn find_active_by_group(&self, group_id: &str) -> AppResult<Vec<Round>>;
    async fn find_all_by_group(&self, group_id: &str) -> AppResult<Vec<Round>>;
    async fn insert(&self, round: &Round) -> AppResult<Round>;
    async fn update_status(&self, round_id: &RoundId, status: RoundStatus) -> AppResult<()>;
}

#[async_trait]
pub trait ItemRepo: Send + Sync {
    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<Item>>;
    async fn find_by_id(&self, item_id: &ItemId) -> AppResult<Option<Item>>;
    async fn insert(&self, item: &Item) -> AppResult<Item>;
    async fn find_aliases_by_round(&self, round_id: &RoundId) -> AppResult<Vec<ItemAlias>>;
}

#[async_trait]
pub trait EventRepo: Send + Sync {
    async fn insert(&self, event: &EventEnvelope) -> AppResult<EventEnvelope>;
    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<EventEnvelope>>;
    async fn find_by_id(&self, event_id: &EventId) -> AppResult<Option<EventEnvelope>>;
    async fn get_max_sequence(&self, round_id: &RoundId) -> AppResult<i64>;
}

#[async_trait]
pub trait SnapshotRepo: Send + Sync {
    async fn save_allocation(&self, snapshot: &AllocationSnapshot) -> AppResult<()>;
    async fn save_settlement(&self, snapshot: &SettlementSnapshot) -> AppResult<()>;
    async fn get_latest_allocation(&self, round_id: &RoundId) -> AppResult<Option<AllocationSnapshot>>;
    async fn get_latest_settlement(&self, round_id: &RoundId) -> AppResult<Option<SettlementSnapshot>>;
}

#[async_trait]
pub trait RawMessageRepo: Send + Sync {
    async fn insert_raw_message(&self, msg: &RawMessageRecord) -> AppResult<RawMessageRecord>;
    async fn find_by_message_id(&self, group_id: &str, qq_message_id: &str) -> AppResult<Option<RawMessageRecord>>;
}

#[derive(Debug, Clone)]
pub struct RawMessageRecord {
    pub raw_message_id: String,
    pub group_id: String,
    pub user_id: String,
    pub qq_message_id: String,
    pub timestamp: DateTime<Utc>,
    pub text: Option<String>,
    pub images: serde_json::Value,
    pub is_admin: bool,
}

#[async_trait]
pub trait EligibilityRepo: Send + Sync {
    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<Eligibility>>;
    async fn find_by_user_and_round(&self, user_id: &UserId, round_id: &RoundId) -> AppResult<Vec<Eligibility>>;
    async fn insert(&self, eligibility: &Eligibility) -> AppResult<Eligibility>;
}
