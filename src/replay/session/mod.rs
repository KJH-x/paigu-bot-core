use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{ClaimCancelled, DomainEvent, EventEnvelope, EventStatus};
use crate::domain::ids::{ClaimId, EligibilityId, EventId, ItemId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::replay::{describe_event, rebuild_allocation_snapshot};
use crate::messages::{MessageLog, MessageRecord};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::policy;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidateContext, ValidationOutcome};
use crate::replay::state_diff::StateDiff;
use crate::round::PhaseWindow;
use crate::settings::{AppConfig, ItemConfig};
use crate::settlement::SettlementConfig;

/// 重放离线解析使用的规则置信度阈值（与实时校验层一致）。
const REPLAY_CONFIDENCE_THRESHOLD: f32 = 0.65;

/// 重放时可覆盖的附加条件；`None` 表示沿用 base 配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayOverrides {
    #[serde(default)]
    pub priority_users: Option<Vec<String>>,
    #[serde(default)]
    pub phases: Option<Vec<PhaseWindow>>,
    #[serde(default)]
    pub items: Option<Vec<ItemConfig>>,
    #[serde(default)]
    pub whitelist_members: Option<Vec<String>>,
    #[serde(default)]
    pub settlement: Option<SettlementConfig>,
}

impl ReplayOverrides {
    pub fn is_empty(&self) -> bool {
        self.priority_users.is_none()
            && self.phases.is_none()
            && self.items.is_none()
            && self.whitelist_members.is_none()
            && self.settlement.is_none()
    }
}

