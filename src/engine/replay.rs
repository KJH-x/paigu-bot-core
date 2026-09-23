use std::sync::Arc;

use crate::domain::claim::{Claim, EffectiveClaimLine, Eligibility};
use crate::domain::discount::DiscountRule;
use crate::domain::event::{compare_event_order, DomainEvent, EventEnvelope};
use crate::domain::ids::RoundId;
use crate::domain::item::Item;
use crate::domain::round::Round;
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::allocation_engine::AllocationEngine;
use crate::engine::event_store::{EventStore, InMemoryEventStore};
use crate::engine::settlement_engine::SettlementEngine;

#[allow(dead_code)] // 保留：测试夹具与后续重放接线；本轮保留公开字段/方法，不删除。
pub struct ReplayService {
    pub event_store: Arc<dyn EventStore>,
    pub allocation_engine: AllocationEngine,
    pub settlement_engine: SettlementEngine,
}

impl ReplayService {
    pub fn new(event_store: Arc<dyn EventStore>) -> Self {
        Self {
            event_store,
            allocation_engine: AllocationEngine::new(),
            settlement_engine: SettlementEngine::new(),
        }
    }

    #[allow(dead_code)] // 保留的公开 API（replay/** 接线用），本轮不删除。
    pub async fn rebuild_snapshot(
        &self,
        round_id: &RoundId,
        items: &[Item],
        eligibilities: &[Eligibility],
        _round: &Round,
    ) -> anyhow::Result<(AllocationSnapshot, Option<SettlementSnapshot>)> {
        let events = self.event_store.read_all(round_id).await?;
        let mut sorted_events = events.clone();
        sorted_events.sort_by(compare_event_order);

        let effective_claims = self.collect_effective_claims(&sorted_events, eligibilities);
        let mut allocation =
            self.allocation_engine
                .allocate(items, &effective_claims, &sorted_events)?;
        allocation.version = sorted_events.len() as i64;

        let settlement = if let Some(s) = self.collect_discount_rules(&sorted_events) {
            let input = crate::engine::settlement_engine::SettlementInput {
                allocation: allocation.clone(),
                items: items.to_vec(),
                discount_rules: s,
            };
            Some(self.settlement_engine.settle(&input))
        } else {
            None
        };

        Ok((allocation, settlement))
    }

    pub fn collect_effective_claims(
        &self,
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
                    self.apply_cancellation(&mut claims, &ev.user_id, cancel);
                }
                DomainEvent::ClaimModified(modify) => {
                    self.apply_modification(&mut claims, modify);
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
        &self,
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
        &self,
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

    #[allow(dead_code)] // 仅由保留的 `rebuild_snapshot` 调用。
    fn collect_discount_rules(&self, events: &[EventEnvelope]) -> Option<Vec<DiscountRule>> {
        for ev in events.iter().rev() {
            if let DomainEvent::DiscountRulesSet(ref rules_set) = ev.payload {
                return Some(rules_set.rules.clone());
            }
        }
        None
    }
}

/// 用内存事件列表重建分配快照（Pipeline 等同步重放的唯一入口）。
/// 先按 `(effective_at, sequence)` 排序，再收集生效 claim 并按优先级排序，最后分配。
pub fn rebuild_allocation_snapshot(
    items: &[Item],
    events: &[EventEnvelope],
    eligibilities: &[Eligibility],
) -> AllocationSnapshot {
    let mut sorted = events.to_vec();
    sorted.sort_by(compare_event_order);
    let store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let service = ReplayService::new(store);
    let lines = service.collect_effective_claims(&sorted, eligibilities);
    match service.allocation_engine.allocate(items, &lines, &sorted) {
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
