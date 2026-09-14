use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::EventEnvelope;
use crate::domain::ids::{EligibilityId, ItemId, RoundId, UserId};
use crate::domain::item::{Item, ItemKind, ItemVariant, RoundContext};
use crate::domain::money::MoneyCents;
use crate::domain::round::{Round, RoundConfig, RoundStatus};
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::snapshot::AllocationSnapshot;
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::replay::replay_engine::{ReplayEngine, ReplayOptions, ReplayResult};
use crate::simulation::queue_file::{read_jsonl_queue_file, QueueMessageRecord};

#[derive(Debug, Clone, Serialize)]
pub struct MessageOutcome {
    pub source_sequence: u64,
    pub message_id: String,
    pub user_id: String,
    pub nickname: String,
    pub text: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub round_id: String,
    pub group_id: String,
    pub message_count: usize,
    pub applied_count: usize,
    pub rejected_count: usize,
    pub need_confirm_count: usize,
    pub ignored_count: usize,
    pub unsupported_count: usize,
    pub duplicate_count: usize,
    pub outcomes: Vec<MessageOutcome>,
    pub eligibility: Vec<Eligibility>,
    pub final_snapshot: AllocationSnapshot,
    pub final_settlement: Option<SettlementSnapshot>,
    pub replay: ReplayResult,
}

pub struct RoundFixture {
    pub round_id: String,
    pub title: String,
    pub group_id: String,
    pub items: Vec<Item>,
    /// 预存(购物金)用户；在 priority_window 内仅他们可排。
    pub priority_users: Vec<String>,
    /// 优先时段 [start_ms, end_ms)（end 独占）。
    pub priority_window: Option<(i64, i64)>,
}

pub async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let mut round_config: Option<String> = None;
    let mut queue: Option<String> = None;
    let mut out_dir: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--round-config" => {
                i += 1;
                round_config = args.get(i).cloned();
            }
            "--queue" => {
                i += 1;
                queue = args.get(i).cloned();
            }
            "--out" => {
                i += 1;
                out_dir = args.get(i).cloned();
            }
            other => {
                anyhow::bail!("未知参数: {}", other);
            }
        }
        i += 1;
    }

    let round_config = round_config.ok_or_else(|| anyhow::anyhow!("缺少 --round-config"))?;
    let queue = queue.ok_or_else(|| anyhow::anyhow!("缺少 --queue"))?;
    let out_dir = out_dir.unwrap_or_else(|| "out".to_string());

    let fixture = load_round_fixture(Path::new(&round_config))?;
    let result = verify(Path::new(&queue), fixture).await?;
    write_outputs(Path::new(&out_dir), &result)?;

    println!(
        "验证完成: {} 条消息, 生效 {} / 拒绝 {} / 需确认 {} / 忽略 {} / 未支持 {} / 重复 {}",
        result.message_count,
        result.applied_count,
        result.rejected_count,
        result.need_confirm_count,
        result.ignored_count,
        result.unsupported_count,
        result.duplicate_count
    );
    println!("输出目录: {}", out_dir);
    Ok(())
}

