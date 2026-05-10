use std::collections::HashMap;

use crate::domain::ids::{ItemId, UserId};
use crate::domain::item::Item;
use crate::domain::claim::{EffectiveClaimLine, ClaimType, SlotPolicy};
use crate::domain::event::{EventEnvelope, DomainEvent, AdminAllocationAction};
use crate::domain::allocation::{
    ItemAllocation, BoxAllocation, SlotAllocation, SlotStatus,
    SingleAllocation, WaitingLine, UserAllocationSummary, UserItemAllocation,
};
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::money::MoneyCents;
use crate::error::AppResult;

pub struct AllocationEngine {}

impl AllocationEngine {
    pub fn new() -> Self {
        Self {}
    }

    pub fn allocate(
        &self,
        items: &[Item],
        claim_lines: &[EffectiveClaimLine],
        events: &[EventEnvelope],
    ) -> AppResult<AllocationSnapshot> {
        let now = chrono::Utc::now();
        let round_id = items.first().map(|i| i.round_id.clone()).unwrap_or_else(|| crate::domain::ids::RoundId("unknown".to_string()));
        let version = 1i64;

        let mut sorted_lines = claim_lines.to_vec();
        sorted_lines.sort_by(|a, b| {
            b.priority_level.cmp(&a.priority_level)
                .then_with(|| a.effective_at.cmp(&b.effective_at))
                .then_with(|| a.sequence.cmp(&b.sequence))
                .then_with(|| a.line_index.cmp(&b.line_index))
        });

        let mut item_states: HashMap<ItemId, ItemWorkingState> = HashMap::new();
        let item_map: HashMap<ItemId, &Item> = items.iter().map(|i| (i.item_id.clone(), i)).collect();

        for item in items {
            item_states.insert(item.item_id.clone(), ItemWorkingState::new(item));
        }

        self.apply_admin_constraints(&mut item_states, events);

        for line in &sorted_lines {
            if let Some(state) = item_states.get_mut(&line.item_id) {
                match line.claim_type {
                    ClaimType::Split | ClaimType::GiftClaim => {
                        self.allocate_split_line(state, line);
                    }
                    ClaimType::Single => {
                        let max_qty = item_map.get(&line.item_id).and_then(|item| item.max_quantity);
                        state.allocate_single(line, max_qty);
                    }
                }
            }
        }

        let mut item_allocations = Vec::new();
        let mut user_summaries_map: HashMap<UserId, Vec<UserItemAllocation>> = HashMap::new();
        let warnings = Vec::new();

        for (item_id, state) in &item_states {
            let item = item_map.get(item_id);
            let item_name = item.map(|i| i.name.clone()).unwrap_or_default();
            let kind = item.map(|i| i.kind.as_str().to_string()).unwrap_or_default();

            let boxes: Vec<BoxAllocation> = state.boxes.values()
                .map(|b| BoxAllocation {
                    box_index: b.box_index,
                    slots: b.slots.clone(),
                })
                .collect();

            for mbox in state.boxes.values() {
                for slot in &mbox.slots {
                    if let Some(ref uid) = slot.user_id {
                        if let Some(item) = item {
                            let entry = user_summaries_map.entry(uid.clone()).or_default();
                            if let Some(existing) = entry.iter_mut().find(|e| e.item_id == *item_id) {
                                existing.quantity += 1;
                                existing.gross = existing.gross.checked_add(item.unit_price).unwrap_or(existing.gross);
                            } else {
                                entry.push(UserItemAllocation {
                                    item_id: item_id.clone(),
                                    item_name: item.name.clone(),
                                    quantity: 1,
                                    claim_type: ClaimType::Split,
                                    unit_price: item.unit_price,
                                    gross: item.unit_price,
                                });
                            }
                        }
                    }
                }
            }

            for sa in &state.singles {
                if let Some(item) = item {
                    let entry = user_summaries_map.entry(sa.user_id.clone()).or_default();
                    entry.push(UserItemAllocation {
                        item_id: item_id.clone(),
                        item_name: item.name.clone(),
                        quantity: sa.quantity,
                        claim_type: ClaimType::Single,
                        unit_price: sa.unit_price,
                        gross: sa.unit_price.checked_mul_u32(sa.quantity).unwrap_or(MoneyCents::zero()),
                    });
                }
            }

            item_allocations.push(ItemAllocation {
                item_id: item_id.clone(),
                item_name,
                kind,
                boxes,
                singles: state.singles.clone(),
                waiting: state.waiting.clone(),
            });
        }

        let user_summaries: Vec<UserAllocationSummary> = user_summaries_map.into_iter()
            .map(|(uid, items)| UserAllocationSummary {
                user_id: uid,
                display_name: String::new(),
                items,
            })
            .collect();

        Ok(AllocationSnapshot {
            round_id,
            version,
            generated_at: now,
            item_allocations,
            user_summaries,
            warnings,
        })
    }

