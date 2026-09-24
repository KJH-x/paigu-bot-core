use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::claim::{ClaimType, SlotPolicy};
use crate::domain::ids::{ClaimId, ItemId, UserId};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;

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

    #[cfg(test)]
    pub fn user_id_str(&self) -> Option<&str> {
        self.user_id.as_ref().map(|u| u.0.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxAllocation {
    pub box_index: u32,
    pub slots: Vec<SlotAllocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAllocation {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: String,
    #[serde(default)]
    pub variant_id: Option<String>,
    pub boxes: Vec<BoxAllocation>,
    pub singles: Vec<SingleAllocation>,
    pub waiting: Vec<WaitingLine>,
}

impl ItemAllocation {
    #[cfg(test)]
    pub fn box_at(&self, index: u32) -> Option<&BoxAllocation> {
        self.boxes.iter().find(|b| b.box_index == index)
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

/// 包尾强制成盒标记（写入 `SlotAllocation.lock_reason`）。
pub const TAIL_FORCED_BOX: &str = "tail_forced_box";

/// §U8 包尾盒解析：**强制成盒** + **自动滑入**（幂等、确定性）。
///
/// 模型：「列」= 盒序号；同一 base item 的各变体在同一列至多占一个槽（共享列）。
///
/// 规则：
/// - 包尾（`TailLocked`）的**锁定列** = 其申报变体集中各变体**普通认购**（`Normal`）的
///   最大列序 + 1；若申报变体无普通认购则取 1。
/// - 包尾人占锁定列中其申报变体的槽；该列其余变体槽保持空置（`LockedEmpty`），整列视为
///   **强制成盒**（`lock_reason = tail_forced_box`）。
/// - **自动滑入**：锁定列之前**未成盒**的列中，若某变体的普通认购**不在**申报变体集内
///   （不冲突），则移入锁定列的对应变体槽；**冲突变体**（属于申报集）不滑入，也不分配给包尾人。
///
/// 幂等性：锁定列只由 `Normal` 槽推导，重复调用结果一致；已滑入的普通槽不会被再次移动。
pub fn resolve_tail_boxes(snapshot: &AllocationSnapshot) -> AllocationSnapshot {
    let mut out = snapshot.clone();

    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, ia) in out.item_allocations.iter().enumerate() {
        groups.entry(ia.item_id.0.clone()).or_default().push(idx);
    }

    for (item_id, indices) in &groups {
        let variant_entries: Vec<(String, usize)> = indices
            .iter()
            .filter_map(|&i| out.item_allocations[i].variant_id.clone().map(|v| (v, i)))
            .collect();
        if variant_entries.is_empty() {
            continue;
        }

        let all_variants: BTreeSet<String> =
            variant_entries.iter().map(|(v, _)| v.clone()).collect();

        let mut tail_groups: BTreeMap<String, Vec<TailSlot>> = BTreeMap::new();
        for (variant, idx) in &variant_entries {
            let entry = &out.item_allocations[*idx];
            for b in &entry.boxes {
                for s in &b.slots {
                    if s.status != SlotStatus::Filled || s.slot_policy != SlotPolicy::TailLocked {
                        continue;
                    }
                    let Some(user_id) = s.user_id.clone() else {
                        continue;
                    };
                    let key = s
                        .segment_id
                        .clone()
                        .unwrap_or_else(|| format!("user:{}", user_id.0));
                    tail_groups.entry(key).or_default().push(TailSlot {
                        variant: variant.clone(),
                        entry_idx: *idx,
                        box_index: b.box_index,
                        slot_index: s.slot_index,
                        user_id,
                        claim_id: s.claim_id.clone(),
                        claim_line_index: s.claim_line_index,
                    });
                }
            }
        }
        if tail_groups.is_empty() {
            continue;
        }

        let mut used_tail_columns: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();

        for slots in tail_groups.values() {
            let declared: BTreeSet<String> = slots.iter().map(|s| s.variant.clone()).collect();

            let mut locked = 1u32;
            for v in &declared {
                let max_normal = max_normal_column(&out, &variant_entries, v);
                locked = locked.max(max_normal.saturating_add(1));
            }
            loop {
                let collides = declared.iter().any(|v| {
                    used_tail_columns
                        .get(v)
                        .map(|cols| cols.contains(&locked))
                        .unwrap_or(false)
                });
                if !collides {
                    break;
                }
                locked = locked.saturating_add(1);
            }

            for s in slots {
                clear_slot(&mut out, s.entry_idx, s.box_index, s.slot_index);
            }
            for s in slots {
                let target = SlotAllocation {
                    slot_index: 1,
                    user_id: Some(s.user_id.clone()),
                    claim_id: s.claim_id.clone(),
                    claim_line_index: s.claim_line_index,
                    status: SlotStatus::Filled,
                    slot_policy: SlotPolicy::TailLocked,
                    segment_id: Some(tail_segment(s)),
                    lock_reason: Some(TAIL_FORCED_BOX.to_string()),
                };
                set_slot(&mut out, s.entry_idx, locked, 1, target);
            }

            // 自动滑入：锁定列之前未成盒的列中，非冲突变体的普通认购移入锁定列。
            let mut columns: BTreeSet<u32> = BTreeSet::new();
            for (_, idx) in &variant_entries {
                for b in &out.item_allocations[*idx].boxes {
                    if b.box_index < locked {
                        columns.insert(b.box_index);
                    }
                }
            }
            for c in columns {
                if column_is_formed(&out, &variant_entries, &all_variants, c) {
                    continue;
                }
                for (variant, idx) in &variant_entries {
                    if declared.contains(variant) {
                        continue;
                    }
                    if locked_slot_filled(&out, *idx, locked) {
                        continue;
                    }
                    if let Some(src) = normal_slot_at(&out, *idx, c) {
                        let moved = SlotAllocation {
                            slot_index: 1,
                            user_id: src.user_id.clone(),
                            claim_id: src.claim_id.clone(),
                            claim_line_index: src.claim_line_index,
                            status: SlotStatus::Filled,
                            slot_policy: SlotPolicy::Normal,
                            segment_id: None,
                            lock_reason: None,
                        };
                        clear_slot(&mut out, *idx, c, src.slot_index);
                        set_slot(&mut out, *idx, locked, 1, moved);
                    }
                }
            }

            // 强制成盒：把锁定列其余变体槽标记为 LockedEmpty（不覆盖已滑入的普通槽）。
            for (variant, idx) in &variant_entries {
                if declared.contains(variant) {
                    continue;
                }
                if !locked_slot_filled(&out, *idx, locked) {
                    let marker = SlotAllocation::locked_empty(
                        locked,
                        SlotPolicy::TailLocked,
                        Some(format!("tail-forced:{item_id}:{locked}")),
                    );
                    let mut marker = marker;
                    marker.lock_reason = Some(TAIL_FORCED_BOX.to_string());
                    set_slot(&mut out, *idx, locked, 1, marker);
                }
            }

            for v in &declared {
                used_tail_columns
                    .entry(v.clone())
                    .or_default()
                    .insert(locked);
            }
            let user_id = slots.first().map(|s| s.user_id.clone());
            let message = format!("包尾强制成盒：{item_id} 列 {locked}");
            if !out.warnings.iter().any(|w| w.message == message) {
                out.warnings.push(AllocationWarning {
                    item_id: ItemId(item_id.clone()),
                    user_id,
                    message,
                    severity: WarningSeverity::Info,
                });
            }
        }

        for (_, idx) in &variant_entries {
            out.item_allocations[*idx]
                .boxes
                .sort_by_key(|b| b.box_index);
        }
    }

    out
}

struct TailSlot {
    variant: String,
    entry_idx: usize,
    box_index: u32,
    slot_index: u32,
    user_id: UserId,
    claim_id: Option<ClaimId>,
    claim_line_index: Option<u32>,
}

fn tail_segment(s: &TailSlot) -> String {
    format!("tail-forced:{}:{}", s.user_id.0, s.box_index)
}

fn max_normal_column(
    snapshot: &AllocationSnapshot,
    variant_entries: &[(String, usize)],
    variant: &str,
) -> u32 {
    let mut max = 0u32;
    for (v, idx) in variant_entries {
        if v != variant {
            continue;
        }
        for b in &snapshot.item_allocations[*idx].boxes {
            let has_normal = b
                .slots
                .iter()
                .any(|s| s.status == SlotStatus::Filled && s.slot_policy == SlotPolicy::Normal);
            if has_normal {
                max = max.max(b.box_index);
            }
        }
    }
    max
}

fn column_is_formed(
    snapshot: &AllocationSnapshot,
    variant_entries: &[(String, usize)],
    all_variants: &BTreeSet<String>,
    column: u32,
) -> bool {
    all_variants.iter().all(|v| {
        variant_entries
            .iter()
            .filter(|(ev, _)| ev == v)
            .any(|(_, idx)| box_has_filled(snapshot, *idx, column))
    })
}

fn box_has_filled(snapshot: &AllocationSnapshot, entry_idx: usize, box_index: u32) -> bool {
    snapshot.item_allocations[entry_idx]
        .boxes
        .iter()
        .find(|b| b.box_index == box_index)
        .map(|b| {
            b.slots
                .iter()
                .any(|s| s.status == SlotStatus::Filled && s.user_id.is_some())
        })
        .unwrap_or(false)
}

fn locked_slot_filled(snapshot: &AllocationSnapshot, entry_idx: usize, box_index: u32) -> bool {
    box_has_filled(snapshot, entry_idx, box_index)
}

fn normal_slot_at(
    snapshot: &AllocationSnapshot,
    entry_idx: usize,
    box_index: u32,
) -> Option<SlotAllocation> {
    snapshot.item_allocations[entry_idx]
        .boxes
        .iter()
        .find(|b| b.box_index == box_index)
        .and_then(|b| {
            b.slots
                .iter()
                .find(|s| {
                    s.status == SlotStatus::Filled
                        && s.slot_policy == SlotPolicy::Normal
                        && s.user_id.is_some()
                })
                .cloned()
        })
}

fn clear_slot(
    snapshot: &mut AllocationSnapshot,
    entry_idx: usize,
    box_index: u32,
    slot_index: u32,
) {
    if let Some(b) = snapshot.item_allocations[entry_idx]
        .boxes
        .iter_mut()
        .find(|b| b.box_index == box_index)
    {
        if let Some(s) = b.slots.iter_mut().find(|s| s.slot_index == slot_index) {
            *s = SlotAllocation::empty(slot_index);
        }
    }
}

fn set_slot(
    snapshot: &mut AllocationSnapshot,
    entry_idx: usize,
    box_index: u32,
    slot_index: u32,
    slot: SlotAllocation,
) {
    let entry = &mut snapshot.item_allocations[entry_idx];
    let b = match entry.boxes.iter_mut().find(|b| b.box_index == box_index) {
        Some(b) => b,
        None => {
            entry.boxes.push(BoxAllocation {
                box_index,
                slots: Vec::new(),
            });
            entry.boxes.last_mut().expect("pushed")
        }
    };
    while b.slots.len() < slot_index as usize {
        b.slots
            .push(SlotAllocation::empty((b.slots.len() + 1) as u32));
    }
    if let Some(s) = b.slots.get_mut((slot_index - 1) as usize) {
        *s = slot;
    }
}
