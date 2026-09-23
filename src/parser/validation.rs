use crate::domain::claim::ClaimLine;
use crate::domain::event::{ClaimCancelled, ClaimCreated, DomainEvent, EventEnvelope};
use crate::domain::ids::UserId;
use crate::domain::item::RoundContext;
use crate::error::AppResult;
use crate::parser::alias_match;
use crate::parser::parsed_event::{ParsedIntent, ParsedMessage};

pub enum ValidationOutcome {
    Ok(EventEnvelope),
    NeedConfirm(crate::parser::reply::BotReply),
    Reject(crate::parser::reply::BotReply),
    Ignore,
}

#[derive(Clone)]
pub struct EventValidator {
    pub confidence_threshold: f32,
}

impl EventValidator {
    pub fn new(confidence_threshold: f32) -> Self {
        Self {
            confidence_threshold,
        }
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
        use crate::parser::reply::BotReply;

        if parsed.intent == ParsedIntent::Unknown {
            return Ok(ValidationOutcome::Ignore);
        }

        if !parsed.ambiguous_parts.is_empty() {
            return Ok(ValidationOutcome::NeedConfirm(BotReply::NeedConfirm {
                text: format!(
                    "以下部分不确定：{}。请回复确认。",
                    parsed.ambiguous_parts.join("，")
                ),
                confirm_token: uuid::Uuid::new_v4().to_string(),
            }));
        }

        if parsed.confidence < self.confidence_threshold {
            return Ok(ValidationOutcome::Reject(BotReply::Text(format!(
                "识别置信度不足 ({:.0}%)，请按格式重发。",
                parsed.confidence * 100.0
            ))));
        }

        match parsed.intent {
            ParsedIntent::Claim => {
                // Rule-parsed messages already resolved (item, variant) with context;
                // use that hint directly. Otherwise fall back to alias matching.
                let use_hint = parsed.items.iter().all(|pi| pi.resolved_item_id.is_some());
                let (round_id, pairs): (
                    crate::domain::ids::RoundId,
                    Vec<(crate::domain::ids::ItemId, Option<String>)>,
                ) = if use_hint {
                    let rid = parsed
                        .items
                        .iter()
                        .find_map(|pi| pi.resolved_round_id.clone())
                        .or_else(|| active_rounds.first().map(|r| r.round_id.0.clone()))
                        .unwrap_or_else(|| "unknown".to_string());
                    let pairs = parsed
                        .items
                        .iter()
                        .map(|pi| {
                            (
                                crate::domain::ids::ItemId(pi.resolved_item_id.clone().unwrap()),
                                pi.resolved_variant_id.clone(),
                            )
                        })
                        .collect();
                    (crate::domain::ids::RoundId(rid), pairs)
                } else {
                    let resolved =
                        alias_match::resolve_multiple_items(&parsed.items, active_rounds);
                    for (i, r) in resolved.iter().enumerate() {
                        if !r.resolved {
                            match &r.ambiguity {
                                Some(msg) => {
                                    return Ok(ValidationOutcome::Reject(BotReply::Text(
                                        msg.clone(),
                                    )));
                                }
                                None => {
                                    return Ok(ValidationOutcome::Reject(BotReply::Text(format!(
                                        "无法识别商品：{}",
                                        parsed.items[i].name
                                    ))));
                                }
                            }
                        }
                    }
                    let rid = resolved[0].round_id.clone().unwrap();
                    let pairs = resolved
                        .iter()
                        .map(|r| (r.item_id.clone().unwrap(), r.variant_id.clone()))
                        .collect();
                    (rid, pairs)
                };

                for pi in &parsed.items {
                    if pi.quantity == 0 {
                        return Ok(ValidationOutcome::Reject(BotReply::Text(format!(
                            "商品 {} 数量为 0，未记录。",
                            pi.name
                        ))));
                    }
                    if pi.quantity > 99 {
                        return Ok(ValidationOutcome::Reject(BotReply::Text(format!(
                            "商品 {} 数量 {} 超出单次上限 99，请分开发送。",
                            pi.name, pi.quantity
                        ))));
                    }
                }

                let claim_id = crate::domain::ids::ClaimId(uuid::Uuid::new_v4().to_string());
                let event_id = crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string());

                let items: Vec<ClaimLine> = parsed
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, pi)| {
                        let normalized = crate::parser::normalize::normalize_claim_item(pi);
                        let item_id = pairs[i].0.clone();
                        let claim_type = match normalized.claim_type.as_deref() {
                            Some("Single") => crate::domain::claim::ClaimType::Single,
                            Some("GiftClaim") => crate::domain::claim::ClaimType::GiftClaim,
                            _ => crate::domain::claim::ClaimType::Split,
                        };
                        let slot_policy = match normalized.slot_policy.as_deref() {
                            Some("TailLocked") => crate::domain::claim::SlotPolicy::TailLocked,
                            Some("AdminFixed") => crate::domain::claim::SlotPolicy::AdminFixed,
                            Some("ColumnLocked") => crate::domain::claim::SlotPolicy::ColumnLocked,
                            Some("FullBox") => crate::domain::claim::SlotPolicy::FullBox,
                            _ => crate::domain::claim::SlotPolicy::Normal,
                        };
                        ClaimLine {
                            item_id,
                            variant_id: pairs[i].1.clone(),
                            quantity: pi.quantity,
                            claim_type,
                            slot_policy,
                            is_proxy_card: pi.is_proxy_card.unwrap_or(false),
                            notes: normalized.notes.clone().or(pi.notes.clone()),
                        }
                    })
                    .collect();

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
                        source_text: parsed
                            .items
                            .first()
                            .map(|i| i.name.clone())
                            .unwrap_or_default(),
                        parse_trace: None,
                        validation_trace: vec![],
                    }),
                    status: crate::domain::event::EventStatus::Active,
                };

                Ok(ValidationOutcome::Ok(event))
            }

            ParsedIntent::Cancel => {
                if active_rounds.is_empty() {
                    return Ok(ValidationOutcome::Reject(BotReply::Text(
                        "没有活跃的团可撤销。".to_string(),
                    )));
                }

                let round_id = if let Some(ref hint) = parsed.round_hint {
                    active_rounds
                        .iter()
                        .find(|r| {
                            r.title.contains(hint.as_str()) || r.round_id.0.contains(hint.as_str())
                        })
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

            ParsedIntent::AdminCommand => Ok(ValidationOutcome::Reject(BotReply::Text(
                "管理员命令请使用斜杠命令格式。".to_string(),
            ))),

            _ => Ok(ValidationOutcome::Ignore),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parsed_event::ParsedClaimItem;

    fn claim_item(name: &str, quantity: u32) -> ParsedClaimItem {
        ParsedClaimItem {
            name: name.to_string(),
            category_hint: None,
            quantity,
            claim_type: Some("Split".to_string()),
            is_proxy_card: Some(false),
            slot_policy: Some("Normal".to_string()),
            notes: None,
            resolved_item_id: Some("item_a".to_string()),
            resolved_variant_id: None,
            resolved_round_id: Some("round_a".to_string()),
        }
    }

    fn parsed(intent: ParsedIntent, items: Vec<ParsedClaimItem>) -> ParsedMessage {
        ParsedMessage {
            intent,
            round_hint: None,
            items,
            cancel_target_hint: None,
            admin_command: None,
            confidence: 0.95,
            ambiguous_parts: vec![],
        }
    }

    async fn validate(parsed: ParsedMessage) -> ValidationOutcome {
        let validator = EventValidator::new(0.65);
        validator
            .validate(
                parsed,
                &UserId("u1".to_string()),
                "g1",
                Some("m1".to_string()),
                &[],
                chrono::Utc::now(),
                1,
            )
            .await
            .expect("validate")
    }

    #[tokio::test]
    async fn zero_quantity_is_rejected() {
        let outcome = validate(parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 0)])).await;
        assert!(matches!(outcome, ValidationOutcome::Reject(_)));
    }

    #[tokio::test]
    async fn quantity_over_limit_is_rejected() {
        let outcome = validate(parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 100)])).await;
        assert!(matches!(outcome, ValidationOutcome::Reject(_)));
    }

    #[tokio::test]
    async fn ambiguous_parts_need_confirm() {
        let mut message = parsed(ParsedIntent::Claim, vec![claim_item("商品甲", 1)]);
        message.ambiguous_parts = vec!["商品名不确定".to_string()];
        let outcome = validate(message).await;
        assert!(matches!(outcome, ValidationOutcome::NeedConfirm(_)));
    }

    #[tokio::test]
    async fn unknown_intent_is_ignored() {
        let outcome = validate(parsed(ParsedIntent::Unknown, vec![])).await;
        assert!(matches!(outcome, ValidationOutcome::Ignore));
    }
}
