use super::RoundFixture;
use crate::domain::event::EventEnvelope;
use crate::domain::ids::{EventId, RoundId};
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::snapshot::{AllocationSnapshot, DecisionTrace};
use crate::engine::{
    replay::rebuild_allocation_snapshot,
    settlement_engine::{SettlementEngine, SettlementInput},
};
use crate::messages::MessageRecord;
use crate::replay::session;
use crate::replay::state_diff::StateDiff;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(super) fn build(
    fixture: &RoundFixture,
    records: &[MessageRecord],
    result: &session::ReplayResult,
) -> ReplayResult {
    let replay_id = format!("replay_{}", result.round_id);
    let mut before = rebuild_allocation_snapshot(
        &fixture.items,
        &[],
        &result.eligibilities,
        &result.display_names,
    );
    let mut steps = Vec::with_capacity(result.events.len());
    for (index, event) in result.events.iter().enumerate() {
        let prefix_records: Vec<_> = records
            .iter()
            .filter(|r| r.seq <= event.sequence)
            .cloned()
            .collect();
        let names = session::resolve_display_names(&fixture.config, &prefix_records);
        let mut after = rebuild_allocation_snapshot(
            &fixture.items,
            &result.events[..=index],
            &result.eligibilities,
            &names,
        );
        after.generated_at = event.effective_at;
        let mut settlement = SettlementEngine::new().settle(&SettlementInput {
            allocation: after.clone(),
            items: fixture.items.clone(),
            discount_rules: vec![],
        });
        settlement.generated_at = after.generated_at;
        for bill in &mut settlement.user_bills {
            bill.display_name = names
                .get(&bill.user_id)
                .cloned()
                .unwrap_or_else(|| bill.user_id.0.clone());
        }
        let state_diff = StateDiff::from_snapshots(&before, &after);
        let decision_trace = trace(event, &after, &result.eligibilities);
        steps.push(ReplayStep {
            replay_id: replay_id.clone(),
            round_id: event.round_id.clone(),
            step_index: index as u64,
            event_id: event.event_id.clone(),
            raw_message_id: event.raw_message_id.clone(),
            occurred_at: event.effective_at,
            input_event: event.clone(),
            before_version: before.version,
            after_version: after.version,
            state_diff,
            allocation_snapshot: after.clone(),
            settlement_snapshot: Some(settlement),
            decision_trace,
            warnings: vec![],
            errors: vec![],
        });
        before = after;
    }
    let final_settlement = if steps.is_empty() {
        None
    } else {
        let mut settlement = SettlementEngine::new().settle(&SettlementInput {
            allocation: result.board.clone(),
            items: fixture.items.clone(),
            discount_rules: vec![],
        });
        settlement.generated_at = result.board.generated_at;
        for bill in &mut settlement.user_bills {
            bill.display_name = result
                .display_names
                .get(&bill.user_id)
                .cloned()
                .unwrap_or_else(|| bill.user_id.0.clone());
        }
        Some(settlement)
    };
    ReplayResult {
        replay_id,
        final_snapshot: result.board.clone(),
        final_settlement,
        steps,
        input_message_count: records.len() as u64,
    }
}

fn trace(
    event: &EventEnvelope,
    board: &AllocationSnapshot,
    eligibility: &[crate::domain::claim::Eligibility],
) -> DecisionTrace {
    use crate::domain::event::DomainEvent;
    use crate::domain::snapshot::{AllocationTraceItem, PriorityTrace};
    let mut trace = DecisionTrace {
        parse_trace: None,
        validation_trace: vec![],
        priority_trace: None,
        allocation_trace: vec![],
        settlement_trace: vec![],
    };
    match &event.payload {
        DomainEvent::ClaimCreated(claim) => {
            trace.parse_trace = claim.parse_trace.clone();
            trace.validation_trace = claim.validation_trace.clone();
            if let Some(first) = claim.items.first() {
                let level = crate::domain::claim::EffectiveClaimLine::compute_priority(
                    &claim.user_id,
                    &first.item_id,
                    event.effective_at,
                    eligibility,
                );
                trace.priority_trace = Some(PriorityTrace {
                    user_id: claim.user_id.0.clone(),
                    priority_level: level,
                    matched_eligibility_ids: eligibility
                        .iter()
                        .filter(|e| {
                            e.user_id == claim.user_id
                                && e.applies_at(event.effective_at)
                                && e.applies_to_item(&first.item_id)
                                && e.has_uses_remaining()
                        })
                        .map(|e| e.eligibility_id.0.clone())
                        .collect(),
                    explanation: format!("用户 {} 优先级 {}", claim.user_id.0, level),
                });
            }
            let mut single_used = std::collections::HashMap::<String, u32>::new();
            for (line_index, line) in claim.items.iter().enumerate() {
                let mut slots = vec![];
                let mut single_total: u32 = 0;
                for allocation in board
                    .item_allocations
                    .iter()
                    .filter(|a| a.item_id == line.item_id && a.subitem_id == line.subitem_id)
                {
                    for column in &allocation.boxes {
                        for slot in &column.slots {
                            if slot.claim_id.as_ref() == Some(&claim.claim_id)
                                && slot.claim_line_index == Some(line_index as u32)
                                && slot.user_id.is_some()
                            {
                                slots.push(format!(
                                    "box{}:slot{}",
                                    column.box_index, slot.slot_index
                                ));
                            }
                        }
                    }
                    single_total = single_total.saturating_add(
                        allocation
                            .singles
                            .iter()
                            .filter(|s| s.claim_id == claim.claim_id)
                            .map(|s| s.quantity)
                            .sum(),
                    );
                }
                let used = single_used.entry(line.item_id.0.clone()).or_default();
                let singles = single_total.saturating_sub(*used).min(line.quantity);
                *used += singles;
                let allocated = slots.len() as u32 + singles;
                if singles > 0 {
                    slots.push(format!("single x{singles}"));
                }
                trace.allocation_trace.push(AllocationTraceItem {
                    claim_id: claim.claim_id.0.clone(),
                    item_id: line.item_id.0.clone(),
                    quantity_requested: line.quantity,
                    quantity_allocated: allocated,
                    policy: line.slot_policy.as_str().into(),
                    final_slots: slots,
                    explanation: format!("实际分配 {allocated}/{}", line.quantity),
                });
            }
        }
        DomainEvent::ClaimCancelled(cancel) => {
            trace.parse_trace = cancel.parse_trace.clone();
            trace.validation_trace = cancel.validation_trace.clone();
        }
        _ => {}
    }
    trace
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
