use serde::{Deserialize, Serialize};

use crate::domain::ids::ItemId;
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::allocation::SlotStatus;
use crate::domain::claim::SlotPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    pub slot_changes: Vec<SlotChange>,
    pub claim_changes: Vec<ClaimChange>,
    pub user_total_changes: Vec<UserTotalChange>,
    pub item_total_changes: Vec<ItemTotalChange>,
    pub settlement_changes: Vec<SettlementChange>,
}

impl StateDiff {
    pub fn from_snapshots(before: &AllocationSnapshot, after: &AllocationSnapshot) -> Self {
        let mut slot_changes = Vec::new();

        for after_ia in &after.item_allocations {
            let before_ia = before.item_allocations.iter()
                .find(|i| i.item_id == after_ia.item_id);

            for after_box in &after_ia.boxes {
                let before_box = before_ia.and_then(|i| i.boxes.iter().find(|b| b.box_index == after_box.box_index));

                for after_slot in &after_box.slots {
                    let before_slot = before_box.and_then(|b| b.slots.iter().find(|s| s.slot_index == after_slot.slot_index));

                    let changed = match before_slot {
                        Some(bs) => bs.status != after_slot.status || bs.user_id != after_slot.user_id,
                        None => true,
                    };

                    if changed {
                        let reason = match (before_slot, after_slot) {
                            (Some(bs), as_) if bs.status == SlotStatus::Empty && as_.status == SlotStatus::Filled => {
                                if as_.slot_policy == SlotPolicy::TailLocked {
                                    SlotChangeReason::TailSegmentCreated
                                } else if as_.slot_policy == SlotPolicy::AdminFixed {
                                    SlotChangeReason::AdminFixed
                                } else {
                                    SlotChangeReason::NewClaimFilled
                                }
                            }
                            (Some(bs), as_) if bs.status == SlotStatus::Filled && as_.status == SlotStatus::Empty => {
                                SlotChangeReason::CancelReleased
                            }
                            (None, as_) => {
                                if as_.slot_policy == SlotPolicy::TailLocked {
                                    SlotChangeReason::TailSegmentCreated
                                } else if as_.slot_policy == SlotPolicy::AdminFixed {
                                    SlotChangeReason::AdminFixed
                                } else {
                                    SlotChangeReason::NewClaimFilled
                                }
                            }
                            _ => SlotChangeReason::NewClaimFilled,
                        };
                        slot_changes.push(SlotChange {
                            item_id: after_ia.item_id.clone(),
                            box_index: after_box.box_index,
                            slot_index: after_slot.slot_index,
                            before: before_slot.map(|s| SlotView {
                                status: s.status.clone(),
                                user_id: s.user_id.clone().map(|u| u.0),
                                nickname: None,
                                claim_id: s.claim_id.clone().map(|c| c.0),
                                segment_id: s.segment_id.clone(),
                                slot_policy: s.slot_policy.clone(),
                                lock_reason: s.lock_reason.clone(),
                            }),
                            after: Some(SlotView {
                                status: after_slot.status.clone(),
                                user_id: after_slot.user_id.clone().map(|u| u.0),
                                nickname: None,
                                claim_id: after_slot.claim_id.clone().map(|c| c.0),
                                segment_id: after_slot.segment_id.clone(),
                                slot_policy: after_slot.slot_policy.clone(),
                                lock_reason: after_slot.lock_reason.clone(),
                            }),
                            reason,
                            causality: ChangeCausality::Direct,
                        });
                    }
                }
            }
        }

        use std::collections::HashSet;

        let mut changed_claim_ids: HashSet<String> = HashSet::new();
        for change in &slot_changes {
            if let Some(ref after_view) = change.after {
                if let Some(ref claim_id) = after_view.claim_id {
                    changed_claim_ids.insert(claim_id.clone());
                }
            }
            if let Some(ref before_view) = change.before {
                if let Some(ref claim_id) = before_view.claim_id {
                    changed_claim_ids.insert(claim_id.clone());
                }
            }
        }

        let claim_changes: Vec<ClaimChange> = changed_claim_ids.into_iter().map(|id| {
            let before_status = before.item_allocations.iter()
                .flat_map(|ia| ia.boxes.iter())
                .flat_map(|b| b.slots.iter())
                .find(|s| s.claim_id.as_ref().map(|c| &c.0) == Some(&id))
                .map(|s| s.status.as_str().to_string());
            let after_status = after.item_allocations.iter()
                .flat_map(|ia| ia.boxes.iter())
                .flat_map(|b| b.slots.iter())
                .find(|s| s.claim_id.as_ref().map(|c| &c.0) == Some(&id))
                .map(|s| s.status.as_str().to_string());
            ClaimChange {
                claim_id: id,
                before_status,
                after_status,
            }
        }).collect();

        let mut user_total_changes = Vec::new();
        for after_summary in &after.user_summaries {
            let before_summary = before.user_summaries.iter().find(|s| s.user_id == after_summary.user_id);
            let after_total: i64 = after_summary.items.iter().map(|i| i.gross.0).sum();
            let before_total: i64 = before_summary.map(|s| s.items.iter().map(|i| i.gross.0).sum()).unwrap_or(0);
            if before_total != after_total {
                user_total_changes.push(UserTotalChange {
                    user_id: after_summary.user_id.0.clone(),
                    before_total,
                    after_total,
                });
            }
        }
        for before_summary in &before.user_summaries {
            if !after.user_summaries.iter().any(|s| s.user_id == before_summary.user_id) {
                let before_total: i64 = before_summary.items.iter().map(|i| i.gross.0).sum();
                user_total_changes.push(UserTotalChange {
                    user_id: before_summary.user_id.0.clone(),
                    before_total,
                    after_total: 0,
                });
            }
        }

        let mut all_item_ids: HashSet<String> = HashSet::new();
        for s in &before.user_summaries {
            for i in &s.items {
                all_item_ids.insert(i.item_id.0.clone());
            }
        }
        for s in &after.user_summaries {
            for i in &s.items {
                all_item_ids.insert(i.item_id.0.clone());
            }
        }

        let item_total_changes: Vec<ItemTotalChange> = all_item_ids.into_iter().map(|id| {
            let before_qty: u32 = before.user_summaries.iter()
                .flat_map(|s| s.items.iter())
                .filter(|i| i.item_id.0 == id)
                .map(|i| i.quantity)
                .sum();
            let after_qty: u32 = after.user_summaries.iter()
                .flat_map(|s| s.items.iter())
                .filter(|i| i.item_id.0 == id)
                .map(|i| i.quantity)
                .sum();
            ItemTotalChange {
                item_id: id,
                before_quantity: before_qty,
                after_quantity: after_qty,
            }
        }).filter(|c| c.before_quantity != c.after_quantity).collect();

        let settlement_changes: Vec<SettlementChange> = user_total_changes.iter().map(|uc| {
            SettlementChange {
                user_id: uc.user_id.clone(),
                before_amount: uc.before_total,
                after_amount: uc.after_total,
                description: format!("金额 {} -> {}", uc.before_total, uc.after_total),
            }
        }).collect();

        StateDiff {
            slot_changes,
            claim_changes,
            user_total_changes,
            item_total_changes,
            settlement_changes,
        }
    }

