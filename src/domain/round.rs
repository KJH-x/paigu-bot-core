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

impl RoundStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoundStatus::Draft => "draft",
            RoundStatus::Scheduled => "scheduled",
            RoundStatus::Active => "active",
            RoundStatus::Settling => "settling",
            RoundStatus::Closed => "closed",
            RoundStatus::Archived => "archived",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(RoundStatus::Draft),
            "scheduled" => Some(RoundStatus::Scheduled),
            "active" => Some(RoundStatus::Active),
            "settling" => Some(RoundStatus::Settling),
            "closed" => Some(RoundStatus::Closed),
            "archived" => Some(RoundStatus::Archived),
            _ => None,
        }
    }

    pub fn allows_claims(&self) -> bool {
        matches!(self, RoundStatus::Active)
    }

    pub fn allows_admin_commands(&self) -> bool {
        matches!(self, RoundStatus::Draft | RoundStatus::Scheduled | RoundStatus::Active | RoundStatus::Settling)
    }
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

impl Round {
    pub fn is_active_at(&self, at: DateTime<Utc>) -> bool {
        if self.status != RoundStatus::Active {
            return false;
        }
        if let Some(start) = self.start_at {
            if at < start {
                return false;
            }
        }
        if let Some(end) = self.end_at {
            if at >= end {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundConfig {
    pub round: Round,
    pub items: Vec<crate::domain::item::Item>,
    pub aliases: Vec<crate::domain::item::ItemAlias>,
    pub eligibility: Vec<crate::domain::claim::Eligibility>,
}
