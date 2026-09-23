use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::RoundId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoundStatus {
    Draft,
    Scheduled,
    Active,
    Settling,
    Closed,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub round_id: RoundId,
    pub group_id: String,
    pub title: String,
    pub status: RoundStatus,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub allow_cancel: bool,
    pub allow_modify: bool,
    pub default_timezone: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundConfig {
    pub round: Round,
    pub items: Vec<crate::domain::item::Item>,
    pub aliases: Vec<crate::domain::item::ItemAlias>,
    pub eligibility: Vec<crate::domain::claim::Eligibility>,
}