    pub fn empty() -> Self {
        Self {
            slot_changes: vec![],
            claim_changes: vec![],
            user_total_changes: vec![],
            item_total_changes: vec![],
            settlement_changes: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotChange {
    pub item_id: ItemId,
    pub box_index: u32,
    pub slot_index: u32,
    pub before: Option<SlotView>,
    pub after: Option<SlotView>,
    pub reason: SlotChangeReason,
    pub causality: ChangeCausality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotView {
    pub status: SlotStatus,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub claim_id: Option<String>,
    pub segment_id: Option<String>,
    pub slot_policy: SlotPolicy,
    pub lock_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotChangeReason {
    NewClaimFilled,
    CancelReleased,
    AutoMovedForward,
    TailSegmentCreated,
    TailSegmentUpdated,
    AdminFixed,
    AdminUnlocked,
    RecomputedByRuleChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChangeCausality {
    Direct,
    Cascade,
    Recalculation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimChange {
    pub claim_id: String,
    pub before_status: Option<String>,
    pub after_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTotalChange {
    pub user_id: String,
    pub before_total: i64,
    pub after_total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemTotalChange {
    pub item_id: String,
    pub before_quantity: u32,
    pub after_quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementChange {
    pub user_id: String,
    pub before_amount: i64,
    pub after_amount: i64,
    pub description: String,
}
