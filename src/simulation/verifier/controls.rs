use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::ids::{EligibilityId, ItemId, RoundId, UserId};
use crate::domain::item::Item;
use crate::messages::processing::{Understanding, UnderstandingSource};
use crate::simulation::queue_file::QueueMessageRecord;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub(super) fn command(
    record: &QueueMessageRecord,
    names: &HashMap<String, String>,
    items: &[Item],
    round: &RoundId,
    closed: &mut bool,
    eligibility: &mut Vec<Eligibility>,
) -> Understanding {
    let text = record.text.trim();
    let (status, detail) = if !record.is_admin {
        ("Unsupported", "非管理员发送斜杠命令".to_string())
    } else if text.starts_with("/加优先") {
        let grant = DateTime::from_timestamp_millis(record.timestamp_ms)
            .and_then(|at| parse_admin_eligibility(text, names, items, round, at));
        match grant {
            Some(grant) => {
                let detail = format!(
                    "授权优先权 user={} level={}",
                    grant.user_id.0, grant.priority_level
                );
                eligibility.push(grant);
                ("Applied", detail)
            }
            None => ("Unsupported", "无法解析 /加优先".to_string()),
        }
    } else if text.starts_with("/结团") {
        *closed = true;
        ("Applied", "结团".to_string())
    } else if text.starts_with("/开团") {
        ("Applied", "开团(模拟中默认已开)".to_string())
    } else if text.starts_with("/导出") {
        ("Unsupported", "导出不在模拟范围内".to_string())
    } else if text.starts_with("/锁位") || text.starts_with("/修正") {
        ("Unsupported", "管理员锁位/修正暂未进入模拟重放".to_string())
    } else if text.starts_with("/设置优惠") {
        ("Unsupported", "优惠规则解析暂未进入模拟重放".to_string())
    } else if text.starts_with("/加商品") {
        ("Unsupported", "模拟中商品表固定".to_string())
    } else {
        ("Unsupported", "未知管理员命令".to_string())
    };
    Understanding::terminal(UnderstandingSource::Control, status, detail, None)
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
        eligibility_id: EligibilityId(format!(
            "fixture-elig-{:x}",
            Sha256::digest(
                serde_json::json!([round_id, user_id, level, item_ids, note, valid_from])
                    .to_string()
                    .as_bytes()
            )
        )),
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
