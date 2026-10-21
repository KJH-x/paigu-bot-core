use serde::{Deserialize, Serialize};

use crate::domain::allocation::SlotStatus;
use crate::domain::claim::SlotPolicy;
use crate::domain::ids::ItemId;
use crate::domain::snapshot::AllocationSnapshot;

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
        use crate::domain::allocation::SlotAllocation;
        use std::collections::{BTreeMap, BTreeSet};
        fn cells(
            board: &AllocationSnapshot,
        ) -> BTreeMap<(String, Option<String>, u32, u32), &SlotAllocation> {
            board
                .item_allocations
                .iter()
                .flat_map(|item| {
                    item.boxes.iter().flat_map(move |column| {
                        column.slots.iter().map(move |slot| {
                            (
                                (
                                    item.item_id.0.clone(),
                                    item.subitem_id.clone(),
                                    column.box_index,
                                    slot.slot_index,
                                ),
                                slot,
                            )
                        })
                    })
                })
                .collect()
        }
        fn view(slot: &SlotAllocation) -> SlotView {
            SlotView {
                status: slot.status.clone(),
                user_id: slot.user_id.as_ref().map(|id| id.0.clone()),
                nickname: None,
                claim_id: slot.claim_id.as_ref().map(|id| id.0.clone()),
                segment_id: slot.segment_id.clone(),
                slot_policy: slot.slot_policy.clone(),
                lock_reason: slot.lock_reason.clone(),
            }
        }
        let previous = cells(before);
        let current = cells(after);
        let keys: BTreeSet<_> = previous.keys().chain(current.keys()).cloned().collect();
        let mut slot_changes = Vec::new();
        for (item, subitem, column, position) in keys {
            let key = (item.clone(), subitem.clone(), column, position);
            let old = previous.get(&key).copied();
            let new = current.get(&key).copied();
            let unchanged = match (old, new) {
                (Some(a), Some(b)) => {
                    a.status == b.status
                        && a.user_id == b.user_id
                        && a.claim_id == b.claim_id
                        && a.claim_line_index == b.claim_line_index
                        && a.slot_policy == b.slot_policy
                        && a.segment_id == b.segment_id
                        && a.lock_reason == b.lock_reason
                }
                (None, None) => true,
                _ => false,
            };
            if unchanged {
                continue;
            }
            let reason = match (old, new) {
                (Some(a), b)
                    if a.status == SlotStatus::Filled
                        && b.is_none_or(|s| s.status == SlotStatus::Empty) =>
                {
                    SlotChangeReason::CancelReleased
                }
                (_, Some(b)) if b.slot_policy == SlotPolicy::TailLocked => {
                    SlotChangeReason::TailSegmentCreated
                }
                (_, Some(b)) if b.slot_policy == SlotPolicy::AdminFixed => {
                    SlotChangeReason::AdminFixed
                }
                (Some(a), Some(b))
                    if a.status == SlotStatus::LockedEmpty && b.status == SlotStatus::Empty =>
                {
                    SlotChangeReason::AdminUnlocked
                }
                (Some(a), Some(b))
                    if a.status == SlotStatus::Filled
                        && b.status == SlotStatus::Filled
                        && a.user_id != b.user_id =>
                {
                    SlotChangeReason::AutoMovedForward
                }
                _ => SlotChangeReason::NewClaimFilled,
            };
            slot_changes.push(SlotChange {
                item_id: ItemId(item),
                subitem_id: subitem,
                box_index: column,
                slot_index: position,
                before: old.map(view),
                after: new.map(view),
                reason,
                causality: ChangeCausality::Direct,
            });
        }

        let mut changed_claim_ids: BTreeSet<String> = BTreeSet::new();
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

        let claim_changes: Vec<ClaimChange> = changed_claim_ids
            .into_iter()
            .map(|id| {
                let before_status = before
                    .item_allocations
                    .iter()
                    .flat_map(|ia| ia.boxes.iter())
                    .flat_map(|b| b.slots.iter())
                    .find(|s| s.claim_id.as_ref().map(|c| &c.0) == Some(&id))
                    .map(|s| s.status.as_str().to_string());
                let after_status = after
                    .item_allocations
                    .iter()
                    .flat_map(|ia| ia.boxes.iter())
                    .flat_map(|b| b.slots.iter())
                    .find(|s| s.claim_id.as_ref().map(|c| &c.0) == Some(&id))
                    .map(|s| s.status.as_str().to_string());
                ClaimChange {
                    claim_id: id,
                    before_status,
                    after_status,
                }
            })
            .collect();

        let mut user_total_changes = Vec::new();
        for after_summary in &after.user_summaries {
            let before_summary = before
                .user_summaries
                .iter()
                .find(|s| s.user_id == after_summary.user_id);
            let after_total: i64 = after_summary.items.iter().map(|i| i.gross.0).sum();
            let before_total: i64 = before_summary
                .map(|s| s.items.iter().map(|i| i.gross.0).sum())
                .unwrap_or(0);
            if before_total != after_total {
                user_total_changes.push(UserTotalChange {
                    user_id: after_summary.user_id.0.clone(),
                    before_total,
                    after_total,
                });
            }
        }
        for before_summary in &before.user_summaries {
            if !after
                .user_summaries
                .iter()
                .any(|s| s.user_id == before_summary.user_id)
            {
                let before_total: i64 = before_summary.items.iter().map(|i| i.gross.0).sum();
                user_total_changes.push(UserTotalChange {
                    user_id: before_summary.user_id.0.clone(),
                    before_total,
                    after_total: 0,
                });
            }
        }

        let mut all_item_ids: BTreeSet<String> = BTreeSet::new();
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

        let item_total_changes: Vec<ItemTotalChange> = all_item_ids
            .into_iter()
            .map(|id| {
                let before_qty: u32 = before
                    .user_summaries
                    .iter()
                    .flat_map(|s| s.items.iter())
                    .filter(|i| i.item_id.0 == id)
                    .map(|i| i.quantity)
                    .sum();
                let after_qty: u32 = after
                    .user_summaries
                    .iter()
                    .flat_map(|s| s.items.iter())
                    .filter(|i| i.item_id.0 == id)
                    .map(|i| i.quantity)
                    .sum();
                ItemTotalChange {
                    item_id: id,
                    before_quantity: before_qty,
                    after_quantity: after_qty,
                }
            })
            .filter(|c| c.before_quantity != c.after_quantity)
            .collect();

        let settlement_changes: Vec<SettlementChange> = user_total_changes
            .iter()
            .map(|uc| SettlementChange {
                user_id: uc.user_id.clone(),
                before_amount: uc.before_total,
                after_amount: uc.after_total,
                description: format!("金额 {} -> {}", uc.before_total, uc.after_total),
            })
            .collect();

        StateDiff {
            slot_changes,
            claim_changes,
            user_total_changes,
            item_total_changes,
            settlement_changes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotChange {
    pub item_id: ItemId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subitem_id: Option<String>,
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
