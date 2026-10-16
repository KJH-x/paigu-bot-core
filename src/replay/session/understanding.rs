use chrono::{DateTime, Utc};

use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{ItemId, RoundId, UserId};
use crate::domain::item::RoundContext;
use crate::engine::replay::describe_event;
use crate::messages::processing::{
    stabilize_events, ProcessingOutcome, Understanding, UnderstandingSource,
};
use crate::messages::MessageRecord;
use crate::parser::parsed_event::{ParsedIntent, ParsedMessage};
use crate::parser::policy;
use crate::parser::validation::{EventValidator, ValidateContext, ValidationOutcome};
use crate::round::RoundPhase;
use crate::settings::AppConfig;

pub(crate) async fn prepare_understanding(
    cfg: &AppConfig,
    rec: &MessageRecord,
    parsed: ParsedMessage,
    source: UnderstandingSource,
) -> Understanding {
    let mut result = Understanding::terminal(source, "Ignored", "无法识别为排谷/撤销意图", None);
    result.parsed = Some(parsed.clone());
    result.is_modify = parsed.intent == ParsedIntent::Modify;
    let mut input = parsed;
    if result.is_modify {
        input.intent = ParsedIntent::Claim;
    }
    if input.intent == ParsedIntent::Claim
        && input.items.is_empty()
        && input.ambiguous_parts.is_empty()
    {
        result.validation.detail = "未解析出商品".to_string();
        return result;
    }
    let now = DateTime::<Utc>::from_timestamp_millis(rec.timestamp_ms).unwrap_or_else(Utc::now);
    let rounds = vec![RoundContext {
        round_id: RoundId(cfg.round.round_id.clone()),
        title: cfg.round.title.clone(),
        items: cfg.round.to_items(),
    }];
    let outcome = EventValidator::new(super::REPLAY_CONFIDENCE_THRESHOLD)
        .validate(
            input,
            ValidateContext {
                user_id: &UserId(rec.user_id.clone()),
                group_id: &rec.group_id,
                raw_message_id: Some(rec.message_id.clone()),
                active_rounds: &rounds,
                box_map: &crate::settings::box_redirect_map(&cfg.round.items),
                now,
                sequence: rec.seq,
            },
        )
        .await;
    match outcome {
        Ok(ValidationOutcome::Ok(mut event)) => {
            let mut detail = describe_event(&event);
            if result.is_modify {
                detail = format!("改单：{detail}");
                if let DomainEvent::ClaimCreated(created) = &event.payload {
                    let mut targets = Vec::<ItemId>::new();
                    for line in &created.items {
                        if !targets.contains(&line.item_id) {
                            targets.push(line.item_id.clone());
                        }
                    }
                    for target in targets {
                        result.events.push(super::cancel_for_modify(
                            &rounds[0].round_id,
                            rec,
                            rec.seq,
                            now,
                            target,
                        ));
                    }
                }
            }
            if rec.processing.is_none() {
                super::stabilize_event(&mut event, rec.seq);
            }
            result.events.push(*event);
            if rec.processing.is_some() {
                stabilize_events(&mut result.events, rec);
            }
            result.validation = ProcessingOutcome::new("Applied", detail, None);
        }
        Ok(ValidationOutcome::NeedConfirm(reply)) | Ok(ValidationOutcome::Reject(reply)) => {
            let is_confirm = matches!(reply, crate::parser::reply::BotReply::NeedConfirm { .. });
            let text = reply.text_content().unwrap_or("需要核对").to_string();
            result.validation = ProcessingOutcome::new(
                if is_confirm {
                    "NeedConfirm"
                } else {
                    "Rejected"
                },
                text.clone(),
                Some(text),
            );
        }
        Ok(ValidationOutcome::Ignore) => {}
        Err(error) => {
            result.validation = ProcessingOutcome::new("Error", format!("处理失败: {error}"), None);
        }
    }
    result
}