    fn allocate_split_line(&self, state: &mut ItemWorkingState, line: &EffectiveClaimLine) {
        match line.slot_policy {
            SlotPolicy::TailLocked => {
                self.allocate_tail_locked(state, line);
            }
            SlotPolicy::AdminFixed => {
                // Admin-fixed placements are handled separately
            }
            _ => {
                self.allocate_normal(state, line);
            }
        }
    }

    fn allocate_normal(&self, state: &mut ItemWorkingState, line: &EffectiveClaimLine) {
        for _ in 0..line.quantity {
            let slot = state.find_first_fillable_normal_slot();
            match slot {
                Some((box_idx, slot_idx)) => {
                    state.fill_slot(box_idx, slot_idx, line, SlotPolicy::Normal, None);
                }
                None => {
                    let (box_idx, slot_idx) = state.create_next_box_and_first_slot();
                    state.fill_slot(box_idx, slot_idx, line, SlotPolicy::Normal, None);
                }
            }
        }
    }

    fn allocate_tail_locked(&self, state: &mut ItemWorkingState, line: &EffectiveClaimLine) {
        let segment_id = format!("tail:{}:{}:{}", line.user_id.0, line.claim_id.0, line.line_index);
        let box_idx = state.next_box_index();

        for i in 0..line.quantity {
            let slot_idx = i + 1;
            state.ensure_slot_exists(box_idx, slot_idx);
            state.fill_slot(box_idx, slot_idx, line, SlotPolicy::TailLocked, Some(segment_id.clone()));
        }

        if let Some(box_size) = state.box_size {
            for slot_idx in (line.quantity + 1)..=box_size {
                state.ensure_slot_exists(box_idx, slot_idx);
                state.mark_locked_empty(box_idx, slot_idx, SlotPolicy::TailLocked, Some(segment_id.clone()));
            }
        }
    }

    fn apply_admin_constraints(
        &self,
        item_states: &mut HashMap<ItemId, ItemWorkingState>,
        events: &[EventEnvelope],
    ) {
        for ev in events {
            match &ev.payload {
                DomainEvent::AdminAllocationAdjusted(adj) => {
                    match &adj.action {
                        AdminAllocationAction::FixUserToSlot { item_id, user_id, box_index, slot_index } => {
                            if let Some(state) = item_states.get_mut(item_id) {
                                state.admin_fix_slot(*box_index, *slot_index, user_id.clone());
                            }
                        }
                        AdminAllocationAction::LockSlot { item_id, box_index, slot_index, reason } => {
                            if let Some(state) = item_states.get_mut(item_id) {
                                state.admin_lock_slot(*box_index, *slot_index, reason.clone());
                            }
                        }
                        AdminAllocationAction::UnlockSlot { item_id, box_index, slot_index } => {
                            if let Some(state) = item_states.get_mut(item_id) {
                                state.admin_unlock_slot(*box_index, *slot_index);
                            }
                        }
                        AdminAllocationAction::RemoveUserItem { item_id, user_id, quantity } => {
                            if let Some(state) = item_states.get_mut(item_id) {
                                state.remove_user_quantity(user_id, *quantity);
                            }
                        }
                    }
                }
                DomainEvent::AdminSlotLocked(lock) => {
                    if let Some(state) = item_states.get_mut(&lock.item_id) {
                        state.admin_lock_slot(lock.box_index, lock.slot_index, lock.reason.clone());
                    }
                }
                DomainEvent::AdminSlotUnlocked(unlock) => {
                    if let Some(state) = item_states.get_mut(&unlock.item_id) {
                        state.admin_unlock_slot(unlock.box_index, unlock.slot_index);
                    }
                }
                _ => {}
            }
        }
    }
}

