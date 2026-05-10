use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::{RoundId, EventId, UserId};
use crate::domain::round::RoundConfig;
use crate::domain::event::{EventEnvelope, DomainEvent};
use crate::domain::snapshot::{AllocationSnapshot, DecisionTrace};
use crate::domain::settlement::SettlementSnapshot;
use crate::engine::allocation_engine::AllocationEngine;
use crate::engine::settlement_engine::SettlementEngine;
use crate::replay::state_diff::StateDiff;
use crate::error::ReplayError;

pub struct ReplayEngine {
    pub allocation_engine: AllocationEngine,
    pub settlement_engine: SettlementEngine,
}

impl ReplayEngine {
    pub fn new() -> Self {
        Self {
            allocation_engine: AllocationEngine::new(),
            settlement_engine: SettlementEngine::new(),
        }
    }

    pub async fn replay(
        &self,
        round_config: RoundConfig,
        events: Vec<EventEnvelope>,
        options: ReplayOptions,
    ) -> Result<ReplayResult, ReplayError> {
        let mut sorted_events = events;
        sorted_events.sort_by(|a, b| crate::domain::event::compare_event_order(a, b));

        let mut state = ReplayRuntimeState::new(round_config);
        let mut steps = Vec::new();
        let mut previous_snapshot = state.to_allocation_snapshot();

        for (index, event) in sorted_events.into_iter().enumerate() {
            let before_version = state.version;
            let before_snapshot = state.to_allocation_snapshot();

            let decision_trace = self.apply_event_with_trace(&mut state, &event).await?;

            let after_snapshot = state.to_allocation_snapshot();
            let state_diff = StateDiff::from_snapshots(&before_snapshot, &after_snapshot);

            let settlement_snapshot = if options.include_settlement {
                Some(self.settlement_engine.settle(
                    &crate::engine::settlement_engine::SettlementInput {
                        allocation: after_snapshot.clone(),
                        items: state.items.clone(),
                        discount_rules: state.discount_rules.clone(),
                        gift_valuations: vec![],
                    }
                ).map_err(|e| ReplayError::SnapshotRestoreFailed(0, e.to_string()))?)
            } else {
                None
            };

            let step = ReplayStep {
                replay_id: options.replay_id.clone(),
                round_id: state.round_id.clone(),
                step_index: index as u64,
                event_id: event.event_id.clone(),
                raw_message_id: event.raw_message_id.clone(),
                occurred_at: event.effective_at,
                input_event: event,
                before_version,
                after_version: state.version,
                state_diff,
                allocation_snapshot: after_snapshot.clone(),
                settlement_snapshot,
                decision_trace,
                warnings: vec![],
                errors: vec![],
            };

            steps.push(step);
            previous_snapshot = after_snapshot;
        }

        Ok(ReplayResult {
            replay_id: options.replay_id,
            final_snapshot: previous_snapshot,
            final_settlement: None,
            steps,
            input_message_count: 0,
        })
    }

    async fn apply_event_with_trace(
        &self,
        state: &mut ReplayRuntimeState,
        event: &EventEnvelope,
    ) -> Result<DecisionTrace, ReplayError> {
        match &event.payload {
            DomainEvent::ClaimCreated(claim) => {
                state.add_claim(claim, event);
                let priority = crate::domain::claim::EffectiveClaimLine::compute_priority(
                    &claim.user_id,
                    &claim.items.first().map(|l| l.item_id.clone()).unwrap_or(crate::domain::ids::ItemId("unknown".to_string())),
                    event.effective_at,
                    &state.eligibility,
                );

                let priority_trace = crate::domain::snapshot::PriorityTrace {
                    user_id: claim.user_id.0.clone(),
                    priority_level: priority,
                    matched_eligibility_ids: vec![],
                    explanation: format!("用户 {} 优先级 {}", claim.user_id.0, priority),
                };

                let allocation_trace: Vec<crate::domain::snapshot::AllocationTraceItem> = claim.items.iter().map(|line| {
                    crate::domain::snapshot::AllocationTraceItem {
                        claim_id: claim.claim_id.0.clone(),
                        item_id: line.item_id.0.clone(),
                        quantity_requested: line.quantity,
                        quantity_allocated: line.quantity,
                        policy: line.slot_policy.as_str().to_string(),
                        final_slots: vec![],
                        explanation: format!("排入 {} x{}", line.item_id.0, line.quantity),
                    }
                }).collect();

                Ok(DecisionTrace {
                    parse_trace: claim.parse_trace.clone(),
                    validation_trace: claim.validation_trace.clone(),
                    priority_trace: Some(priority_trace),
                    allocation_trace,
                    settlement_trace: vec![],
                })
            }
            DomainEvent::ClaimCancelled(cancel) => {
                state.apply_cancel(&event.user_id, cancel);
                Ok(DecisionTrace {
                    parse_trace: cancel.parse_trace.clone(),
                    validation_trace: cancel.validation_trace.clone(),
                    priority_trace: None,
                    allocation_trace: vec![],
                    settlement_trace: vec![],
                })
            }
            DomainEvent::DiscountRulesSet(rules) => {
                state.discount_rules = rules.rules.clone();
                state.version += 1;
                Ok(DecisionTrace {
                    parse_trace: None,
                    validation_trace: vec![],
                    priority_trace: None,
                    allocation_trace: vec![],
                    settlement_trace: vec![crate::domain::snapshot::SettlementTraceItem {
                        rule_id: "discount_rules".to_string(),
                        description: format!("设置 {} 条优惠规则", rules.rules.len()),
                    }],
                })
            }
            _ => {
                state.version += 1;
                Ok(DecisionTrace::empty_with_note("event processed"))
            }
        }
    }
}

