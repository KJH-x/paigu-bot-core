use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{ClaimId, EligibilityId, EventId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::replay::{describe_event, rebuild_allocation_snapshot};
use crate::messages::{MessageRecord, MessageStore};
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::replay::state_diff::StateDiff;
use crate::round::{can_cancel, can_claim, phase_at, PhaseWindow, RoundPhase};
use crate::settings::{in_priority_window, is_priority_user, AppConfig, ItemConfig};
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
    store: &dyn MessageStore,
    base: &AppConfig,
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let records = store.read_all().await?;
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

        if let Some(event) = processed.event {
            if processed.priority_claim
                && !eligibilities.iter().any(|e| e.user_id.0 == rec.user_id)
            {
                eligibilities.push(priority_eligibility(&round_id, &rec.user_id));
            }
            events.push(event);
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
    board.generated_at =
        DateTime::<Utc>::from_timestamp_millis(latest_ts).unwrap_or_else(Utc::now);
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
    event: Option<EventEnvelope>,
    priority_claim: bool,
}

fn skipped(status: &str, detail: impl Into<String>) -> Processed {
    Processed {
        status: status.to_string(),
        detail: detail.into(),
        event: None,
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
    let (identity, _) = crate::gateway::onebot::clean_nickname(&rec.nickname);

    if rec.routed.eq_ignore_ascii_case("drop") {
        return skipped("Dropped", "非白名单/Drop");
    }

    if !cfg.gateway.whitelist_members.is_empty()
        && !is_priority_user(
            &cfg.gateway.whitelist_members,
            &[
                rec.user_id.as_str(),
                rec.nickname.as_str(),
                identity.as_str(),
                display.as_str(),
            ],
        )
    {
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

    let rule = RuleParser::parse(text, items, rec.is_admin);

    if rule.intent == ParsedIntent::Modify {
        return skipped("Ignored", "改单功能暂未实现");
    }
    if rule.intent == ParsedIntent::Claim && rule.items.is_empty() && rule.ambiguous_parts.is_empty() {
        return skipped("Ignored", "未解析出商品");
    }

    let now = DateTime::<Utc>::from_timestamp_millis(rec.timestamp_ms).unwrap_or_else(Utc::now);
    let user_id = UserId(rec.user_id.clone());
    let event = match validator
        .validate(
            rule,
            &user_id,
            &rec.group_id,
            Some(rec.message_id.clone()),
            round_contexts,
            now,
            rec.seq,
        )
        .await
    {
        Ok(ValidationOutcome::Ok(mut event)) => {
            stabilize_event(&mut event, rec.seq);
            event
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

    let is_priority = is_priority_user(
        &cfg.round.priority_users,
        &[
            rec.user_id.as_str(),
            rec.nickname.as_str(),
            identity.as_str(),
            display.as_str(),
        ],
    );
    let window = cfg
        .round
        .priority_window
        .as_ref()
        .map(|w| (w.start_ms, w.end_ms));
    if in_priority_window(window, rec.timestamp_ms) && !is_priority {
        return skipped("Rejected", "优先时段仅限预存(购物金)用户");
    }

    if !cfg.round.phases.is_empty() {
        if let Some(phase) = phase_at(&cfg.round.phases, rec.timestamp_ms) {
            if let Some(detail) = phase_rejection(cfg, &event, phase, is_priority) {
                return skipped("Rejected", detail);
            }
        }
    }

    let detail = describe_event(&event);
    let priority_claim = is_priority && matches!(event.payload, DomainEvent::ClaimCreated(_));
    Processed {
        status: "Applied".to_string(),
        detail,
        event: Some(event),
        priority_claim,
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

fn phase_label(phase: RoundPhase) -> &'static str {
    match phase {
        RoundPhase::Phase0 => "Phase 0",
        RoundPhase::PhaseI => "Phase I",
        RoundPhase::PhaseII => "Phase II",
        RoundPhase::PhaseIII => "Phase III",
        RoundPhase::Settling => "结算",
        RoundPhase::Locked => "锁定",
    }
}

fn phase_rejection(
    cfg: &AppConfig,
    event: &EventEnvelope,
    phase: RoundPhase,
    is_priority: bool,
) -> Option<String> {
    let label = phase_label(phase);
    if phase == RoundPhase::Locked {
        return Some(format!("阶段越权：{label}阶段已锁定，禁止操作"));
    }
    match &event.payload {
        DomainEvent::ClaimCreated(c) => c.items.iter().find_map(|line| {
            let class = cfg.round.item_class(&line.item_id.0);
            (!can_claim(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许排该商品"))
        }),
        DomainEvent::ClaimCancelled(c) => c.target_item_id.as_ref().and_then(|item_id| {
            let class = cfg.round.item_class(&item_id.0);
            (!can_cancel(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许撤销该商品"))
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::JsonlMessageStore;
    use crate::settings::{default_config, VariantConfig};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("paigu-session-{tag}-{}", uuid::Uuid::new_v4()))
    }

    fn test_config() -> AppConfig {
        let mut cfg = default_config();
        cfg.llm.enabled = false;
        cfg.round.round_id = "test_round".to_string();
        cfg.round.title = "测试团".to_string();
        cfg.round.priority_users = vec![];
        cfg.round.priority_window = None;
        cfg.round.phases = vec![];
        cfg.round.items = vec![ItemConfig {
            item_id: "badge".to_string(),
            name: "徽章".to_string(),
            kind: "split".to_string(),
            class: Some("B".to_string()),
            unit_price_cents: 0,
            box_size: None,
            max_quantity: None,
            aliases: vec!["徽章".to_string()],
            variants: vec![VariantConfig {
                variant_id: "v_a".to_string(),
                name: "甲".to_string(),
                unit_price_cents: 0,
                pieces: 0,
                capacity: Some(1),
                aliases: vec![],
            }],
        }];
        cfg
    }

    fn record(seq: i64, user_id: &str, text: &str, ts: i64) -> MessageRecord {
        MessageRecord {
            seq,
            group_id: "123456789".to_string(),
            user_id: user_id.to_string(),
            nickname: user_id.to_string(),
            message_id: format!("m{seq}"),
            text: text.to_string(),
            timestamp_ms: ts,
            is_admin: false,
            routed: "message".to_string(),
            status: "Applied".to_string(),
            detail: String::new(),
        }
    }

    fn slot_user(board: &AllocationSnapshot, box_index: u32, slot_index: u32) -> Option<String> {
        board
            .item_allocations
            .iter()
            .find(|ia| ia.item_id.0 == "badge")
            .and_then(|ia| ia.boxes.iter().find(|b| b.box_index == box_index))
            .and_then(|b| b.slots.iter().find(|s| s.slot_index == slot_index))
            .and_then(|s| s.user_id.as_ref().map(|u| u.0.clone()))
    }

    #[tokio::test]
    async fn replay_is_deterministic() {
        let cfg = test_config();
        let records = vec![
            record(1, "u1", "排 徽章 甲 1", 1_000),
            record(2, "u2", "排 徽章 甲 1", 2_000),
        ];
        let first = replay_messages(&cfg, &records, ReplayOverrides::default())
            .await
            .unwrap();
        let second = replay_messages(&cfg, &records, ReplayOverrides::default())
            .await
            .unwrap();

        assert_eq!(
            serde_json::to_value(&first.board).unwrap(),
            serde_json::to_value(&second.board).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&first.outcomes).unwrap(),
            serde_json::to_value(&second.outcomes).unwrap()
        );
        assert_eq!(first.version, second.version);
    }

    #[tokio::test]
    async fn override_priority_changes_board() {
        let cfg = test_config();
        let records = vec![
            record(1, "u1", "排 徽章 甲 1", 1_000),
            record(2, "u2", "排 徽章 甲 1", 2_000),
        ];

        let base = replay_messages(&cfg, &records, ReplayOverrides::default())
            .await
            .unwrap();
        assert_eq!(slot_user(&base.board, 1, 1).as_deref(), Some("u1"));

        let overridden = replay_messages(
            &cfg,
            &records,
            ReplayOverrides {
                priority_users: Some(vec!["u2".to_string()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(slot_user(&overridden.board, 1, 1).as_deref(), Some("u2"));
        assert!(overridden.diff.changed);
        assert!(!overridden.diff.state.slot_changes.is_empty());
    }

    #[tokio::test]
    async fn override_phases_rejects_and_empties_board() {
        let cfg = test_config();
        let records = vec![record(1, "u1", "排 徽章 甲 1", 1_000)];

        let base = replay_messages(&cfg, &records, ReplayOverrides::default())
            .await
            .unwrap();
        assert_eq!(base.outcomes[0].status, "Applied");

        let locked = replay_messages(
            &cfg,
            &records,
            ReplayOverrides {
                phases: Some(vec![PhaseWindow {
                    phase: RoundPhase::Locked,
                    start_ms: 0,
                    end_ms: i64::MAX,
                }]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(locked.outcomes[0].status, "Rejected");
        assert!(locked.board.user_summaries.is_empty());
        assert!(locked.diff.changed);
    }

    #[tokio::test]
    async fn override_whitelist_drops_outsider() {
        let cfg = test_config();
        let records = vec![
            record(1, "u1", "排 徽章 甲 1", 1_000),
            record(2, "u2", "排 徽章 甲 1", 2_000),
        ];

        let filtered = replay_messages(
            &cfg,
            &records,
            ReplayOverrides {
                whitelist_members: Some(vec!["u1".to_string()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(filtered.outcomes[0].status, "Applied");
        assert_eq!(filtered.outcomes[1].status, "Dropped");
        assert!(filtered.board.user_summaries.iter().all(|s| s.user_id.0 == "u1"));
    }

    #[tokio::test]
    async fn replay_reads_store_and_supports_edit_recompute() {
        let cfg = test_config();
        let dir = temp_dir("store");
        let store = JsonlMessageStore::new(&dir, &cfg.round.round_id);
        store.append(&record(1, "u1", "排 徽章 甲 1", 1_000)).await.unwrap();

        let before = replay(&store, &cfg, ReplayOverrides::default()).await.unwrap();
        assert_eq!(before.version, 1);
        assert_eq!(before.board.user_summaries.len(), 1);

        store
            .replace_all(&[record(1, "u1", "今天天气不错", 1_000)])
            .await
            .unwrap();
        let after = replay(&store, &cfg, ReplayOverrides::default()).await.unwrap();
        assert_eq!(after.outcomes[0].status, "Ignored");
        assert!(after.board.user_summaries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