struct BoxWorkingState {
    box_index: u32,
    slots: Vec<SlotAllocation>,
}

struct ItemWorkingState {
    box_size: Option<u32>,
    boxes: HashMap<u32, BoxWorkingState>,
    singles: Vec<SingleAllocation>,
    waiting: Vec<WaitingLine>,
    next_box: u32,
    next_slot_in_box: HashMap<u32, u32>,
}

impl ItemWorkingState {
    fn new(item: &Item) -> Self {
        Self {
            box_size: item.box_size,
            boxes: HashMap::new(),
            singles: Vec::new(),
            waiting: Vec::new(),
            next_box: 1,
            next_slot_in_box: HashMap::new(),
        }
    }

    fn next_box_index(&mut self) -> u32 {
        let idx = self.next_box;
        self.next_box += 1;
        idx
    }

    fn find_first_fillable_normal_slot(&self) -> Option<(u32, u32)> {
        let mut box_indices: Vec<u32> = self.boxes.keys().copied().collect();
        box_indices.sort();
        for bi in box_indices {
            if let Some(mbox) = self.boxes.get(&bi) {
                for (j, slot) in mbox.slots.iter().enumerate() {
                    if slot.is_fillable() {
                        return Some((bi, (j + 1) as u32));
                    }
                }
            }
        }
        None
    }

    fn ensure_slot_exists(&mut self, box_index: u32, slot_index: u32) {
        let mbox = self.boxes.entry(box_index).or_insert_with(|| BoxWorkingState {
            box_index,
            slots: Vec::new(),
        });
        while mbox.slots.len() < slot_index as usize {
            mbox.slots.push(SlotAllocation::empty((mbox.slots.len() + 1) as u32));
        }
    }

    fn fill_slot(&mut self, box_index: u32, slot_index: u32, line: &EffectiveClaimLine, policy: SlotPolicy, segment_id: Option<String>) {
        self.ensure_slot_exists(box_index, slot_index);
        let mbox = self.boxes.get_mut(&box_index).unwrap();
        let idx = (slot_index - 1) as usize;
        if idx < mbox.slots.len() {
            mbox.slots[idx] = SlotAllocation {
                slot_index,
                user_id: Some(line.user_id.clone()),
                claim_id: Some(line.claim_id.clone()),
                claim_line_index: Some(line.line_index),
                status: SlotStatus::Filled,
                slot_policy: policy,
                segment_id,
                lock_reason: None,
            };
        }
    }

    fn mark_locked_empty(&mut self, box_index: u32, slot_index: u32, policy: SlotPolicy, segment_id: Option<String>) {
        self.ensure_slot_exists(box_index, slot_index);
        let mbox = self.boxes.get_mut(&box_index).unwrap();
        let idx = (slot_index - 1) as usize;
        if idx < mbox.slots.len() {
            mbox.slots[idx] = SlotAllocation::locked_empty(slot_index, policy, segment_id);
        }
    }

