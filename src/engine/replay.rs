use std::sync::Arc;

use crate::domain::ids::RoundId;
use crate::domain::round::Round;
use crate::domain::item::Item;
use crate::domain::claim::{Claim, EffectiveClaimLine, Eligibility};
use crate::domain::event::{EventEnvelope, DomainEvent, compare_event_order};
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::discount::DiscountRule;
use crate::engine::event_store::EventStore;
use crate::engine::allocation_engine::AllocationEngine;
use crate::engine::settlement_engine::SettlementEngine;
use crate::error::AppResult;

pub struct ReplayService {
    pub event_store: Arc<dyn EventStore>,
    pub allocation_engine: AllocationEngine,
    pub settlement_engine: SettlementEngine,
}

impl ReplayService {
    pub fn new(
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        Self {
            event_store,
            allocation_engine: AllocationEngine::new(),
            settlement_engine: SettlementEngine::new(),
        }
    }

    pub async fn rebuild_snapshot(
        &self,
        round_id: &RoundId,
        items: &[Item],
        eligibilities: &[Eligibility],
        _round: &Round,
    ) -> AppResult<(AllocationSnapshot, Option<SettlementSnapshot>)> {
        let events = self.event_store.read_all(round_id).await?;
        let mut sorted_events = events.clone();
        sorted_events.sort_by(compare_event_order);

        let effective_claims = self.collect_effective_claims(&sorted_events, eligibilities);
        let allocation = self.allocation_engine.allocate(items, &effective_claims, &sorted_events)?;

        let settlement = if let Some(s) = self.collect_discount_rules(&sorted_events) {
            let input = crate::engine::settlement_engine::SettlementInput {
                allocation: allocation.clone(),
                items: items.to_vec(),
                discount_rules: s,
                gift_valuations: vec![],
            };
            Some(self.settlement_engine.settle(&input)?)
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
                    &claim.user_id, &line.item_id, claim.effective_at, eligibilities
                );

                lines.push(EffectiveClaimLine {
                    claim_id: claim.claim_id.clone(),
                    line_index: i as u32,
                    user_id: claim.user_id.clone(),
                    item_id: line.item_id.clone(),
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
            b.priority_level.cmp(&a.priority_level)
                .then_with(|| a.effective_at.cmp(&b.effective_at))
                .then_with(|| a.sequence.cmp(&b.sequence))
                .then_with(|| a.line_index.cmp(&b.line_index))
        });

        lines
    }

    fn apply_cancellation(&self, claims: &mut Vec<Claim>, user_id: &crate::domain::ids::UserId,
        cancel: &crate::domain::event::ClaimCancelled) {
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

    fn apply_modification(&self, claims: &mut Vec<Claim>,
        modify: &crate::domain::event::ClaimModified) {
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

    fn collect_discount_rules(&self, events: &[EventEnvelope]) -> Option<Vec<DiscountRule>> {
        for ev in events.iter().rev() {
            if let DomainEvent::DiscountRulesSet(ref rules_set) = ev.payload {
                return Some(rules_set.rules.clone());
            }
        }
        None
    }
}