#[derive(Clone)]
pub(crate) struct Evaluation {
    pub outcome: ProcessingOutcome,
    pub events: Vec<EventEnvelope>,
    pub priority_claim: bool,
}

impl Evaluation {
    pub fn skipped(status: &str, detail: impl Into<String>) -> Self {
        Self {
            outcome: ProcessingOutcome::new(status, detail, None),
            events: vec![],
            priority_claim: false,
        }
    }
}

pub(crate) fn evaluate_understanding(
    cfg: &AppConfig,
    rec: &MessageRecord,
    understood: &Understanding,
    manual_phase: Option<RoundPhase>,
) -> Evaluation {
    let sender_uid = rec.sender_user_id.as_deref().unwrap_or(&rec.user_id);
    let sender_nick = rec.sender_nickname.as_deref().unwrap_or(&rec.nickname);
    if rec.routed.to_ascii_lowercase().starts_with("drop")
        || !policy::group_allowed(&cfg.gateway.whitelist_groups, &rec.group_id)
    {
        return Evaluation::skipped("Dropped", "非白名单/Drop");
    }
    let mut candidates = crate::settings::priority_candidates(cfg, &rec.user_id, &rec.nickname);
    candidates.push(sender_uid.to_string());
    candidates.push(sender_nick.to_string());
    if !policy::member_allowed(
        &cfg.gateway.whitelist_members,
        &candidates.iter().map(String::as_str).collect::<Vec<_>>(),
    ) {
        return Evaluation::skipped("Dropped", "成员不在白名单");
    }
    if understood.events.is_empty() {
        return Evaluation {
            outcome: understood.validation.clone(),
            events: vec![],
            priority_claim: false,
        };
    }
    for event in &understood.events {
        if event.round_id.0 != cfg.round.round_id
            || event.group_id != rec.group_id
            || event.user_id.0 != rec.user_id
        {
            return Evaluation::skipped("Error", "已存动作与原消息身份不一致，需核对");
        }
        let valid = match &event.payload {
            DomainEvent::ClaimCreated(claim) => {
                claim.user_id.0 == rec.user_id
                    && claim.items.iter().all(|line| {
                        cfg.round.items.iter().any(|item| {
                            item.item_id == line.item_id.0
                                && line.subitem_id.as_ref().is_none_or(|id| {
                                    item.subitems.iter().any(|s| &s.subitem_id == id)
                                })
                        })
                    })
            }
            DomainEvent::ClaimCancelled(cancel) => cancel
                .target_item_id
                .as_ref()
                .is_none_or(|id| cfg.round.items.iter().any(|i| i.item_id == id.0)),
            _ => false,
        };
        if !valid {
            return Evaluation::skipped(
                "Rejected",
                "已存动作的商品/子项已不在当前目录，未按新别名改排",
            );
        }
    }
    let is_priority = policy::is_priority(cfg, sender_uid, sender_nick);
    let rejection = if policy::in_priority_at(cfg, rec.timestamp_ms) && !is_priority {
        Some("优先时段仅限预存(购物金)用户".to_string())
    } else {
        manual_phase
            .or_else(|| policy::phase_at(&cfg.round.phases, rec.timestamp_ms))
            .and_then(|phase| {
                understood
                    .events
                    .iter()
                    .find_map(|event| policy::phase_rejection(cfg, event, phase, is_priority))
            })
    };
    if let Some(reason) = rejection {
        let reason = if matches!(
            manual_phase,
            Some(RoundPhase::Settling | RoundPhase::Locked)
        ) {
            format!("已锁定，不再接受排/撤/改：{reason}")
        } else {
            reason
        };
        return Evaluation {
            outcome: ProcessingOutcome::new("Rejected", reason.clone(), Some(reason)),
            events: vec![],
            priority_claim: false,
        };
    }
    Evaluation {
        outcome: understood.validation.clone(),
        events: understood.events.clone(),
        priority_claim: is_priority
            && understood
                .events
                .iter()
                .any(|event| matches!(event.payload, DomainEvent::ClaimCreated(_))),
    }
}
