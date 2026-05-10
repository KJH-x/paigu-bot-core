use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::{UserId, ItemId, ClaimId, RoundId, EligibilityId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimType {
    Split,
    Single,
    GiftClaim,
}

impl ClaimType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimType::Split => "split",
            ClaimType::Single => "single",
            ClaimType::GiftClaim => "gift_claim",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "split" => Some(ClaimType::Split),
            "single" => Some(ClaimType::Single),
            "giftclaim" | "gift_claim" | "gift" => Some(ClaimType::GiftClaim),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotPolicy {
    Normal,
    TailLocked,
    ColumnLocked,
    AdminFixed,
}

impl SlotPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            SlotPolicy::Normal => "normal",
            SlotPolicy::TailLocked => "tail_locked",
            SlotPolicy::ColumnLocked => "column_locked",
            SlotPolicy::AdminFixed => "admin_fixed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimLine {
    pub item_id: ItemId,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub slot_policy: SlotPolicy,
    pub is_proxy_card: bool,
    pub notes: Option<String>,
}

impl ClaimLine {
    pub fn new_split(item_id: impl Into<ItemId>, quantity: u32) -> Self {
        Self {
            item_id: item_id.into(),
            quantity,
            claim_type: ClaimType::Split,
            slot_policy: SlotPolicy::Normal,
            is_proxy_card: false,
            notes: None,
        }
    }

    pub fn new_single(item_id: impl Into<ItemId>, quantity: u32) -> Self {
        Self {
            item_id: item_id.into(),
            quantity,
            claim_type: ClaimType::Single,
            slot_policy: SlotPolicy::Normal,
            is_proxy_card: false,
            notes: None,
        }
    }

    pub fn new_tail_locked(item_id: impl Into<ItemId>, quantity: u32) -> Self {
        Self {
            item_id: item_id.into(),
            quantity,
            claim_type: ClaimType::Split,
            slot_policy: SlotPolicy::TailLocked,
            is_proxy_card: false,
            notes: Some("包尾".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: ClaimId,
    pub round_id: RoundId,
    pub user_id: UserId,
    pub items: Vec<ClaimLine>,
    pub source_text: String,
    pub effective_at: DateTime<Utc>,
    pub sequence: i64,
    pub status: ClaimStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimStatus {
    Active,
    Cancelled,
    PartiallyCancelled,
    Modified,
}

impl Claim {
    pub fn is_empty(&self) -> bool {
        self.items.iter().all(|l| l.quantity == 0)
    }

    pub fn cancel_all(&mut self) {
        for line in &mut self.items {
            line.quantity = 0;
        }
        self.status = ClaimStatus::Cancelled;
    }

    pub fn cancel_item_quantity(&mut self, item_id: &ItemId, quantity: u32) -> u32 {
        let mut remaining = quantity;
        for line in &mut self.items {
            if &line.item_id == item_id && remaining > 0 {
                let cancelled = line.quantity.min(remaining);
                line.quantity -= cancelled;
                remaining -= cancelled;
            }
        }
        if self.is_empty() {
            self.status = ClaimStatus::Cancelled;
        }
        quantity - remaining
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eligibility {
    pub eligibility_id: EligibilityId,
    pub round_id: RoundId,
    pub user_id: UserId,
    pub priority_type: String,
    pub priority_level: i32,
    pub scope: EligibilityScope,
    pub max_uses: Option<i32>,
    pub used_count: i32,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilityScope {
    pub item_ids: Option<Vec<ItemId>>,
    pub item_kinds: Option<Vec<String>>,
    pub only_before_start_minutes: Option<i32>,
}

impl Eligibility {
    pub fn applies_to_item(&self, item_id: &ItemId) -> bool {
        if let Some(ref item_ids) = self.scope.item_ids {
            if !item_ids.is_empty() && !item_ids.contains(item_id) {
                return false;
            }
        }
        true
    }

    pub fn applies_to_item_kind(&self, kind: &str) -> bool {
        if let Some(ref kinds) = self.scope.item_kinds {
            if !kinds.is_empty() && !kinds.contains(&kind.to_string()) {
                return false;
            }
        }
        true
    }

    pub fn applies_at(&self, at: DateTime<Utc>) -> bool {
        if let Some(valid_from) = self.valid_from {
            if at < valid_from {
                return false;
            }
        }
        if let Some(valid_until) = self.valid_until {
            if at >= valid_until {
                return false;
            }
        }
        true
    }

    pub fn has_uses_remaining(&self) -> bool {
        if let Some(max) = self.max_uses {
            self.used_count < max
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveClaimLine {
    pub claim_id: ClaimId,
    pub line_index: u32,
    pub user_id: UserId,
    pub item_id: ItemId,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub slot_policy: SlotPolicy,
    pub effective_at: DateTime<Utc>,
    pub sequence: i64,
    pub priority_level: i32,
}

impl EffectiveClaimLine {
    pub fn compute_priority(user_id: &UserId, item_id: &ItemId, effective_at: DateTime<Utc>,
        eligibilities: &[Eligibility]) -> i32 {
        eligibilities
            .iter()
            .filter(|e| &e.user_id == user_id)
            .filter(|e| e.applies_to_item(item_id))
            .filter(|e| e.applies_at(effective_at))
            .filter(|e| e.has_uses_remaining())
            .map(|e| e.priority_level)
            .max()
            .unwrap_or(0)
    }
}
