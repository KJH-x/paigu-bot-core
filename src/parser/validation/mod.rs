use crate::domain::claim::ClaimLine;
use crate::domain::event::{ClaimCancelled, ClaimCreated, DomainEvent, EventEnvelope};
use crate::domain::ids::{ItemId, UserId};
use crate::domain::item::RoundContext;
use crate::parser::alias_match;
use crate::parser::parsed_event::{ParsedIntent, ParsedMessage};

/// 校验所需上下文（避免 `validate` 参数过多）。
pub struct ValidateContext<'a> {
    pub user_id: &'a UserId,
    pub group_id: &'a str,
    pub raw_message_id: Option<String>,
    pub active_rounds: &'a [RoundContext],
    /// §5 整盒路由：拼团项（子项为空时）→ 整盒项 id 的映射。
    pub box_map: &'a std::collections::HashMap<ItemId, ItemId>,
    pub now: chrono::DateTime<chrono::Utc>,
    pub sequence: i64,
}

pub enum ValidationOutcome {
    Ok(Box<EventEnvelope>),
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
        ctx: ValidateContext<'_>,
    ) -> anyhow::Result<ValidationOutcome> {
        use crate::parser::reply::BotReply;

        let ValidateContext {
            user_id,
            group_id,
            raw_message_id,
            active_rounds,
            box_map,
            now,
            sequence,
        } = ctx;

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
                // Rule-parsed messages already resolved (item, subitem) with context;
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
                                pi.resolved_subitem_id.clone(),
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
                        .map(|r| (r.item_id.clone().unwrap(), r.subitem_id.clone()))
                        .collect();
                    (rid, pairs)
                };

                // §5 整盒路由：命中拼团项且子项为空 → 重定向到其整盒项（整盒走单领队列）。
                let (pairs, redirected): (Vec<(ItemId, Option<String>)>, Vec<bool>) = pairs
                    .into_iter()
                    .map(|(item_id, subitem_id)| {
                        if subitem_id.is_none() {
                            if let Some(box_id) = box_map.get(&item_id) {
                                return ((box_id.clone(), None), true);
                            }
                        }
                        ((item_id, subitem_id), false)
                    })
                    .unzip();

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
                        let claim_type = if redirected[i] {
                            // §5：重定向到整盒 → 单领队列（整盒语义）。
                            crate::domain::claim::ClaimType::Single
                        } else {
                            match normalized.claim_type.as_deref() {
                                Some("Single") => crate::domain::claim::ClaimType::Single,
                                Some("GiftClaim") => crate::domain::claim::ClaimType::GiftClaim,
                                _ => crate::domain::claim::ClaimType::Split,
                            }
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
                            subitem_id: pairs[i].1.clone(),
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

                Ok(ValidationOutcome::Ok(Box::new(event)))
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
                    let item = &parsed.items[0];
                    if item.quantity == 0 || item.quantity > 99 {
                        return Ok(ValidationOutcome::Reject(BotReply::Text(
                            "撤销数量应为 1–99，请核对后重发".into(),
                        )));
                    }
                    let item_id = if let Some(id) = &item.resolved_item_id {
                        active_rounds
                            .iter()
                            .find(|round| {
                                round.round_id == round_id
                                    && item
                                        .resolved_round_id
                                        .as_ref()
                                        .is_none_or(|hint| hint == &round.round_id.0)
                            })
                            .and_then(|round| {
                                round.items.iter().find(|entry| &entry.item_id.0 == id)
                            })
                            .map(|entry| entry.item_id.clone())
                    } else {
                        let resolved = alias_match::resolve_item(item, active_rounds);
                        resolved
                            .item_id
                            .filter(|_| resolved.round_id.as_ref() == Some(&round_id))
                    };
                    let Some(item_id) = item_id else {
                        return Ok(ValidationOutcome::NeedConfirm(BotReply::NeedConfirm {
                            text: format!(
                                "无法确认要撤销的商品：{}；未撤销最近一笔，请核对商品后修订",
                                item.name
                            ),
                            confirm_token: uuid::Uuid::new_v4().to_string(),
                        }));
                    };
                    (Some(item_id), Some(item.quantity))
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

                Ok(ValidationOutcome::Ok(Box::new(event)))
            }

            ParsedIntent::AdminCommand => Ok(ValidationOutcome::Reject(BotReply::Text(
                "管理员命令请使用斜杠命令格式。".to_string(),
            ))),

            _ => Ok(ValidationOutcome::Ignore),
        }
    }
}

#[cfg(test)]
mod tests;
