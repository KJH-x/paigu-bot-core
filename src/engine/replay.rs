use std::collections::HashMap;

use crate::domain::claim::{Claim, EffectiveClaimLine, Eligibility};
use crate::domain::event::{compare_event_order, DomainEvent, EventEnvelope};
use crate::domain::ids::{RoundId, UserId};
use crate::domain::item::Item;
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::allocation_engine::AllocationEngine;

/// 从事件流中收集当前生效的 claim 行，并按优先级排序。
pub fn collect_effective_claims(
    events: &[EventEnvelope],
    eligibilities: &[Eligibility],
) -> Vec<EffectiveClaimLine> {
    let mut claims: Vec<Claim> = Vec::new();

    for ev in events {
        match &ev.payload {
            DomainEvent::ClaimCreated(c) => {
                let claim = Claim {
                    claim_id: c.claim_id.clone(),
                    round_id: ev.round_id.clone(),
                    user_id: c.user_id.clone(),
                    items: c.items.clone(),
                    source_text: c.source_text.clone(),
                    effective_at: ev.effective_at,
                    sequence: ev.sequence,
                    status: crate::domain::claim::ClaimStatus::Active,
                };
                claims.push(claim);
            }
            DomainEvent::ClaimCancelled(cancel) => {
                apply_cancellation(&mut claims, &ev.user_id, cancel);
            }
            DomainEvent::ClaimModified(modify) => {
                apply_modification(&mut claims, modify);
            }
            _ => {}
        }
    }

    let mut lines = Vec::new();
    for claim in &claims {
        if claim.status != crate::domain::claim::ClaimStatus::Active {
            continue;
        }
        for (i, line) in claim.items.iter().enumerate() {
            if line.quantity == 0 {
                continue;
            }
            let priority = EffectiveClaimLine::compute_priority(
                &claim.user_id,
                &line.item_id,
                claim.effective_at,
                eligibilities,
            );

            lines.push(EffectiveClaimLine {
                claim_id: claim.claim_id.clone(),
                line_index: i as u32,
                user_id: claim.user_id.clone(),
                item_id: line.item_id.clone(),
                variant_id: line.variant_id.clone(),
                quantity: line.quantity,
                claim_type: line.claim_type.clone(),
                slot_policy: line.slot_policy.clone(),
                effective_at: claim.effective_at,
                sequence: claim.sequence,
                priority_level: priority,
            });
        }
    }

    lines.sort_by(|a, b| {
        b.priority_level
            .cmp(&a.priority_level)
            .then_with(|| a.effective_at.cmp(&b.effective_at))
            .then_with(|| a.sequence.cmp(&b.sequence))
            .then_with(|| a.line_index.cmp(&b.line_index))
    });

    lines
}

fn apply_cancellation(
    claims: &mut [Claim],
    user_id: &crate::domain::ids::UserId,
    cancel: &crate::domain::event::ClaimCancelled,
) {
    if let Some(ref target_id) = cancel.target_claim_id {
        for c in claims.iter_mut() {
            if &c.claim_id == target_id && &c.user_id == user_id {
                c.cancel_all();
            }
        }
        return;
    }

    if let Some(ref item_id) = cancel.target_item_id {
        let mut remaining = cancel.quantity.unwrap_or(u32::MAX);
        for c in claims.iter_mut().rev() {
            if &c.user_id == user_id {
                let cancelled = c.cancel_item_quantity(item_id, remaining);
                remaining -= cancelled;
                if remaining == 0 {
                    break;
                }
            }
        }
        return;
    }

    for c in claims.iter_mut().rev() {
        if &c.user_id == user_id && !c.is_empty() {
            c.cancel_all();
            break;
        }
    }
}

fn apply_modification(
    claims: &mut [Claim],
    modify: &crate::domain::event::ClaimModified,
) {
    for c in claims.iter_mut() {
        if c.claim_id == modify.target_claim_id {
            if let Some(ref item_id) = modify.target_item_id {
                if let Some(qty) = modify.new_quantity {
                    for line in &mut c.items {
                        if &line.item_id == item_id {
                            line.quantity = qty;
                            if let Some(ref policy) = modify.new_slot_policy {
                                line.slot_policy = policy.clone();
                            }
                        }
                    }
                }
            }
            c.status = crate::domain::claim::ClaimStatus::Modified;
        }
    }
}

/// 用内存事件列表重建分配快照（Pipeline 等同步重放的唯一入口）。
/// 先按 `(effective_at, sequence)` 排序，再收集生效 claim 并按优先级排序，最后分配。
pub fn rebuild_allocation_snapshot(
    items: &[Item],
    events: &[EventEnvelope],
    eligibilities: &[Eligibility],
    display_names: &HashMap<UserId, String>,
) -> AllocationSnapshot {
    let mut sorted = events.to_vec();
    sorted.sort_by(compare_event_order);
    let lines = collect_effective_claims(&sorted, eligibilities);
    match AllocationEngine::new().allocate_with_names(items, &lines, &sorted, display_names) {
        Ok(mut snapshot) => {
            snapshot.version = sorted.len() as i64;
            snapshot
        }
        Err(_) => empty_snapshot(items),
    }
}

fn empty_snapshot(items: &[Item]) -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: items
            .first()
            .map(|i| i.round_id.clone())
            .unwrap_or_else(|| RoundId("unknown".to_string())),
        version: 0,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![],
        user_summaries: vec![],
        warnings: vec![],
    }
}

/// 事件的人类可读摘要（消息流展示用）。
pub fn describe_event(event: &EventEnvelope) -> String {
    match &event.payload {
        DomainEvent::ClaimCreated(c) => {
            let items: Vec<String> = c
                .items
                .iter()
                .map(|l| format!("{}x{}[{}]", l.item_id.0, l.quantity, l.slot_policy.as_str()))
                .collect();
            format!("claim: {}", items.join(", "))
        }
        DomainEvent::ClaimCancelled(c) => format!(
            "cancel: item={:?} qty={:?}",
            c.target_item_id.as_ref().map(|i| i.0.clone()),
            c.quantity
        ),
        other => format!("event: {}", other.event_type_str()),
    }
}
