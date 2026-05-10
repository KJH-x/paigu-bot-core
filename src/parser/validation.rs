use crate::domain::ids::UserId;
use crate::domain::event::{EventEnvelope, DomainEvent, ClaimCreated, ClaimCancelled};
use crate::domain::claim::ClaimLine;
use crate::domain::item::RoundContext;
use crate::parser::parsed_event::{ParsedMessage, ParsedIntent};
use crate::parser::alias_match;
use crate::error::AppResult;

pub enum ValidationOutcome {
    Ok(EventEnvelope),
    NeedConfirm(crate::inbound::command_router::BotReply),
    Reject(crate::inbound::command_router::BotReply),
    Ignore,
}

#[derive(Clone)]
pub struct EventValidator {
    pub confidence_threshold: f32,
}

impl EventValidator {
    pub fn new(confidence_threshold: f32) -> Self {
        Self { confidence_threshold }
    }

    pub async fn validate(
        &self,
        parsed: ParsedMessage,
        user_id: &UserId,
        group_id: &str,
        raw_message_id: Option<String>,
        active_rounds: &[RoundContext],
        now: chrono::DateTime<chrono::Utc>,
        sequence: i64,
    ) -> AppResult<ValidationOutcome> {
        use crate::inbound::command_router::BotReply;

        if parsed.confidence < self.confidence_threshold {
            return Ok(ValidationOutcome::Reject(BotReply::Text(
                format!("识别置信度不足 ({:.0}%)，请按格式重发。", parsed.confidence * 100.0)
            )));
        }

        if !parsed.ambiguous_parts.is_empty() {
            return Ok(ValidationOutcome::NeedConfirm(BotReply::NeedConfirm {
                text: format!("以下部分不确定：{}。请回复确认。", parsed.ambiguous_parts.join("，")),
                confirm_token: uuid::Uuid::new_v4().to_string(),
            }));
        }

        match parsed.intent {
            ParsedIntent::Claim => {
                let resolved = alias_match::resolve_multiple_items(&parsed.items, active_rounds);

                for (i, r) in resolved.iter().enumerate() {
                    if !r.resolved {
                        match &r.ambiguity {
                            Some(msg) => {
                                return Ok(ValidationOutcome::Reject(BotReply::Text(msg.clone())));
                            }
                            None => {
                                return Ok(ValidationOutcome::Reject(BotReply::Text(
                                    format!("无法识别商品：{}", parsed.items[i].name)
                                )));
                            }
                        }
                    }
                }

                let round_id = resolved[0].round_id.clone().unwrap();
                let claim_id = crate::domain::ids::ClaimId(uuid::Uuid::new_v4().to_string());
                let event_id = crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string());

                let items: Vec<ClaimLine> = parsed.items.iter().enumerate().map(|(i, pi)| {
                    let normalized = crate::parser::normalize::normalize_claim_item(pi);
                    let item_id = resolved[i].item_id.clone().unwrap();
                    let claim_type = match normalized.claim_type.as_deref() {
                        Some("Single") => crate::domain::claim::ClaimType::Single,
                        Some("GiftClaim") => crate::domain::claim::ClaimType::GiftClaim,
                        _ => crate::domain::claim::ClaimType::Split,
                    };
                    let slot_policy = match normalized.slot_policy.as_deref() {
                        Some("TailLocked") => crate::domain::claim::SlotPolicy::TailLocked,
                        Some("AdminFixed") => crate::domain::claim::SlotPolicy::AdminFixed,
                        Some("ColumnLocked") => crate::domain::claim::SlotPolicy::ColumnLocked,
                        _ => crate::domain::claim::SlotPolicy::Normal,
                    };
                    ClaimLine {
                        item_id,
                        quantity: pi.quantity,
                        claim_type,
                        slot_policy,
                        is_proxy_card: pi.is_proxy_card.unwrap_or(false),
                        notes: normalized.notes.clone().or(pi.notes.clone()),
                    }
                }).collect();

                let event = EventEnvelope {
                    event_id,
                    round_id,
                    group_id: group_id.to_string(),
                    user_id: user_id.clone(),
                    raw_message_id,
                    event_type: "claim_created".to_string(),
                    effective_at: now,
                    sequence,
                    payload: DomainEvent::ClaimCreated(ClaimCreated {
                        claim_id,
                        user_id: user_id.clone(),
                        items,
                        source_text: parsed.items.first().map(|i| i.name.clone()).unwrap_or_default(),
                        parse_trace: None,
                        validation_trace: vec![],
                    }),
                    status: crate::domain::event::EventStatus::Active,
                };

                Ok(ValidationOutcome::Ok(event))
            }

            ParsedIntent::Cancel => {
                if active_rounds.is_empty() {
                    return Ok(ValidationOutcome::Reject(BotReply::Text("没有活跃的团可撤销。".to_string())));
                }

                let round_id = if let Some(ref hint) = parsed.round_hint {
                    active_rounds.iter()
                        .find(|r| r.title.contains(hint.as_str()) || r.round_id.0.contains(hint.as_str()))
                        .map(|r| r.round_id.clone())
                        .unwrap_or(active_rounds[0].round_id.clone())
                } else {
                    active_rounds[0].round_id.clone()
                };
                let event_id = crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string());

                let (target_item_id, quantity) = if !parsed.items.is_empty() {
                    let resolved = alias_match::resolve_item(&parsed.items[0], active_rounds);
                    if let Some(item_id) = resolved.item_id {
                        (Some(item_id), Some(parsed.items[0].quantity))
                    } else {
                        (None, Some(parsed.items[0].quantity))
                    }
                } else {
                    (None, None)
                };

                let event = EventEnvelope {
                    event_id,
                    round_id,
                    group_id: group_id.to_string(),
                    user_id: user_id.clone(),
                    raw_message_id,
                    event_type: "claim_cancelled".to_string(),
                    effective_at: now,
                    sequence,
                    payload: DomainEvent::ClaimCancelled(ClaimCancelled {
                        target_claim_id: None,
                        target_item_id,
                        quantity,
                        reason: parsed.cancel_target_hint,
                        parse_trace: None,
                        validation_trace: vec![],
                    }),
                    status: crate::domain::event::EventStatus::Active,
                };

                Ok(ValidationOutcome::Ok(event))
            }

            ParsedIntent::AdminCommand => {
                Ok(ValidationOutcome::Reject(BotReply::Text("管理员命令请使用斜杠命令格式。".to_string())))
            }

            _ => {
                Ok(ValidationOutcome::Ignore)
            }
        }
    }
}