pub(crate) fn load_round_fixture(path: &Path) -> anyhow::Result<RoundFixture> {
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;

    let round_id = v.get("round_id").and_then(|x| x.as_str()).unwrap_or("round").to_string();
    let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let group_id = v.get("group_id").and_then(|x| x.as_str()).unwrap_or("").to_string();

    let priority_users: Vec<String> = v
        .get("priority_users")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let priority_window = v.get("priority_window").and_then(|w| {
        let s = w.get("start_ms").and_then(|x| x.as_i64());
        let e = w.get("end_ms").and_then(|x| x.as_i64());
        match (s, e) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        }
    });

    let items_v = v.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let mut items = Vec::new();
    for (idx, iv) in items_v.iter().enumerate() {
        let item_id = iv.get("item_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let name = iv.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let kind_str = iv.get("kind").and_then(|x| x.as_str()).unwrap_or("split");
        let kind = match kind_str {
            "single" => ItemKind::Single,
            "gift" => ItemKind::Gift,
            "shipping" => ItemKind::Shipping,
            "adjustment" => ItemKind::Adjustment,
            _ => ItemKind::Split,
        };
        let unit_price_cents = iv
            .get("unit_price_cents")
            .and_then(|x| x.as_i64())
            .or_else(|| iv.get("unit_price").and_then(|x| x.as_i64()))
            .unwrap_or(0);
        let box_size = iv.get("box_size").and_then(|x| x.as_u64()).map(|x| x as u32);
        let max_quantity = iv.get("max_quantity").and_then(|x| x.as_u64()).map(|x| x as u32);
        let aliases: Vec<String> = iv
            .get("aliases")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let variants: Vec<ItemVariant> = iv
            .get("variants")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|vv| {
                        let variant_id = vv
                            .get("variant_id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let variant_name = vv
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let variant_price = vv
                            .get("unit_price_cents")
                            .and_then(|x| x.as_i64())
                            .or_else(|| vv.get("unit_price").and_then(|x| x.as_i64()))
                            .unwrap_or(0);
                        let capacity = vv.get("capacity").and_then(|x| x.as_u64()).map(|x| x as u32);
                        let variant_aliases: Vec<String> = vv
                            .get("aliases")
                            .and_then(|x| x.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        ItemVariant {
                            variant_id,
                            name: variant_name,
                            unit_price: MoneyCents(variant_price),
                            capacity,
                            aliases: variant_aliases,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        items.push(Item {
            item_id: ItemId(if item_id.is_empty() { format!("item_{}", idx) } else { item_id }),
            round_id: RoundId(round_id.clone()),
            name,
            kind,
            unit_price: MoneyCents(unit_price_cents),
            box_size,
            max_quantity,
            is_blind: false,
            is_proxy_card: false,
            aliases,
            sort_order: idx as i32,
            metadata: Value::Null,
            variants,
        });
    }

    Ok(RoundFixture {
        round_id,
        title,
        group_id,
        items,
        priority_users,
        priority_window,
    })
}

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
        eligibilities.push(make_eligibility(&round_id, u, 10, None, "购物金预存", epoch));
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
            outcomes.push(outcome(rec, "Duplicate", "重复 message_id，已按幂等忽略".to_string()));
            continue;
        }

        if text.starts_with('/') {
            if !rec.is_admin {
                outcomes.push(outcome(rec, "Unsupported", "非管理员发送斜杠命令".to_string()));
                continue;
            }
            if text.starts_with("/加优先") {
                match parse_admin_eligibility(&text, &nickname_map, &fixture.items, &round_id, now) {
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
                    None => outcomes.push(outcome(rec, "Unsupported", "无法解析 /加优先".to_string())),
                }
            } else if text.starts_with("/结团") {
                round_closed = true;
                outcomes.push(outcome(rec, "Applied", "结团".to_string()));
            } else if text.starts_with("/开团") {
                outcomes.push(outcome(rec, "Applied", "开团(模拟中默认已开)".to_string()));
            } else if text.starts_with("/导出") {
                outcomes.push(outcome(rec, "Unsupported", "导出不在模拟范围内".to_string()));
            } else if text.starts_with("/锁位") || text.starts_with("/修正") {
                outcomes.push(outcome(rec, "Unsupported", "管理员锁位/修正暂未进入模拟重放".to_string()));
            } else if text.starts_with("/设置优惠") {
                outcomes.push(outcome(rec, "Unsupported", "优惠规则解析暂未进入模拟重放".to_string()));
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
        if crate::settings::in_priority_window(fixture.priority_window, rec.timestamp_ms)
            && !crate::settings::is_priority_user(
                &fixture.priority_users,
                &[rec.user_id.as_str()],
            )
        {
            outcomes.push(outcome(rec, "Rejected", "优先时段仅限预存(购物金)用户，请求已拒绝".to_string()));
            continue;
        }

        if text.contains("购物金") && !text.contains("非购物金") && !fund_users.contains(&rec.user_id) {
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
            outcomes.push(outcome(rec, "Unsupported", "改单意图未在校验层实现".to_string()));
            continue;
        }

        let user_id = UserId(rec.user_id.clone());
        let validation = validator
            .validate(
                parsed,
                &user_id,
                &rec.group_id,
                Some(rec.message_id.clone()),
                &round_contexts,
                now,
                (idx + 1) as i64,
            )
            .await?;

        match validation {
            ValidationOutcome::Ok(event) => {
                outcomes.push(outcome_from_event(rec, &event));
                events.push(event);
            }
            ValidationOutcome::NeedConfirm(reply) => {
                outcomes.push(outcome(rec, "NeedConfirm", reply.text_content().unwrap_or("").to_string()));
            }
            ValidationOutcome::Reject(reply) => {
                outcomes.push(outcome(rec, "Rejected", reply.text_content().unwrap_or("").to_string()));
            }
            ValidationOutcome::Ignore => {
                outcomes.push(outcome(rec, "Ignored", "无法识别为排谷/撤销意图".to_string()));
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
    let need_confirm_count = outcomes.iter().filter(|o| o.status == "NeedConfirm").count();
    let ignored_count = outcomes.iter().filter(|o| o.status == "Ignored").count();
    let unsupported_count = outcomes.iter().filter(|o| o.status == "Unsupported").count();
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
                .map(|l| match &l.variant_id {
                    Some(variant) => format!("{}@{}x{}[{}]", l.item_id.0, variant, l.quantity, l.slot_policy.as_str()),
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
    let note = if note.is_empty() { "优先权".to_string() } else { note };

    Some(make_eligibility(round_id, &user_id, level, scope, &note, now))
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

fn write_outputs(out_dir: &Path, result: &VerifyResult) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;

    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(out_dir.join("result.json"), json)?;

    let mut jsonl = String::new();
    for o in &result.outcomes {
        jsonl.push_str(&serde_json::to_string(o)?);
        jsonl.push('\n');
    }
    std::fs::write(out_dir.join("outcomes.jsonl"), jsonl)?;

    let report = render_report(result);
    std::fs::write(out_dir.join("report.md"), report)?;

    Ok(())
}

fn render_report(result: &VerifyResult) -> String {
    let mut s = String::new();
    s.push_str(&format!("# 模拟验证报告: {}\n\n", result.round_id));
    s.push_str(&format!("- group_id: {}\n", result.group_id));
    s.push_str(&format!("- 消息总数: {}\n", result.message_count));
    s.push_str(&format!("- 生效(Applied): {}\n", result.applied_count));
    s.push_str(&format!("- 拒绝(Rejected): {}\n", result.rejected_count));
    s.push_str(&format!("- 需确认(NeedConfirm): {}\n", result.need_confirm_count));
    s.push_str(&format!("- 忽略(Ignored): {}\n", result.ignored_count));
    s.push_str(&format!("- 未支持(Unsupported): {}\n", result.unsupported_count));
    s.push_str(&format!("- 重复(Duplicate): {}\n\n", result.duplicate_count));

    s.push_str("## 购物金优先权 (Eligibility)\n\n");
    if result.eligibility.is_empty() {
        s.push_str("(无)\n\n");
    } else {
        for e in &result.eligibility {
            s.push_str(&format!(
                "- user={} level={} note={} valid_from={}\n",
                e.user_id.0,
                e.priority_level,
                e.note.clone().unwrap_or_default(),
                e.valid_from.map(|t| t.to_rfc3339()).unwrap_or_default()
            ));
        }
        s.push('\n');
    }

    s.push_str("## 逐条消息结果\n\n");
    s.push_str("| seq | msg | user | status | text | detail |\n");
    s.push_str("|---|---|---|---|---|---|\n");
    for o in &result.outcomes {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            o.source_sequence,
            o.message_id,
            o.user_id,
            o.status,
            o.text.replace('|', "\\|"),
            o.detail.replace('|', "\\|")
        ));
    }
    s.push('\n');

    s.push_str("## 最终排结果\n\n");
    for ia in &result.final_snapshot.item_allocations {
        let variant_label = ia
            .variant_id
            .as_ref()
            .map(|v| format!(" variant={}", v))
            .unwrap_or_default();
        s.push_str(&format!("### {} ({}, {}){}\n\n", ia.item_name, ia.item_id.0, ia.kind, variant_label));
        let mut boxes = ia.boxes.clone();
        boxes.sort_by_key(|b| b.box_index);
        for b in &boxes {
            let mut slots = b.slots.clone();
            slots.sort_by_key(|s| s.slot_index);
            let cells: Vec<String> = slots
                .iter()
                .map(|s| match s.status {
                    crate::domain::allocation::SlotStatus::Filled => {
                        s.user_id.as_ref().map(|u| u.0.clone()).unwrap_or_else(|| "?".to_string())
                    }
                    crate::domain::allocation::SlotStatus::LockedEmpty => "LOCKED".to_string(),
                    crate::domain::allocation::SlotStatus::AdminReserved => "ADMIN".to_string(),
                    crate::domain::allocation::SlotStatus::Empty => "·".to_string(),
                })
                .collect();
            s.push_str(&format!("- box{}: {}\n", b.box_index, cells.join(" | ")));
        }
        if !ia.singles.is_empty() {
            s.push_str("- 单领: ");
            let singles: Vec<String> = ia.singles.iter().map(|x| format!("{}x{}", x.user_id.0, x.quantity)).collect();
            s.push_str(&singles.join(", "));
            s.push('\n');
        }
        if !ia.waiting.is_empty() {
            s.push_str("- 等待(waiting): ");
            let w: Vec<String> = ia.waiting.iter().map(|x| format!("{}x{}", x.user_id.0, x.quantity)).collect();
            s.push_str(&w.join(", "));
            s.push('\n');
        }
        s.push('\n');
    }

    if let Some(sett) = &result.final_settlement {
        s.push_str("## 结算\n\n");
        for b in &sett.user_bills {
            s.push_str(&format!(
                "- {}: gross={} discount={} gift={} shipping={} final={}\n",
                b.user_id.0,
                b.gross_total.format_yuan(),
                b.discount_share.format_yuan(),
                b.gift_value_share.format_yuan(),
                b.shipping_fee.format_yuan(),
                b.final_total.format_yuan()
            ));
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item() -> Item {
        Item {
            item_id: ItemId("pass_sp".to_string()),
            round_id: RoundId("round_x".to_string()),
            name: "通行认证SP-月行水上".to_string(),
            kind: ItemKind::Split,
            unit_price: MoneyCents(4500),
            box_size: Some(2),
            max_quantity: None,
            is_blind: false,
            is_proxy_card: false,
            aliases: vec!["通行证".to_string()],
            sort_order: 0,
            metadata: serde_json::json!({}),
            variants: vec![],
        }
    }

    fn record(sequence: u64, user: &str, text: &str) -> QueueMessageRecord {
        QueueMessageRecord {
            source_sequence: sequence,
            group_id: "g1".to_string(),
            user_id: user.to_string(),
            nickname: format!("成员{sequence}"),
            message_id: format!("m{sequence}"),
            timestamp_ms: 1_700_000_000_000 + sequence as i64 * 1000,
            text: text.to_string(),
            attachments: vec![],
            reply_to_message_id: None,
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn end_to_end_parse_validate_replay_compares_slots() {
        let path = std::env::temp_dir().join(format!(
            "paigu-verifier-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let records = vec![
            record(1, "u1", "排 通行证 1"),
            record(2, "u2", "排 通行证 1"),
        ];
        let body = records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, body).unwrap();

        let fixture = RoundFixture {
            round_id: "round_x".to_string(),
            title: "测试团".to_string(),
            group_id: "g1".to_string(),
            items: vec![sample_item()],
            priority_users: vec![],
            priority_window: None,
        };

        let result = verify(&path, fixture).await.expect("verify");
        let _ = std::fs::remove_file(&path);

        assert_eq!(result.applied_count, 2);
        assert_eq!(result.rejected_count, 0);

        let allocation = result
            .final_snapshot
            .item_allocations
            .iter()
            .find(|i| i.item_id.0 == "pass_sp")
            .expect("pass_sp allocation");
        assert_eq!(allocation.boxes.len(), 1);
        let slots = &allocation.boxes[0].slots;
        assert_eq!(slots[0].user_id_str(), Some("u1"));
        assert_eq!(slots[1].user_id_str(), Some("u2"));
    }
}
