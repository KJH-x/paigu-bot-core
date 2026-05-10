use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::{EventId, RoundId, UserId, ItemId, ClaimId};
use crate::domain::claim::{ClaimLine, SlotPolicy};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum DomainEvent {
    ClaimCreated(ClaimCreated),
    ClaimCancelled(ClaimCancelled),
    ClaimModified(ClaimModified),
    AdminAllocationAdjusted(AdminAllocationAdjusted),
    AdminSlotLocked(AdminSlotLocked),
    AdminSlotUnlocked(AdminSlotUnlocked),
    DiscountRulesSet(DiscountRulesSet),
    RoundClosed(RoundClosed),
    RoundOpened(RoundOpened),
    ParseOverride(ParseOverrideEvent),
}

impl DomainEvent {
    pub fn event_type_str(&self) -> &'static str {
        match self {
            DomainEvent::ClaimCreated(_) => "claim_created",
            DomainEvent::ClaimCancelled(_) => "claim_cancelled",
            DomainEvent::ClaimModified(_) => "claim_modified",
            DomainEvent::AdminAllocationAdjusted(_) => "admin_allocation_adjusted",
            DomainEvent::AdminSlotLocked(_) => "admin_slot_locked",
            DomainEvent::AdminSlotUnlocked(_) => "admin_slot_unlocked",
            DomainEvent::DiscountRulesSet(_) => "discount_rules_set",
            DomainEvent::RoundClosed(_) => "round_closed",
            DomainEvent::RoundOpened(_) => "round_opened",
            DomainEvent::ParseOverride(_) => "parse_override",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub round_id: RoundId,
    pub group_id: String,
    pub user_id: UserId,
    pub raw_message_id: Option<String>,
    pub event_type: String,
    pub effective_at: DateTime<Utc>,
    pub sequence: i64,
    pub payload: DomainEvent,
    pub status: EventStatus,
}

impl EventEnvelope {
    pub fn event_id(&self) -> &EventId {
        &self.event_id
    }

    pub fn raw_message_id_str(&self) -> Option<&str> {
        self.raw_message_id.as_deref()
    }

    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.effective_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventStatus {
    Active,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCreated {
    pub claim_id: ClaimId,
    pub user_id: UserId,
    pub items: Vec<ClaimLine>,
    pub source_text: String,
    pub parse_trace: Option<crate::domain::snapshot::ParseTrace>,
    pub validation_trace: Vec<crate::domain::snapshot::ValidationTraceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCancelled {
    pub target_claim_id: Option<ClaimId>,
    pub target_item_id: Option<ItemId>,
    pub quantity: Option<u32>,
    pub reason: Option<String>,
    pub parse_trace: Option<crate::domain::snapshot::ParseTrace>,
    pub validation_trace: Vec<crate::domain::snapshot::ValidationTraceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimModified {
    pub target_claim_id: ClaimId,
    pub target_item_id: Option<ItemId>,
    pub new_quantity: Option<u32>,
    pub new_slot_policy: Option<SlotPolicy>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAllocationAdjusted {
    pub adjustment_id: String,
    pub action: AdminAllocationAction,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminAllocationAction {
    FixUserToSlot {
        item_id: ItemId,
        user_id: UserId,
        box_index: u32,
        slot_index: u32,
    },
    LockSlot {
        item_id: ItemId,
        box_index: u32,
        slot_index: u32,
        reason: Option<String>,
    },
    UnlockSlot {
        item_id: ItemId,
        box_index: u32,
        slot_index: u32,
    },
    RemoveUserItem {
        item_id: ItemId,
        user_id: UserId,
        quantity: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSlotLocked {
    pub item_id: ItemId,
    pub box_index: u32,
    pub slot_index: u32,
    pub reason: Option<String>,
    pub locked_by: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSlotUnlocked {
    pub item_id: ItemId,
    pub box_index: u32,
    pub slot_index: u32,
    pub unlocked_by: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountRulesSet {
    pub rules: Vec<crate::domain::discount::DiscountRule>,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundClosed {
    pub closed_by: UserId,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundOpened {
    pub opened_by: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseOverrideEvent {
    pub event_id: EventId,
    pub round_id: RoundId,
    pub target_raw_message_id: String,
    pub corrected_parsed_message: serde_json::Value,
    pub admin_user_id: UserId,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventOrderKey {
    pub priority_level: i32,
    pub effective_timestamp_ms: i64,
    pub source_sequence: i64,
    pub message_id: String,
}

pub fn compare_event_order(a: &EventEnvelope, b: &EventEnvelope) -> std::cmp::Ordering {
    a.effective_at.cmp(&b.effective_at)
        .then_with(|| a.sequence.cmp(&b.sequence))
}

pub fn compare_claim_line_order(a: &crate::domain::claim::EffectiveClaimLine,
    b: &crate::domain::claim::EffectiveClaimLine) -> std::cmp::Ordering {
    b.priority_level.cmp(&a.priority_level)
        .then_with(|| a.effective_at.cmp(&b.effective_at))
        .then_with(|| a.sequence.cmp(&b.sequence))
        .then_with(|| a.line_index.cmp(&b.line_index))
}
