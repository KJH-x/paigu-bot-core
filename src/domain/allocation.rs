use serde::{Deserialize, Serialize};

use crate::domain::ids::{ItemId, UserId, ClaimId};
use crate::domain::money::MoneyCents;
use crate::domain::claim::{ClaimType, SlotPolicy};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Filled,
    LockedEmpty,
    AdminReserved,
}

impl SlotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SlotStatus::Empty => "empty",
            SlotStatus::Filled => "filled",
            SlotStatus::LockedEmpty => "locked_empty",
            SlotStatus::AdminReserved => "admin_reserved",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotAllocation {
    pub slot_index: u32,
    pub user_id: Option<UserId>,
    pub claim_id: Option<ClaimId>,
    pub claim_line_index: Option<u32>,
    pub status: SlotStatus,
    pub slot_policy: SlotPolicy,
    pub segment_id: Option<String>,
    pub lock_reason: Option<String>,
}

impl SlotAllocation {
    pub fn empty(index: u32) -> Self {
        Self {
            slot_index: index,
            user_id: None,
            claim_id: None,
            claim_line_index: None,
            status: SlotStatus::Empty,
            slot_policy: SlotPolicy::Normal,
            segment_id: None,
            lock_reason: None,
        }
    }

    pub fn locked_empty(index: u32, policy: SlotPolicy, segment_id: Option<String>) -> Self {
        Self {
            slot_index: index,
            user_id: None,
            claim_id: None,
            claim_line_index: None,
            status: SlotStatus::LockedEmpty,
            slot_policy: policy,
            segment_id,
            lock_reason: None,
        }
    }

    pub fn is_fillable(&self) -> bool {
        self.status == SlotStatus::Empty
            && self.slot_policy == SlotPolicy::Normal
            && self.segment_id.is_none()
    }

    pub fn user_id_str(&self) -> Option<&str> {
        self.user_id.as_ref().map(|u| u.0.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxAllocation {
    pub box_index: u32,
    pub slots: Vec<SlotAllocation>,
}

impl BoxAllocation {
    pub fn new(box_index: u32) -> Self {
        Self {
            box_index,
            slots: Vec::new(),
        }
    }

    pub fn slot(&self, index: usize) -> Option<&SlotAllocation> {
        self.slots.get(index)
    }

    pub fn slot_mut(&mut self, index: usize) -> Option<&mut SlotAllocation> {
        self.slots.get_mut(index)
    }

    pub fn first_empty_normal_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_fillable())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAllocation {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: String,
    pub boxes: Vec<BoxAllocation>,
    pub singles: Vec<SingleAllocation>,
    pub waiting: Vec<WaitingLine>,
}

impl ItemAllocation {
    pub fn box_at(&self, index: u32) -> Option<&BoxAllocation> {
        self.boxes.iter().find(|b| b.box_index == index)
    }

    pub fn box_at_mut(&mut self, index: u32) -> Option<&mut BoxAllocation> {
        self.boxes.iter_mut().find(|b| b.box_index == index)
    }

    pub fn get_or_create_box(&mut self, box_index: u32) -> &mut BoxAllocation {
        let pos = self.boxes.iter().position(|b| b.box_index == box_index);
        if let Some(pos) = pos {
            &mut self.boxes[pos]
        } else {
            let new_box = BoxAllocation::new(box_index);
            self.boxes.push(new_box);
            self.boxes.last_mut().unwrap()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleAllocation {
    pub user_id: UserId,
    pub claim_id: ClaimId,
    pub item_id: ItemId,
    pub quantity: u32,
    pub unit_price: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitingLine {
    pub user_id: UserId,
    pub claim_id: ClaimId,
    pub item_id: ItemId,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub priority_level: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAllocationSummary {
    pub user_id: UserId,
    pub display_name: String,
    pub items: Vec<UserItemAllocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserItemAllocation {
    pub item_id: ItemId,
    pub item_name: String,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub unit_price: MoneyCents,
    pub gross: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationWarning {
    pub item_id: ItemId,
    pub user_id: Option<UserId>,
    pub message: String,
    pub severity: WarningSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WarningSeverity {
    Info,
    Warning,
    Error,
}