/// 把 overrides 合并进一份配置副本（不修改 base）。
pub fn apply_overrides(base: &AppConfig, overrides: &ReplayOverrides) -> AppConfig {
    let mut cfg = base.clone();
    if let Some(v) = &overrides.priority_users {
        cfg.round.priority_users = v.clone();
    }
    if let Some(v) = &overrides.phases {
        cfg.round.phases = v.clone();
    }
    if let Some(v) = &overrides.items {
        cfg.round.items = v.clone();
    }
    if let Some(v) = &overrides.whitelist_members {
        cfg.gateway.whitelist_members = v.clone();
    }
    cfg
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMessageView {
    pub seq: i64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOutcome {
    pub seq: i64,
    pub status: String,
    pub detail: String,
    pub applied: bool,
}

/// 基线重放（base 配置）与覆盖重放（overrides）之间的差异。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDiff {
    pub base_version: i64,
    pub replay_version: i64,
    pub changed: bool,
    pub state: StateDiff,
}

impl ReplayDiff {
    pub fn from_boards(before: &AllocationSnapshot, after: &AllocationSnapshot) -> Self {
        let state = StateDiff::from_snapshots(before, after);
        let changed = before.version != after.version
            || !state.slot_changes.is_empty()
            || !state.claim_changes.is_empty()
            || !state.user_total_changes.is_empty()
            || !state.item_total_changes.is_empty()
            || !state.settlement_changes.is_empty();
        Self {
            base_version: before.version,
            replay_version: after.version,
            changed,
            state,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub round_id: String,
    pub revision: u64,
    pub version: i64,
    pub messages: Vec<ReplayMessageView>,
    pub board: AllocationSnapshot,
    pub outcomes: Vec<ReplayOutcome>,
    pub events: Vec<EventEnvelope>,
    pub diff: ReplayDiff,
    #[serde(default)]
    pub settlement: Option<SettlementConfig>,
}

/// 从持久化消息日志重放（C2）：复用与实时同一条规则解析器 + 校验层 + 分配引擎。
pub async fn replay(
    store: &MessageLog,
    base: &AppConfig,
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let records = store.read_all(&base.round.round_id).await?;
    replay_messages(base, &records, overrides).await
}

/// 从内存消息列表重放（快照导入 / 单测复用）。
pub async fn replay_messages(
    base: &AppConfig,
    records: &[MessageRecord],
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let replay_cfg = apply_overrides(base, &overrides);

    let (baseline_board, computed) = if overrides.is_empty() {
        let computed = compute(&replay_cfg, records).await;
        (computed.board.clone(), computed)
    } else {
        let baseline = compute(base, records).await;
        let computed = compute(&replay_cfg, records).await;
        (baseline.board, computed)
    };

    let diff = ReplayDiff::from_boards(&baseline_board, &computed.board);

    Ok(ReplayResult {
        round_id: replay_cfg.round.round_id.clone(),
        revision: base.revision,
        version: computed.version,
        messages: computed.messages,
        board: computed.board,
        outcomes: computed.outcomes,
        events: computed.events,
        diff,
        settlement: overrides.settlement,
    })
}

struct Computation {
    messages: Vec<ReplayMessageView>,
    outcomes: Vec<ReplayOutcome>,
    events: Vec<EventEnvelope>,
    board: AllocationSnapshot,
    version: i64,
}

async fn compute(cfg: &AppConfig, records: &[MessageRecord]) -> Computation {
    let items = cfg.round.to_items();
    let round_id = RoundId(cfg.round.round_id.clone());
    let round_contexts = vec![RoundContext {
        round_id: round_id.clone(),
        title: cfg.round.title.clone(),
        items: items.clone(),
    }];
    let validator = EventValidator::new(REPLAY_CONFIDENCE_THRESHOLD);

    let mut ordered: Vec<&MessageRecord> = records.iter().collect();
    ordered.sort_by_key(|r| (r.timestamp_ms, r.seq));

    let mut events: Vec<EventEnvelope> = Vec::new();
    let mut eligibilities: Vec<Eligibility> = Vec::new();
    let mut messages = Vec::with_capacity(ordered.len());
    let mut outcomes = Vec::with_capacity(ordered.len());
    let mut latest_ts: i64 = 0;

    for rec in ordered {
        latest_ts = latest_ts.max(rec.timestamp_ms);
        let processed = process_one(cfg, rec, &items, &round_contexts, &validator).await;

        if !processed.events.is_empty() {
            if processed.priority_claim && !eligibilities.iter().any(|e| e.user_id.0 == rec.user_id)
            {
                eligibilities.push(priority_eligibility(&round_id, &rec.user_id));
            }
            events.extend(processed.events);
        }

        messages.push(ReplayMessageView {
            seq: rec.seq,
            group_id: rec.group_id.clone(),
            user_id: rec.user_id.clone(),
            nickname: rec.nickname.clone(),
            text: rec.text.clone(),
            timestamp_ms: rec.timestamp_ms,
            is_admin: rec.is_admin,
            status: processed.status.clone(),
            detail: processed.detail.clone(),
        });
        outcomes.push(ReplayOutcome {
            seq: rec.seq,
            applied: processed.status == "Applied",
            status: processed.status,
            detail: processed.detail,
        });
    }

    let mut board = rebuild_allocation_snapshot(&items, &events, &eligibilities);
    board.generated_at = DateTime::<Utc>::from_timestamp_millis(latest_ts).unwrap_or_else(Utc::now);
    let version = board.version;

    Computation {
        messages,
        outcomes,
        events,
        board,
        version,
    }
}

struct Processed {
    status: String,
    detail: String,
    events: Vec<EventEnvelope>,
    priority_claim: bool,
}

fn skipped(status: &str, detail: impl Into<String>) -> Processed {
    Processed {
        status: status.to_string(),
        detail: detail.into(),
        events: Vec::new(),
        priority_claim: false,
    }
}

async fn process_one(
    cfg: &AppConfig,
    rec: &MessageRecord,
    items: &[Item],
    round_contexts: &[RoundContext],
    validator: &EventValidator,
) -> Processed {
    let display = if rec.nickname.trim().is_empty() {
        rec.user_id.clone()
    } else {
        rec.nickname.clone()
    };
    let (identity, _) = crate::parser::normalize::clean_nickname(&rec.nickname);

    if rec.routed.eq_ignore_ascii_case("drop") {
        return skipped("Dropped", "非白名单/Drop");
    }

    if !policy::member_allowed(
        &cfg.gateway.whitelist_members,
        &[
            rec.user_id.as_str(),
            rec.nickname.as_str(),
            identity.as_str(),
            display.as_str(),
        ],
    ) {
        return skipped("Dropped", "成员不在白名单");
    }

    let text = rec.text.trim();
    if text.is_empty() {
        return skipped("Ignored", "空消息");
    }

    if text.starts_with('/') {
        return if rec.is_admin {
            skipped("Applied", "管理员命令已记录")
        } else {
            skipped("Rejected", "非管理员斜杠命令")
        };
    }

    let mut rule = RuleParser::parse(text, items, rec.is_admin);
    // 改单（D-2）：与实时管线一致，按新数量重排，应用时先撤销本人该商品的既有认购。
    let is_modify = rule.intent == ParsedIntent::Modify;
    if is_modify {
        rule.intent = ParsedIntent::Claim;
    }
    if rule.intent == ParsedIntent::Claim
        && rule.items.is_empty()
        && rule.ambiguous_parts.is_empty()
    {
        return skipped("Ignored", "未解析出商品");
    }

    let now = DateTime::<Utc>::from_timestamp_millis(rec.timestamp_ms).unwrap_or_else(Utc::now);
    let user_id = UserId(rec.user_id.clone());
    let event = match validator
        .validate(
            rule,
            ValidateContext {
                user_id: &user_id,
                group_id: &rec.group_id,
                raw_message_id: Some(rec.message_id.clone()),
                active_rounds: round_contexts,
                now,
                sequence: rec.seq,
            },
        )
        .await
    {
        Ok(ValidationOutcome::Ok(mut event)) => {
            stabilize_event(&mut event, rec.seq);
            *event
        }
        Ok(ValidationOutcome::NeedConfirm(reply)) => {
            let detail = reply
                .text_content()
                .map(str::to_string)
                .unwrap_or_else(|| "需要确认".to_string());
            return skipped("NeedConfirm", detail);
        }
        Ok(ValidationOutcome::Reject(reply)) => {
            let detail = reply
                .text_content()
                .map(str::to_string)
                .unwrap_or_else(|| "校验拒绝".to_string());
            return skipped("Rejected", detail);
        }
        Ok(ValidationOutcome::Ignore) => {
            return skipped("Ignored", "无法识别为排谷/撤销意图");
        }
        Err(e) => return skipped("Error", format!("处理失败: {e}")),
    };

    let is_priority = policy::is_priority(
        cfg,
        &[
            rec.user_id.as_str(),
            rec.nickname.as_str(),
            identity.as_str(),
            display.as_str(),
        ],
    );
    if policy::in_priority_at(cfg, rec.timestamp_ms) && !is_priority {
        return skipped("Rejected", "优先时段仅限预存(购物金)用户");
    }

    if !cfg.round.phases.is_empty() {
        if let Some(phase) = policy::phase_at(&cfg.round.phases, rec.timestamp_ms) {
            if let Some(detail) = policy::phase_rejection(cfg, &event, phase, is_priority) {
                return skipped("Rejected", detail);
            }
        }
    }

    let mut detail = describe_event(&event);
    if is_modify {
        detail = format!("改单：{detail}");
    }
    let priority_claim = is_priority && matches!(event.payload, DomainEvent::ClaimCreated(_));
    let round_id = RoundId(cfg.round.round_id.clone());
    let mut events: Vec<EventEnvelope> = Vec::new();
    if is_modify {
        if let DomainEvent::ClaimCreated(created) = &event.payload {
            let targets: Vec<ItemId> = created.items.iter().map(|l| l.item_id.clone()).collect();
            for (index, item_id) in targets.into_iter().enumerate() {
                let mut cancel = cancel_for_modify(&round_id, rec, rec.seq, now, item_id);
                cancel.event_id = EventId(format!("evt-{}-cancel-{index}", rec.seq));
                events.push(cancel);
            }
        }
    }
    events.push(event);
    Processed {
        status: "Applied".to_string(),
        detail,
        events,
        priority_claim,
    }
}

/// 改单撤销事件（与实时 `Pipeline` 的 `cancel_for_modify` 语义一致）。
fn cancel_for_modify(
    round_id: &RoundId,
    rec: &MessageRecord,
    seq: i64,
    now: DateTime<Utc>,
    item_id: ItemId,
) -> EventEnvelope {
    EventEnvelope {
        event_id: EventId(format!("evt-{seq}-cancel")),
        round_id: round_id.clone(),
        group_id: rec.group_id.clone(),
        user_id: UserId(rec.user_id.clone()),
        raw_message_id: Some(rec.message_id.clone()),
        event_type: "claim_cancelled".to_string(),
        effective_at: now,
        sequence: seq,
        payload: DomainEvent::ClaimCancelled(ClaimCancelled {
            target_claim_id: None,
            target_item_id: Some(item_id),
            quantity: None,
            reason: Some("改单".to_string()),
            parse_trace: None,
            validation_trace: vec![],
        }),
        status: EventStatus::Active,
    }
}

/// 重放时用日志序号派生稳定的 event/claim 标识，保证「同日志同结果」。
fn stabilize_event(event: &mut EventEnvelope, seq: i64) {
    event.event_id = EventId(format!("evt-{seq}"));
    if let DomainEvent::ClaimCreated(claim) = &mut event.payload {
        claim.claim_id = ClaimId(format!("claim-{seq}"));
    }
}

fn priority_eligibility(round_id: &RoundId, user_id: &str) -> Eligibility {
    Eligibility {
        eligibility_id: EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        user_id: UserId(user_id.to_string()),
        priority_type: "shopping_fund".to_string(),
        priority_level: 10,
        scope: EligibilityScope {
            item_ids: None,
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: Some(DateTime::<Utc>::from_timestamp_millis(0).unwrap_or_else(Utc::now)),
        valid_until: None,
        note: Some("预存(购物金)用户".to_string()),
    }
}

#[cfg(test)]
mod tests;
