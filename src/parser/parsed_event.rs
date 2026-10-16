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
    pub resolved_subitem_id: Option<String>,
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
    pub subitem_id: Option<String>,
    pub candidates: Vec<(crate::domain::ids::RoundId, crate::domain::ids::ItemId, i32)>,
    pub resolved: bool,
    pub ambiguity: Option<String>,
}

impl ResolveResult {
    pub fn not_found() -> Self {
        Self {
            round_id: None,
            item_id: None,
            subitem_id: None,
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
            subitem_id: None,
            candidates: vec![],
            resolved: true,
            ambiguity: None,
        }
    }

    pub fn resolved_subitem(
        round_id: crate::domain::ids::RoundId,
        item_id: crate::domain::ids::ItemId,
        subitem_id: String,
    ) -> Self {
        Self {
            round_id: Some(round_id),
            item_id: Some(item_id),
            subitem_id: Some(subitem_id),
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
            subitem_id: None,
            candidates,
            resolved: false,
            ambiguity: Some(msg),
        }
    }
}

#[derive(Clone, Default)]
pub struct ParseOverrides {
    messages: HashMap<String, ParsedMessage>,
    records: HashMap<String, (ParsedMessage, u64, String)>,
    selections: HashMap<String, (super::action_override::SavedSelection, u64)>,
    invalid_selections: HashMap<String, u64>,
}

impl ParseOverrides {
    pub fn record_revision(&self, record_id: &str, stored_revision: u64) -> u64 {
        let revision = self
            .records
            .get(record_id)
            .map_or(stored_revision, |(_, revision, _)| {
                stored_revision.max(*revision)
            });
        let revision = self
            .selections
            .get(record_id)
            .map_or(revision, |(_, v)| revision.max(*v));
        self.invalid_selections
            .get(record_id)
            .map_or(revision, |v| revision.max(*v))
    }

    pub fn has_record(&self, record_id: &str) -> bool {
        self.records.contains_key(record_id)
            || self.selections.contains_key(record_id)
            || self.invalid_selections.contains_key(record_id)
    }

    pub fn for_record(&self, record_id: &str) -> Option<&ParsedMessage> {
        self.records.get(record_id).map(|(parsed, _, _)| parsed)
    }

    pub fn reason_for_record(&self, record_id: &str) -> Option<&str> {
        if let Some((saved, _)) = self.selections.get(record_id) {
            return Some(saved.selection.note());
        }
        self.records
            .get(record_id)
            .map(|(_, _, reason)| reason.as_str())
    }

    pub fn selection_for_record(
        &self,
        record_id: &str,
    ) -> Option<&super::action_override::SavedSelection> {
        self.selections.get(record_id).map(|(saved, _)| saved)
    }

    pub fn invalid_selection(&self, record_id: &str) -> bool {
        self.invalid_selections.contains_key(record_id)
    }

    pub fn insert_selection(
        &mut self,
        record_id: String,
        saved: super::action_override::SavedSelection,
        revision: u64,
    ) {
        self.records.remove(&record_id);
        self.invalid_selections.remove(&record_id);
        self.selections.insert(record_id, (saved, revision));
    }
}

/// Record-bound manual choices cannot leak onto duplicate upstream message ids.
/// Legacy/automatic message-bound overrides remain readable without pretending to know their source.
pub fn collect_parse_overrides(events: &[EventEnvelope]) -> ParseOverrides {
    let mut map = ParseOverrides::default();
    for ev in events {
        if let DomainEvent::ParseOverride(over) = &ev.payload {
            if let Some(record_id) = &over.target_record_id {
                if let Ok(saved) = serde_json::from_value::<super::action_override::SavedSelection>(
                    over.corrected_parsed_message.clone(),
                ) {
                    if saved.action_selection_version == 1 {
                        map.insert_selection(
                            record_id.clone(),
                            saved,
                            over.target_record_revision.unwrap_or(1),
                        );
                        continue;
                    }
                }
                if over
                    .corrected_parsed_message
                    .get("action_selection_version")
                    .is_some()
                {
                    map.records.remove(record_id);
                    map.selections.remove(record_id);
                    map.invalid_selections
                        .insert(record_id.clone(), over.target_record_revision.unwrap_or(1));
                    continue;
                }
            }
            if let Ok(corrected) =
                serde_json::from_value::<ParsedMessage>(over.corrected_parsed_message.clone())
            {
                if let Some(record_id) = &over.target_record_id {
                    map.selections.remove(record_id);
                    map.invalid_selections.remove(record_id);
                    map.records.insert(
                        record_id.clone(),
                        (
                            corrected,
                            over.target_record_revision.unwrap_or(1),
                            over.reason.clone(),
                        ),
                    );
                } else {
                    map.messages
                        .insert(over.target_raw_message_id.clone(), corrected);
                }
            }
        }
    }
    map
}

/// 稳定记录覆盖优先；未命中时才读取旧 message_id 覆盖，空结果也不回退。
pub fn apply_parse_override(
    parsed: ParsedMessage,
    record_id: &str,
    raw_message_id: Option<&str>,
    overrides: &ParseOverrides,
) -> ParsedMessage {
    if let Some((corrected, _, _)) = overrides.records.get(record_id) {
        return corrected.clone();
    }
    match raw_message_id.and_then(|id| overrides.messages.get(id)) {
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
                resolved_subitem_id: Some("v_jcl".to_string()),
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
                target_record_id: None,
                target_record_revision: None,
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
        assert!(overrides.messages.contains_key("m1"));

        let fallback = parsed_with("悠人", "hr_resume");
        let applied = apply_parse_override(fallback, "record-a", Some("m1"), &overrides);
        assert_eq!(
            applied.items[0].resolved_item_id.as_deref(),
            Some("pass_sp")
        );

        let untouched = apply_parse_override(
            parsed_with("悠人", "hr_resume"),
            "record-b",
            Some("m2"),
            &overrides,
        );
        assert_eq!(
            untouched.items[0].resolved_item_id.as_deref(),
            Some("hr_resume")
        );
    }
}
