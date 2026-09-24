use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::event::{DomainEvent, EventEnvelope};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParsedIntent {
    Claim,
    Cancel,
    Modify,
    ConfirmAmbiguous,
    AdminCommand,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMessage {
    pub intent: ParsedIntent,
    pub round_hint: Option<String>,
    pub items: Vec<ParsedClaimItem>,
    pub cancel_target_hint: Option<String>,
    pub admin_command: Option<ParsedAdminCommand>,
    pub confidence: f32,
    pub ambiguous_parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedClaimItem {
    pub name: String,
    pub category_hint: Option<String>,
    pub quantity: u32,
    pub claim_type: Option<String>,
    pub is_proxy_card: Option<bool>,
    pub slot_policy: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub resolved_item_id: Option<String>,
    #[serde(default)]
    pub resolved_variant_id: Option<String>,
    #[serde(default)]
    pub resolved_round_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ParsedAdminCommand {
    CreateRound {
        title: String,
        start_at: Option<String>,
        end_at: Option<String>,
    },
    AddItem {
        name: String,
        kind: String,
        unit_price_cents: i64,
        box_size: Option<u32>,
        max_quantity: Option<u32>,
        aliases: Option<Vec<String>>,
    },
    SetDiscountRules {
        rules: Vec<serde_json::Value>,
    },
    CloseRound {},
    ExportRound {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResult {
    pub round_id: Option<crate::domain::ids::RoundId>,
    pub item_id: Option<crate::domain::ids::ItemId>,
    pub variant_id: Option<String>,
    pub candidates: Vec<(crate::domain::ids::RoundId, crate::domain::ids::ItemId, i32)>,
    pub resolved: bool,
    pub ambiguity: Option<String>,
}

impl ResolveResult {
    pub fn not_found() -> Self {
        Self {
            round_id: None,
            item_id: None,
            variant_id: None,
            candidates: vec![],
            resolved: false,
            ambiguity: None,
        }
    }

    pub fn resolved(
        round_id: crate::domain::ids::RoundId,
        item_id: crate::domain::ids::ItemId,
    ) -> Self {
        Self {
            round_id: Some(round_id),
            item_id: Some(item_id),
            variant_id: None,
            candidates: vec![],
            resolved: true,
            ambiguity: None,
        }
    }

    pub fn resolved_variant(
        round_id: crate::domain::ids::RoundId,
        item_id: crate::domain::ids::ItemId,
        variant_id: String,
    ) -> Self {
        Self {
            round_id: Some(round_id),
            item_id: Some(item_id),
            variant_id: Some(variant_id),
            candidates: vec![],
            resolved: true,
            ambiguity: None,
        }
    }

    pub fn ambiguous(
        candidates: Vec<(crate::domain::ids::RoundId, crate::domain::ids::ItemId, i32)>,
        msg: String,
    ) -> Self {
        Self {
            round_id: None,
            item_id: None,
            variant_id: None,
            candidates,
            resolved: false,
            ambiguity: Some(msg),
        }
    }
}

/// §U7：从事件流收集 `ParseOverride` → `target_raw_message_id → 修正后的解析`。
/// 重放/校验时按 `message_id` 直接采用覆盖结果（存在性优先于重新解析）。
pub fn collect_parse_overrides(events: &[EventEnvelope]) -> HashMap<String, ParsedMessage> {
    let mut map = HashMap::new();
    for ev in events {
        if let DomainEvent::ParseOverride(over) = &ev.payload {
            if let Ok(corrected) =
                serde_json::from_value::<ParsedMessage>(over.corrected_parsed_message.clone())
            {
                map.insert(over.target_raw_message_id.clone(), corrected);
            }
        }
    }
    map
}

/// 应用覆盖：命中 `message_id` 时直接返回覆盖结果，否则原样返回。
pub fn apply_parse_override(
    parsed: ParsedMessage,
    raw_message_id: Option<&str>,
    overrides: &HashMap<String, ParsedMessage>,
) -> ParsedMessage {
    match raw_message_id.and_then(|id| overrides.get(id)) {
        Some(corrected) => corrected.clone(),
        None => parsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{EventStatus, ParseOverrideEvent};
    use crate::domain::ids::{EventId, RoundId, UserId};

    fn parsed_with(name: &str, item_id: &str) -> ParsedMessage {
        ParsedMessage {
            intent: ParsedIntent::Claim,
            round_hint: None,
            items: vec![ParsedClaimItem {
                name: name.to_string(),
                category_hint: None,
                quantity: 1,
                claim_type: Some("Split".to_string()),
                is_proxy_card: None,
                slot_policy: None,
                notes: None,
                resolved_item_id: Some(item_id.to_string()),
                resolved_variant_id: Some("v_jcl".to_string()),
                resolved_round_id: Some("r1".to_string()),
            }],
            cancel_target_hint: None,
            admin_command: None,
            confidence: 0.9,
            ambiguous_parts: vec![],
        }
    }

    fn override_event(message_id: &str, corrected: &ParsedMessage) -> EventEnvelope {
        let now = chrono::Utc::now();
        EventEnvelope {
            event_id: EventId("e1".to_string()),
            round_id: RoundId("r1".to_string()),
            group_id: "g1".to_string(),
            user_id: UserId("u1".to_string()),
            raw_message_id: Some(message_id.to_string()),
            event_type: "parse_override".to_string(),
            effective_at: now,
            sequence: 1,
            payload: DomainEvent::ParseOverride(ParseOverrideEvent {
                event_id: EventId("ov1".to_string()),
                round_id: RoundId("r1".to_string()),
                target_raw_message_id: message_id.to_string(),
                corrected_parsed_message: serde_json::to_value(corrected).unwrap(),
                admin_user_id: UserId("u1".to_string()),
                reason: "first-match".to_string(),
                occurred_at: now,
            }),
            status: EventStatus::Active,
        }
    }

    #[test]
    fn collect_and_apply_parse_override_wins() {
        let corrected = parsed_with("结城理", "pass_sp");
        let events = vec![override_event("m1", &corrected)];
        let overrides = collect_parse_overrides(&events);
        assert!(overrides.contains_key("m1"));

        let fallback = parsed_with("悠人", "hr_resume");
        let applied = apply_parse_override(fallback, Some("m1"), &overrides);
        assert_eq!(applied.items[0].resolved_item_id.as_deref(), Some("pass_sp"));

        let untouched = apply_parse_override(parsed_with("悠人", "hr_resume"), Some("m2"), &overrides);
        assert_eq!(untouched.items[0].resolved_item_id.as_deref(), Some("hr_resume"));
    }
}
