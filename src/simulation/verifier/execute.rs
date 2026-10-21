use super::{MessageOutcome, RoundFixture, VerifyResult};
use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::EventEnvelope;
use crate::domain::ids::{EligibilityId, ItemId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::domain::round::{Round, RoundConfig, RoundStatus};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::policy;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidateContext, ValidationOutcome};
use crate::replay::replay_engine::{ReplayEngine, ReplayOptions};
use crate::simulation::queue_file::{read_jsonl_queue_file, QueueMessageRecord};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub async fn verify(queue_path: &Path, fixture: RoundFixture) -> anyhow::Result<VerifyResult> {
    let records = read_jsonl_queue_file(queue_path.to_str().unwrap_or_default()).await?;

    let nickname_map: HashMap<String, String> = records
        .iter()
        .map(|r| (r.nickname.clone(), r.user_id.clone()))
        .collect();

    let round_id = RoundId(fixture.round_id.clone());
    let round_contexts = vec![RoundContext {
        round_id: round_id.clone(),
        title: fixture.title.clone(),
        items: fixture.items.clone(),
    }];

    let validator = EventValidator::new(0.65);
    let mut eligibilities: Vec<Eligibility> = Vec::new();
    let mut fund_users: HashSet<String> = HashSet::new();

    // 预存(购物金)用户：整期优先（priority_level 10）。
    let epoch = DateTime::from_timestamp_millis(0).unwrap_or_else(Utc::now);
    for u in &fixture.priority_users {
        eligibilities.push(make_eligibility(
            &round_id,
            u,
            10,
            None,
            "购物金预存",
            epoch,
        ));
        fund_users.insert(u.clone());
    }
    let mut events: Vec<EventEnvelope> = Vec::new();
    let mut outcomes: Vec<MessageOutcome> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut round_closed = false;

    for (idx, rec) in records.iter().enumerate() {
        let now = DateTime::from_timestamp_millis(rec.timestamp_ms).unwrap_or_else(Utc::now);
        let text = rec.text.trim().to_string();
        let dedup_key = format!("{}::{}", rec.group_id, rec.message_id);

        if !seen_ids.insert(dedup_key) {
            outcomes.push(outcome(
                rec,
                "Duplicate",
                "重复 message_id，已按幂等忽略".to_string(),
            ));
            continue;
        }

        if text.starts_with('/') {
            if !rec.is_admin {
                outcomes.push(outcome(
                    rec,
                    "Unsupported",
                    "非管理员发送斜杠命令".to_string(),
                ));
                continue;
            }
            if text.starts_with("/加优先") {
                match parse_admin_eligibility(&text, &nickname_map, &fixture.items, &round_id, now)
                {
                    Some(e) => {
                        let is_fund = e.note.as_deref().unwrap_or("").contains("购物金");
                        if is_fund {
                            fund_users.insert(e.user_id.0.clone());
                        }
                        outcomes.push(outcome(
                            rec,
                            "Applied",
                            format!("授权优先权 user={} level={}", e.user_id.0, e.priority_level),
                        ));
                        eligibilities.push(e);
                    }
                    None => {
                        outcomes.push(outcome(rec, "Unsupported", "无法解析 /加优先".to_string()))
                    }
                }
            } else if text.starts_with("/结团") {
                round_closed = true;
                outcomes.push(outcome(rec, "Applied", "结团".to_string()));
            } else if text.starts_with("/开团") {
                outcomes.push(outcome(rec, "Applied", "开团(模拟中默认已开)".to_string()));
            } else if text.starts_with("/导出") {
                outcomes.push(outcome(
                    rec,
                    "Unsupported",
                    "导出不在模拟范围内".to_string(),
                ));
            } else if text.starts_with("/锁位") || text.starts_with("/修正") {
                outcomes.push(outcome(
                    rec,
                    "Unsupported",
                    "管理员锁位/修正暂未进入模拟重放".to_string(),
                ));
            } else if text.starts_with("/设置优惠") {
                outcomes.push(outcome(
                    rec,
                    "Unsupported",
                    "优惠规则解析暂未进入模拟重放".to_string(),
                ));
            } else if text.starts_with("/加商品") {
                outcomes.push(outcome(rec, "Unsupported", "模拟中商品表固定".to_string()));
            } else {
                outcomes.push(outcome(rec, "Unsupported", "未知管理员命令".to_string()));
            }
            continue;
        }

        if round_closed {
            outcomes.push(outcome(rec, "Rejected", "团已结团，拒绝排谷".to_string()));
            continue;
        }

        // 优先时段：仅预存(购物金)用户可排，其余请求拒绝。
        if policy::in_priority_window(fixture.priority_window, rec.timestamp_ms)
            && !policy::is_priority_user(&fixture.priority_users, &[rec.user_id.as_str()])
        {
            outcomes.push(outcome(
                rec,
                "Rejected",
                "优先时段仅限预存(购物金)用户，请求已拒绝".to_string(),
            ));
            continue;
        }

        if text.contains("购物金")
            && !text.contains("非购物金")
            && !fund_users.contains(&rec.user_id)
        {
            fund_users.insert(rec.user_id.clone());
            eligibilities.push(make_eligibility(
                &round_id,
                &rec.user_id,
                10,
                None,
                "购物金(自述)",
                now,
            ));
        }

        let parsed = RuleParser::parse(&text, &fixture.items, rec.is_admin);

        if parsed.intent == ParsedIntent::Modify {
            outcomes.push(outcome(
                rec,
                "Unsupported",
                "改单意图未在校验层实现".to_string(),
            ));
            continue;
        }

        let user_id = UserId(rec.user_id.clone());
        // 旧 `simulate` 路径使用夹具 `Item`（无 `box_item_id`），不做整盒重定向。
        let box_map = std::collections::HashMap::new();
        let validation = validator
            .validate(
                parsed,
                ValidateContext {
                    user_id: &user_id,
                    group_id: &rec.group_id,
                    raw_message_id: Some(rec.message_id.clone()),
                    active_rounds: &round_contexts,
                    box_map: &box_map,
                    now,
                    sequence: (idx + 1) as i64,
                },
            )
            .await?;

        match validation {
            ValidationOutcome::Ok(event) => {
                outcomes.push(outcome_from_event(rec, &event));
                events.push(*event);
            }
            ValidationOutcome::NeedConfirm(reply) => {
                outcomes.push(outcome(
                    rec,
                    "NeedConfirm",
                    reply.text_content().unwrap_or("").to_string(),
                ));
            }
            ValidationOutcome::Reject(reply) => {
                outcomes.push(outcome(
                    rec,
                    "Rejected",
                    reply.text_content().unwrap_or("").to_string(),
                ));
            }
            ValidationOutcome::Ignore => {
                outcomes.push(outcome(
                    rec,
                    "Ignored",
                    "无法识别为排谷/撤销意图".to_string(),
                ));
            }
        }
    }

    let round = Round {
        round_id: round_id.clone(),
        group_id: fixture.group_id.clone(),
        title: fixture.title.clone(),
        status: RoundStatus::Active,
        start_at: None,
        end_at: None,
        allow_cancel: true,
        allow_modify: true,
        default_timezone: "Asia/Shanghai".to_string(),
        created_by: "simulation".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let round_config = RoundConfig {
        round,
        items: fixture.items.clone(),
        aliases: vec![],
        eligibility: eligibilities.clone(),
    };

    let engine = ReplayEngine::new();
    let options = ReplayOptions {
        replay_id: format!("replay_{}", fixture.round_id),
        include_settlement: true,
        snapshot_interval: 50,
    };
    let replay = engine
        .replay(round_config, events, options)
        .await
        .map_err(|e| anyhow::anyhow!("重放失败: {}", e))?;

    let final_snapshot = replay.final_snapshot.clone();
    let final_settlement = replay
        .steps
        .last()
        .and_then(|s| s.settlement_snapshot.clone());

    let applied_count = outcomes.iter().filter(|o| o.status == "Applied").count();
    let rejected_count = outcomes.iter().filter(|o| o.status == "Rejected").count();
    let need_confirm_count = outcomes
        .iter()
        .filter(|o| o.status == "NeedConfirm")
        .count();
    let ignored_count = outcomes.iter().filter(|o| o.status == "Ignored").count();
    let unsupported_count = outcomes
        .iter()
        .filter(|o| o.status == "Unsupported")
        .count();
    let duplicate_count = outcomes.iter().filter(|o| o.status == "Duplicate").count();

    Ok(VerifyResult {
        round_id: fixture.round_id.clone(),
        group_id: fixture.group_id.clone(),
        message_count: records.len(),
        applied_count,
        rejected_count,
        need_confirm_count,
        ignored_count,
        unsupported_count,
        duplicate_count,
        outcomes,
        eligibility: eligibilities,
        final_snapshot,
        final_settlement,
        replay,
    })
}

fn outcome(rec: &QueueMessageRecord, status: &str, detail: String) -> MessageOutcome {
    MessageOutcome {
        source_sequence: rec.source_sequence,
        message_id: rec.message_id.clone(),
        user_id: rec.user_id.clone(),
        nickname: rec.nickname.clone(),
        text: rec.text.clone(),
        status: status.to_string(),
        detail,
    }
}

fn outcome_from_event(rec: &QueueMessageRecord, event: &EventEnvelope) -> MessageOutcome {
    let detail = match &event.payload {
        crate::domain::event::DomainEvent::ClaimCreated(c) => {
            let items: Vec<String> = c
                .items
                .iter()
                .map(|l| match &l.subitem_id {
                    Some(subitem) => format!(
                        "{}@{}x{}[{}]",
                        l.item_id.0,
                        subitem,
                        l.quantity,
                        l.slot_policy.as_str()
                    ),
                    None => format!("{}x{}[{}]", l.item_id.0, l.quantity, l.slot_policy.as_str()),
                })
                .collect();
            format!("claim: {}", items.join(", "))
        }
        crate::domain::event::DomainEvent::ClaimCancelled(c) => {
            format!(
                "cancel: item={:?} qty={:?}",
                c.target_item_id.as_ref().map(|i| i.0.clone()),
                c.quantity
            )
        }
        other => format!("event: {}", other.event_type_str()),
    };
    outcome(rec, "Applied", detail)
}

pub(crate) fn make_eligibility(
    round_id: &RoundId,
    user_id: &str,
    level: i32,
    item_ids: Option<Vec<ItemId>>,
    note: &str,
    valid_from: DateTime<Utc>,
) -> Eligibility {
    Eligibility {
        eligibility_id: EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        user_id: UserId(user_id.to_string()),
        priority_type: "shopping_fund".to_string(),
        priority_level: level,
        scope: EligibilityScope {
            item_ids,
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: Some(valid_from),
        valid_until: None,
        note: Some(note.to_string()),
    }
}

pub(crate) fn parse_admin_eligibility(
    text: &str,
    nickname_map: &HashMap<String, String>,
    items: &[Item],
    round_id: &RoundId,
    now: DateTime<Utc>,
) -> Option<Eligibility> {
    let rest = text
        .trim_start_matches('/')
        .trim_start_matches("加优先")
        .trim();

    let mut level = 10i32;
    let mut note = String::new();
    let mut scope_all = true;
    let mut scope_items: Vec<ItemId> = Vec::new();
    let mut user_token: Option<String> = None;
    let mut plain_tokens: Vec<String> = Vec::new();

    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("等级=") {
            level = v.parse().unwrap_or(10);
        } else if let Some(v) = tok.strip_prefix("备注=") {
            note = v.to_string();
        } else if let Some(v) = tok.strip_prefix("范围=") {
            if v.contains("全部") || v.contains("所有") {
                scope_all = true;
            } else if let Some(id) = resolve_item_token(v, items) {
                scope_all = false;
                scope_items.push(id);
            }
        } else if let Some(v) = tok.strip_prefix("用户=") {
            user_token = Some(v.trim_start_matches('@').to_string());
        } else {
            plain_tokens.push(tok.to_string());
        }
    }

    let user_from_plain = user_token.is_none();
    if user_token.is_none() {
        user_token = plain_tokens.first().cloned();
    }

    if scope_all {
        let skip = if user_from_plain { 1 } else { 0 };
        for t in plain_tokens.iter().skip(skip) {
            if let Some(id) = resolve_item_token(t, items) {
                scope_all = false;
                scope_items.push(id);
            }
        }
    }

    let token = user_token?;
    let user_id = nickname_map
        .get(&token)
        .cloned()
        .unwrap_or_else(|| token.clone());

    let scope = if scope_all { None } else { Some(scope_items) };
    let note = if note.is_empty() {
        "优先权".to_string()
    } else {
        note
    };

    Some(make_eligibility(
        round_id, &user_id, level, scope, &note, now,
    ))
}

fn resolve_item_token(token: &str, items: &[Item]) -> Option<ItemId> {
    let t = token.trim_start_matches('@');
    let mut best: Option<(i32, ItemId)> = None;
    for item in items {
        let mut score = 0i32;
        if item.name == t {
            score += 900;
        }
        if item.item_id.0 == t {
            score += 1000;
        }
        if item.aliases.iter().any(|a| a == t) {
            score += 800;
        }
        if !t.is_empty() && item.name.contains(t) {
            score += 400;
        }
        if score > 0 {
            match &best {
                Some((s, _)) if *s >= score => {}
                _ => best = Some((score, item.item_id.clone())),
            }
        }
    }
    best.map(|(_, id)| id)
}
