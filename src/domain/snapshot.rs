use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::RoundId;
use crate::domain::allocation::{ItemAllocation, UserAllocationSummary, AllocationWarning};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationSnapshot {
    pub round_id: RoundId,
    pub version: i64,
    pub generated_at: DateTime<Utc>,
    pub item_allocations: Vec<ItemAllocation>,
    pub user_summaries: Vec<UserAllocationSummary>,
    pub warnings: Vec<AllocationWarning>,
}

impl AllocationSnapshot {
    pub fn item(&self, name: &str) -> Option<&ItemAllocation> {
        self.item_allocations.iter().find(|i| i.item_name == name || i.item_id.0 == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSnapshot {
    pub round_id: String,
    pub title: String,
    pub status: String,
    pub version: i64,
    pub updated_at: String,
    pub items: Vec<PublicItemView>,
    pub user_bills: Vec<PublicUserBill>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicItemView {
    pub item_id: String,
    pub name: String,
    pub kind: String,
    pub unit_price_cents: i64,
    pub boxes: Vec<PublicBoxView>,
    pub singles: Vec<PublicSingleView>,
    pub waiting: Vec<PublicWaitingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicBoxView {
    pub box_index: u32,
    pub slots: Vec<PublicSlotView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSlotView {
    pub slot_index: u32,
    pub status: String,
    pub display_name: Option<String>,
    pub policy: String,
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSlotView {
    pub user_id: String,
    pub nickname: String,
    pub raw_message_id: Option<String>,
    pub claim_id: Option<String>,
    pub slot_policy: String,
    pub status: String,
    pub slot_index: u32,
    pub box_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSingleView {
    pub display_name: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWaitingView {
    pub display_name: String,
    pub quantity: u32,
    pub priority_level: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUserBill {
    pub user_id: String,
    pub display_name: String,
    pub gross_total_cents: i64,
    pub discount_share_cents: i64,
    pub gift_value_share_cents: i64,
    pub shipping_fee_cents: i64,
    pub final_total_cents: i64,
}

impl AllocationSnapshot {
    pub fn to_public(&self, title: &str, status: &str) -> PublicSnapshot {
        PublicSnapshot {
            round_id: self.round_id.0.clone(),
            title: title.to_string(),
            status: status.to_string(),
            version: self.version,
            updated_at: self.generated_at.to_rfc3339(),
            items: self.item_allocations.iter().map(|ia| PublicItemView {
                item_id: ia.item_id.0.clone(),
                name: ia.item_name.clone(),
                kind: ia.kind.clone(),
                unit_price_cents: 0,
                boxes: ia.boxes.iter().map(|b| PublicBoxView {
                    box_index: b.box_index,
                    slots: b.slots.iter().map(|s| PublicSlotView {
                        slot_index: s.slot_index,
                        status: s.status.as_str().to_string(),
                        display_name: s.user_id.as_ref().map(|u| u.0.clone()),
                        policy: s.slot_policy.as_str().to_string(),
                        segment_id: s.segment_id.clone(),
                    }).collect(),
                }).collect(),
                singles: ia.singles.iter().map(|s| PublicSingleView {
                    display_name: s.user_id.0.clone(),
                    quantity: s.quantity,
                }).collect(),
                waiting: ia.waiting.iter().map(|w| PublicWaitingView {
                    display_name: w.user_id.0.clone(),
                    quantity: w.quantity,
                    priority_level: w.priority_level,
                }).collect(),
            }).collect(),
            user_bills: vec![],
            warnings: self.warnings.iter().map(|w| w.message.clone()).collect(),
        }
    }

    pub fn short_ack_for_user(&self, user_id: &str) -> String {
        for summary in &self.user_summaries {
            if summary.user_id.0 == user_id {
                let items: Vec<String> = summary.items.iter()
                    .map(|i| format!("{} x{}", i.item_name, i.quantity))
                    .collect();
                return format!("{}，当前版本 #{}", items.join("，"), self.version);
            }
        }
        format!("当前版本 #{}", self.version)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseTrace {
    pub raw_text: String,
    pub parser_version: String,
    pub prompt_hash: String,
    pub model_name: String,
    pub parsed_json: serde_json::Value,
    pub confidence: f32,
    pub ambiguous_parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationTraceItem {
    pub rule: String,
    pub passed: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityTrace {
    pub user_id: String,
    pub priority_level: i32,
    pub matched_eligibility_ids: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationTraceItem {
    pub claim_id: String,
    pub item_id: String,
    pub quantity_requested: u32,
    pub quantity_allocated: u32,
    pub policy: String,
    pub final_slots: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrace {
    pub parse_trace: Option<ParseTrace>,
    pub validation_trace: Vec<ValidationTraceItem>,
    pub priority_trace: Option<PriorityTrace>,
    pub allocation_trace: Vec<AllocationTraceItem>,
    pub settlement_trace: Vec<SettlementTraceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementTraceItem {
    pub rule_id: String,
    pub description: String,
}

impl DecisionTrace {
    pub fn empty_with_note(note: &str) -> Self {
        Self {
            parse_trace: None,
            validation_trace: vec![],
            priority_trace: None,
            allocation_trace: vec![],
            settlement_trace: vec![SettlementTraceItem {
                rule_id: "note".to_string(),
                description: note.to_string(),
            }],
        }
    }
}