pub struct ReplayRuntimeState {
    pub round_id: RoundId,
    pub items: Vec<crate::domain::item::Item>,
    pub eligibility: Vec<crate::domain::claim::Eligibility>,
    pub claims: Vec<crate::domain::claim::Claim>,
    pub discount_rules: Vec<crate::domain::discount::DiscountRule>,
    pub version: i64,
    pub events: Vec<EventEnvelope>,
}

impl ReplayRuntimeState {
    fn new(config: RoundConfig) -> Self {
        Self {
            round_id: config.round.round_id,
            items: config.items,
            eligibility: config.eligibility,
            claims: vec![],
            discount_rules: vec![],
            version: 1,
            events: vec![],
        }
    }

    fn to_allocation_snapshot(&self) -> AllocationSnapshot {
        let engine = AllocationEngine::new();
        let effective_lines: Vec<crate::domain::claim::EffectiveClaimLine> = self.claims.iter()
            .filter(|c| c.status == crate::domain::claim::ClaimStatus::Active)
            .flat_map(|c| {
                c.items.iter().enumerate().filter_map(|(i, l)| {
                    if l.quantity == 0 { return None; }
                    let priority = crate::domain::claim::EffectiveClaimLine::compute_priority(
                        &c.user_id, &l.item_id, c.effective_at, &self.eligibility
                    );
                    Some(crate::domain::claim::EffectiveClaimLine {
                        claim_id: c.claim_id.clone(),
                        line_index: i as u32,
                        user_id: c.user_id.clone(),
                        item_id: l.item_id.clone(),
                        quantity: l.quantity,
                        claim_type: l.claim_type.clone(),
                        slot_policy: l.slot_policy.clone(),
                        effective_at: c.effective_at,
                        sequence: c.sequence,
                        priority_level: priority,
                    })
                })
            })
            .collect();

        engine.allocate(&self.items, &effective_lines, &self.events)
            .unwrap_or(AllocationSnapshot {
                round_id: self.round_id.clone(),
                version: self.version,
                generated_at: chrono::Utc::now(),
                item_allocations: vec![],
                user_summaries: vec![],
                warnings: vec![],
            })
    }

    fn add_claim(&mut self, claim: &crate::domain::event::ClaimCreated, event: &EventEnvelope) {
        self.claims.push(crate::domain::claim::Claim {
            claim_id: claim.claim_id.clone(),
            round_id: event.round_id.clone(),
            user_id: claim.user_id.clone(),
            items: claim.items.clone(),
            source_text: claim.source_text.clone(),
            effective_at: event.effective_at,
            sequence: event.sequence,
            status: crate::domain::claim::ClaimStatus::Active,
        });
        self.events.push(event.clone());
        self.version += 1;
    }

    fn apply_cancel(&mut self, user_id: &UserId, cancel: &crate::domain::event::ClaimCancelled) {
        if let Some(ref target_id) = cancel.target_claim_id {
            for c in self.claims.iter_mut() {
                if &c.claim_id == target_id && &c.user_id == user_id {
                    c.cancel_all();
                }
            }
            return;
        }

        if let Some(ref item_id) = cancel.target_item_id {
            let mut remaining = cancel.quantity.unwrap_or(u32::MAX);
            for c in self.claims.iter_mut().rev() {
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

        for c in self.claims.iter_mut().rev() {
            if &c.user_id == user_id && !c.is_empty() {
                c.cancel_all();
                break;
            }
        }
        self.version += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOptions {
    pub replay_id: String,
    pub include_settlement: bool,
    pub snapshot_interval: u32,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            replay_id: uuid::Uuid::new_v4().to_string(),
            include_settlement: true,
            snapshot_interval: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub replay_id: String,
    pub final_snapshot: AllocationSnapshot,
    pub final_settlement: Option<SettlementSnapshot>,
    pub steps: Vec<ReplayStep>,
    pub input_message_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStep {
    pub replay_id: String,
    pub round_id: RoundId,
    pub step_index: u64,
    pub event_id: EventId,
    pub raw_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub input_event: EventEnvelope,
    pub before_version: i64,
    pub after_version: i64,
    pub state_diff: StateDiff,
    pub allocation_snapshot: AllocationSnapshot,
    pub settlement_snapshot: Option<SettlementSnapshot>,
    pub decision_trace: DecisionTrace,
    pub warnings: Vec<ReplayWarning>,
    pub errors: Vec<ReplayStepError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayWarning {
    pub step_index: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStepError {
    pub step_index: u64,
    pub message: String,
}