    fn create_next_box_and_first_slot(&mut self) -> (u32, u32) {
        let box_idx = self.next_box_index();
        if let Some(size) = self.box_size {
            for i in 1..=size {
                self.ensure_slot_exists(box_idx, i);
            }
        } else {
            self.ensure_slot_exists(box_idx, 1);
        }
        (box_idx, 1)
    }

    fn allocate_single(&mut self, line: &EffectiveClaimLine, max_quantity: Option<u32>) {
        let current_total: u32 = self.singles.iter().map(|s| s.quantity).sum();
        let available = match max_quantity {
            Some(max) if current_total >= max => {
                self.waiting.push(WaitingLine {
                    user_id: line.user_id.clone(),
                    claim_id: line.claim_id.clone(),
                    item_id: line.item_id.clone(),
                    quantity: line.quantity,
                    claim_type: line.claim_type.clone(),
                    priority_level: line.priority_level,
                });
                return;
            }
            Some(max) => {
                let remaining = max - current_total;
                line.quantity.min(remaining)
            }
            None => line.quantity,
        };

        self.singles.push(SingleAllocation {
            user_id: line.user_id.clone(),
            claim_id: line.claim_id.clone(),
            item_id: line.item_id.clone(),
            quantity: available,
            unit_price: MoneyCents::zero(),
        });

        if available < line.quantity {
            self.waiting.push(WaitingLine {
                user_id: line.user_id.clone(),
                claim_id: line.claim_id.clone(),
                item_id: line.item_id.clone(),
                quantity: line.quantity - available,
                claim_type: line.claim_type.clone(),
                priority_level: line.priority_level,
            });
        }
    }

    fn admin_fix_slot(&mut self, box_index: u32, slot_index: u32, user_id: crate::domain::ids::UserId) {
        self.ensure_slot_exists(box_index, slot_index);
        let mbox = self.boxes.get_mut(&box_index).unwrap();
        let idx = (slot_index - 1) as usize;
        if idx < mbox.slots.len() {
            mbox.slots[idx].user_id = Some(user_id);
            mbox.slots[idx].status = SlotStatus::AdminReserved;
            mbox.slots[idx].slot_policy = SlotPolicy::AdminFixed;
        }
    }

    fn admin_lock_slot(&mut self, box_index: u32, slot_index: u32, reason: Option<String>) {
        self.ensure_slot_exists(box_index, slot_index);
        let mbox = self.boxes.get_mut(&box_index).unwrap();
        let idx = (slot_index - 1) as usize;
        if idx < mbox.slots.len() {
            mbox.slots[idx].status = SlotStatus::LockedEmpty;
            mbox.slots[idx].slot_policy = SlotPolicy::AdminFixed;
            mbox.slots[idx].lock_reason = reason;
        }
    }

    fn admin_unlock_slot(&mut self, box_index: u32, slot_index: u32) {
        self.ensure_slot_exists(box_index, slot_index);
        let mbox = self.boxes.get_mut(&box_index).unwrap();
        let idx = (slot_index - 1) as usize;
        if idx < mbox.slots.len() {
            mbox.slots[idx].status = SlotStatus::Empty;
            mbox.slots[idx].slot_policy = SlotPolicy::Normal;
            mbox.slots[idx].lock_reason = None;
            mbox.slots[idx].user_id = None;
            mbox.slots[idx].claim_id = None;
        }
    }

    fn remove_user_quantity(&mut self, user_id: &crate::domain::ids::UserId, quantity: u32) {
        let mut remaining = quantity;
        for mbox in self.boxes.values_mut() {
            for slot in mbox.slots.iter_mut() {
                if remaining == 0 {
                    break;
                }
                if slot.user_id.as_ref() == Some(user_id) && slot.status == SlotStatus::Filled {
                    slot.user_id = None;
                    slot.claim_id = None;
                    slot.status = SlotStatus::Empty;
                    slot.slot_policy = SlotPolicy::Normal;
                    slot.segment_id = None;
                    remaining -= 1;
                }
            }
        }
    }
}
