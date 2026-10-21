use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{ClaimCancelled, DomainEvent, EventEnvelope, EventStatus};
use crate::domain::ids::{ClaimId, EligibilityId, EventId, ItemId, RoundId, UserId};
use crate::domain::item::Item;
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::replay::rebuild_allocation_snapshot;
use crate::messages::{MessageLog, MessageRecord};
use crate::parser::coalesce::{self, Candidate};
use crate::parser::parsed_event::{apply_parse_override, collect_parse_overrides, ParseOverrides};
use crate::parser::policy;
use crate::parser::rule_parser::RuleParser;
use crate::replay::state_diff::StateDiff;
use crate::round::{PhaseWindow, RoundPhase};
use crate::settings::{AppConfig, ItemConfig};
use crate::settlement::SettlementConfig;

mod understanding;
pub(crate) use understanding::{
    evaluate_understanding, prepare_understanding, prepare_with_catalog, Evaluation,
};

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
    pub record_id: String,
    #[serde(default = "default_record_revision")]
    pub record_revision: u64,
    pub seq: i64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    pub status: String,
    pub detail: String,
    #[serde(default)]
    pub original_automatic: Option<crate::messages::processing::AutomaticSummary>,
    #[serde(default)]
    pub manual_note: Option<String>,
}

fn default_record_revision() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOutcome {
    pub record_id: String,
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
    #[serde(skip)]
    pub(crate) eligibilities: Vec<Eligibility>,
    #[serde(skip)]
    pub(crate) display_names: HashMap<UserId, String>,
    pub diff: ReplayDiff,
    #[serde(default)]
    pub settlement: Option<SettlementConfig>,
    /// 轮次是否锁定（生效阶段 ∈ {Settling, Locked}，含手动 `RoundPhaseSet` 覆盖）。
    #[serde(default)]
    pub locked: bool,
}

/// 从事件日志折叠手动阶段覆盖（`RoundPhaseSet`，按 `(effective_at, sequence)` last-wins）。
pub(crate) async fn load_manual_phase(store: &MessageLog, round_id: &str) -> Option<RoundPhase> {
    let raw = match store.read_raw_events(round_id, 0).await {
        Ok(raw) => raw,
        Err(_) => return None,
    };
    let events: Vec<EventEnvelope> = raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();
    events
        .into_iter()
        .filter(|e| matches!(e.payload, DomainEvent::RoundPhaseSet(_)))
        .max_by(|a, b| (a.effective_at, a.sequence).cmp(&(b.effective_at, b.sequence)))
        .and_then(|e| match e.payload {
            DomainEvent::RoundPhaseSet(set) => set.phase,
            _ => None,
        })
}

/// 重放已存自动/人工动作并重验政策；无自动产物的旧记录单独兼容解析。
pub async fn replay(
    store: &MessageLog,
    base: &AppConfig,
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let records = store.read_only(&base.round.round_id).await?;
    let events = store.domain_events(&base.round.round_id).await?;
    let parse_overrides = collect_parse_overrides(&events);
    let manual_phase = events
        .iter()
        .filter(|e| matches!(e.payload, DomainEvent::RoundPhaseSet(_)))
        .max_by_key(|e| (e.effective_at, e.sequence))
        .and_then(|e| match &e.payload {
            DomainEvent::RoundPhaseSet(set) => set.phase,
            _ => None,
        });
    replay_messages_with_parse(base, &records, overrides, &parse_overrides, manual_phase).await
}

/// 从内存消息列表重放（快照导入 / 单测复用）；不消费 `ParseOverride` 事件。
#[cfg(test)]
pub async fn replay_messages(
    base: &AppConfig,
    records: &[MessageRecord],
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    replay_messages_with_parse(base, records, overrides, &ParseOverrides::default(), None).await
}

/// §U7：读取轮次事件日志中的 `ParseOverride`，建立 `message_id → 修正解析` 映射。
pub(crate) async fn load_parse_overrides(store: &MessageLog, round_id: &str) -> ParseOverrides {
    let raw = match store.read_raw_events(round_id, 0).await {
        Ok(raw) => raw,
        Err(_) => return ParseOverrides::default(),
    };
    let events: Vec<EventEnvelope> = raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();
    collect_parse_overrides(&events)
}

/// 从内存消息列表重放，并消费给定的 `ParseOverride` 映射。
pub(crate) async fn replay_messages_with_parse(
    base: &AppConfig,
    records: &[MessageRecord],
    overrides: ReplayOverrides,
    parse_overrides: &ParseOverrides,
    manual_phase: Option<RoundPhase>,
) -> anyhow::Result<ReplayResult> {
    let replay_cfg = apply_overrides(base, &overrides);

    let (baseline_board, computed) = if overrides.is_empty() {
        let computed = compute(
            &replay_cfg,
            records,
            parse_overrides,
            manual_phase,
            &replay_cfg.round.to_items(),
            &[],
        )
        .await;
        (computed.board.clone(), computed)
    } else {
        let baseline = compute(
            base,
            records,
            parse_overrides,
            manual_phase,
            &base.round.to_items(),
            &[],
        )
        .await;
        let computed = compute(
            &replay_cfg,
            records,
            parse_overrides,
            manual_phase,
            &replay_cfg.round.to_items(),
            &[],
        )
        .await;
        (baseline.board, computed)
    };

    Ok(computed.into_result(&replay_cfg, &baseline_board, overrides.settlement))
}

/// Offline callers supply their catalog/eligibility explicitly; no stores, model or gateway.
pub(crate) async fn replay_prepared(
    cfg: &AppConfig,
    records: &[MessageRecord],
    items: &[Item],
    eligibility: &[Eligibility],
    overrides: &ParseOverrides,
) -> anyhow::Result<ReplayResult> {
    anyhow::ensure!(
        !cfg.llm.enabled && !cfg.gateway.reply_enabled && cfg.members.cache_path.is_empty(),
        "离线会话必须关闭模型/回复及成员磁盘来源"
    );
    anyhow::ensure!(
        records.iter().all(|r| r.processing.is_some()),
        "离线会话必须显式准备全部消息理解"
    );
    let catalog_ids: std::collections::BTreeSet<_> =
        items.iter().map(|item| item.item_id.0.as_str()).collect();
    let config_ids: std::collections::BTreeSet<_> = cfg
        .round
        .items
        .iter()
        .map(|item| item.item_id.as_str())
        .collect();
    anyhow::ensure!(
        catalog_ids == config_ids
            && catalog_ids.len() == items.len()
            && config_ids.len() == cfg.round.items.len()
            && items
                .iter()
                .all(|item| item.round_id.0 == cfg.round.round_id)
            && eligibility
                .iter()
                .all(|grant| grant.round_id.0 == cfg.round.round_id),
        "离线目录/优先权与轮次不一致"
    );
    let record_ids: std::collections::BTreeSet<_> = records
        .iter()
        .map(|record| record.record_id.as_str())
        .collect();
    anyhow::ensure!(
        record_ids.len() == records.len() && !record_ids.contains(""),
        "离线记录身份缺失或重复"
    );
    let computed = compute(cfg, records, overrides, None, items, eligibility).await;
    let baseline = computed.board.clone();
    Ok(computed.into_result(cfg, &baseline, None))
}

struct Computation {
    messages: Vec<ReplayMessageView>,
    outcomes: Vec<ReplayOutcome>,
    events: Vec<EventEnvelope>,
    eligibilities: Vec<Eligibility>,
    display_names: HashMap<UserId, String>,
    board: AllocationSnapshot,
    version: i64,
    locked: bool,
}

impl Computation {
    fn into_result(
        self,
        cfg: &AppConfig,
        baseline: &AllocationSnapshot,
        settlement: Option<SettlementConfig>,
    ) -> ReplayResult {
        let diff = ReplayDiff::from_boards(baseline, &self.board);
        ReplayResult {
            round_id: cfg.round.round_id.clone(),
            revision: cfg.revision,
            version: self.version,
            messages: self.messages,
            board: self.board,
            outcomes: self.outcomes,
            events: self.events,
            eligibilities: self.eligibilities,
            display_names: self.display_names,
            diff,
            settlement,
            locked: self.locked,
        }
    }
}

async fn compute(
    cfg: &AppConfig,
    records: &[MessageRecord],
    parse_overrides: &ParseOverrides,
    manual_phase: Option<RoundPhase>,
    items: &[Item],
    initial_eligibility: &[Eligibility],
) -> Computation {
    let round_id = RoundId(cfg.round.round_id.clone());

    let display_names = resolve_display_names(cfg, records);

    let mut ordered: Vec<&MessageRecord> = records.iter().collect();
    ordered.sort_by_key(|r| (r.timestamp_ms, r.seq));

    let groups = message_groups(cfg, &ordered, parse_overrides);

    let mut events: Vec<EventEnvelope> = Vec::new();
    let mut eligibilities = initial_eligibility.to_vec();
    let mut messages = Vec::with_capacity(ordered.len());
    let mut outcomes = Vec::with_capacity(ordered.len());
    let mut latest_ts: i64 = 0;

    for group in &groups {
        // 代表 = 组内 `(timestamp_ms, seq)` 最大者。
        let rep_pos = group
            .iter()
            .copied()
            .max_by_key(|&i| (ordered[i].timestamp_ms, ordered[i].seq))
            .unwrap_or(group[0]);
        latest_ts = latest_ts.max(
            group
                .iter()
                .map(|&i| ordered[i].timestamp_ms)
                .max()
                .unwrap_or(0),
        );

        let members: Vec<_> = group.iter().map(|&index| ordered[index]).collect();
        let automatic = ordered[rep_pos]
            .processing
            .as_ref()
            .and_then(|history| history.latest_for(&members));
        let mut representative = (*ordered[rep_pos]).clone();
        if group.len() > 1 {
            representative.text = members
                .iter()
                .map(|record| record.text.trim())
                .collect::<Vec<_>>()
                .join("\n");
        }
        let mut processed = process_one(
            cfg,
            &representative,
            parse_overrides,
            manual_phase,
            automatic,
        )
        .await;
        processed.explain_cancellations(&events, cfg);

        let rep_uid = ordered[rep_pos].user_id.clone();
        if !processed.events.is_empty() {
            if processed.priority_claim
                && !eligibilities.iter().any(|e| {
                    e.user_id.0 == rep_uid
                        && e.priority_level >= 10
                        && e.scope.item_ids.is_none()
                        && e.valid_until.is_none()
                        && e.max_uses.is_none()
                        && e.valid_from.is_none_or(|at| at.timestamp_millis() <= 0)
                })
            {
                eligibilities.push(priority_eligibility(&round_id, &rep_uid));
            }
            events.extend(processed.events.clone());
        }

        for &i in group {
            let rec = ordered[i];
            let is_rep = i == rep_pos;
            let (status, detail) = if is_rep {
                (
                    processed.outcome.status.clone(),
                    processed.outcome.detail.clone(),
                )
            } else {
                (
                    "Merged".to_string(),
                    format!("并入 #{}", ordered[rep_pos].seq),
                )
            };
            messages.push(ReplayMessageView {
                original_automatic: crate::messages::processing::original_summary(rec),
                manual_note: parse_overrides
                    .reason_for_record(&rec.record_id)
                    .map(str::to_string),
                record_id: rec.record_id.clone(),
                record_revision: parse_overrides
                    .record_revision(&rec.record_id, rec.record_revision),
                seq: rec.seq,
                group_id: rec.group_id.clone(),
                user_id: rec.user_id.clone(),
                nickname: display_names
                    .get(&UserId(rec.user_id.clone()))
                    .cloned()
                    .unwrap_or_else(|| rec.nickname.clone()),
                text: rec.text.clone(),
                timestamp_ms: rec.timestamp_ms,
                is_admin: rec.is_admin,
                status: status.clone(),
                detail: detail.clone(),
            });
            outcomes.push(ReplayOutcome {
                record_id: rec.record_id.clone(),
                seq: rec.seq,
                applied: is_rep && status == "Applied",
                status,
                detail,
            });
        }
    }

    let mut board = rebuild_allocation_snapshot(items, &events, &eligibilities, &display_names);
    board.round_id = round_id;
    board.generated_at = DateTime::<Utc>::from_timestamp_millis(latest_ts).unwrap_or_else(Utc::now);
    let version = board.version;

    let locked = matches!(
        manual_phase.or_else(|| policy::phase_at(&cfg.round.phases, Utc::now().timestamp_millis())),
        Some(RoundPhase::Settling) | Some(RoundPhase::Locked)
    );

    Computation {
        messages,
        outcomes,
        events,
        eligibilities,
        display_names,
        board,
        version,
        locked,
    }
}

pub(crate) fn resolve_display_names(
    cfg: &AppConfig,
    records: &[MessageRecord],
) -> HashMap<UserId, String> {
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|record| (record.timestamp_ms, record.seq));
    let mut names = HashMap::new();
    for record in ordered {
        let duplicate = record
            .processing
            .as_ref()
            .and_then(|h| h.results.first())
            .is_some_and(|r| {
                matches!(
                    r.understanding.source,
                    crate::messages::processing::UnderstandingSource::Duplicate
                )
            });
        if duplicate
            || record.routed.to_ascii_lowercase().starts_with("drop")
            || !policy::group_allowed(&cfg.gateway.whitelist_groups, &record.group_id)
        {
            continue;
        }
        if let Some(name) = crate::settings::resolve_cn(cfg, &record.user_id, &record.nickname) {
            names.insert(UserId(record.user_id.clone()), name);
        }
    }
    names
}

async fn process_one(
    cfg: &AppConfig,
    rec: &MessageRecord,
    parse_overrides: &ParseOverrides,
    manual_phase: Option<RoundPhase>,
    automatic: Option<&crate::messages::processing::AutomaticResult>,
) -> Evaluation {
    use crate::messages::processing::{Understanding, UnderstandingSource};
    if parse_overrides.invalid_selection(&rec.record_id) {
        return Evaluation::skipped(
            "NeedConfirm",
            "人工修订格式损坏或版本未知，未回退执行旧动作；请核对",
        );
    }
    if let Some(saved) = parse_overrides.selection_for_record(&rec.record_id) {
        use crate::parser::action_override::ActionSelection;
        if matches!(saved.selection, ActionSelection::Automatic { .. }) {
            return automatic.map_or_else(
                || {
                    Evaluation::skipped(
                        "NeedConfirm",
                        "已选恢复自动，但原自动基线缺失或不匹配，需核对",
                    )
                },
                |auto| {
                    evaluate_understanding(
                        cfg,
                        rec,
                        &auto.understanding,
                        auto.manual_phase_at_ingest,
                    )
                },
            );
        }
        let mut understood =
            Understanding::terminal(UnderstandingSource::Control, "Ignored", "人工忽略", None);
        understood.events = saved.events.clone();
        understood.validation = match &saved.selection {
            ActionSelection::Reject { .. } => {
                crate::messages::processing::ProcessingOutcome::new("Rejected", "人工拒绝", None)
            }
            ActionSelection::Replace { .. } if !saved.events.is_empty() => {
                crate::messages::processing::ProcessingOutcome::new(
                    "Applied",
                    saved
                        .events
                        .iter()
                        .map(|e| crate::parser::action_override::describe_event(e, cfg))
                        .collect::<Vec<_>>()
                        .join("；"),
                    None,
                )
            }
            ActionSelection::Replace { .. } => crate::messages::processing::ProcessingOutcome::new(
                "Ignored",
                "人工修订：无动作",
                None,
            ),
            _ => understood.validation,
        };
        return evaluate_understanding(
            cfg,
            rec,
            &understood,
            automatic.map_or(manual_phase, |auto| auto.manual_phase_at_ingest),
        );
    }
    if let Some(parsed) = parse_overrides.for_record(&rec.record_id) {
        let prepared =
            prepare_understanding(cfg, rec, parsed.clone(), UnderstandingSource::Control).await;
        return evaluate_understanding(
            cfg,
            rec,
            &prepared,
            automatic.map_or(manual_phase, |saved| saved.manual_phase_at_ingest),
        );
    }
    if let Some(saved) = automatic {
        return evaluate_understanding(
            cfg,
            rec,
            &saved.understanding,
            saved.manual_phase_at_ingest,
        );
    }
    if rec.processing.is_some() {
        return Evaluation::skipped(
            "NeedConfirm",
            "已存自动理解与当前消息/分组不匹配，需人工核对；未重新解析",
        );
    }
    // Historical logs without an automatic artifact remain a visibly separate compatibility path.
    let text = rec.text.trim();
    let understood = if text.is_empty() || text.starts_with('/') {
        Understanding::terminal(
            UnderstandingSource::Control,
            if text.is_empty() {
                "Ignored"
            } else if rec.is_admin {
                "Applied"
            } else {
                "Rejected"
            },
            if text.is_empty() {
                "空消息"
            } else if rec.is_admin {
                "管理员命令已记录"
            } else {
                "非管理员斜杠命令"
            },
            None,
        )
    } else {
        let rule = RuleParser::parse(text, &cfg.round.to_items(), rec.is_admin);
        let parsed =
            apply_parse_override(rule, &rec.record_id, Some(&rec.message_id), parse_overrides);
        prepare_understanding(cfg, rec, parsed, UnderstandingSource::Rules).await
    };
    evaluate_understanding(cfg, rec, &understood, manual_phase)
}

pub(crate) fn message_groups(
    cfg: &AppConfig,
    ordered: &[&MessageRecord],
    overrides: &ParseOverrides,
) -> Vec<Vec<usize>> {
    let positions: HashMap<_, _> = ordered
        .iter()
        .enumerate()
        .map(|(i, r)| (r.record_id.as_str(), i))
        .collect();
    let mut units = vec![];
    for (owner, record) in ordered.iter().enumerate() {
        let Some(history) = &record.processing else {
            continue;
        };
        if history.unreadable.is_some()
            || history
                .results
                .last()
                .is_some_and(|r| r.format_version != 1)
        {
            continue;
        }
        for (index, result) in history.results.iter().enumerate() {
            if result.format_version != 1 {
                continue;
            }
            let Some(mut members) = result
                .members
                .iter()
                .map(|m| positions.get(m.record_id.as_str()).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if members.is_empty() {
                continue;
            }
            members.sort_unstable();
            members.dedup();
            if members.last() != Some(&owner)
                || members
                    .iter()
                    .any(|&i| overrides.has_record(&ordered[i].record_id))
            {
                continue;
            }
            let refs: Vec<_> = members.iter().map(|&i| ordered[i]).collect();
            if result.matches(&refs) {
                units.push((
                    refs.iter().map(|r| r.seq).max().unwrap_or(0),
                    index,
                    result.created_at_ms,
                    members,
                ));
            }
        }
    }
    units.sort_by(|a, b| (b.0, b.1, b.2).cmp(&(a.0, a.1, a.2)));
    let mut assigned = vec![false; ordered.len()];
    let mut groups = vec![];
    for (_, _, _, members) in units {
        if members.iter().any(|&i| assigned[i]) {
            continue;
        }
        for &i in &members {
            assigned[i] = true;
        }
        groups.push(members);
    }
    // Legacy records alone retain rule-based grouping; persisted units never get regrouped by aliases.
    let legacy_candidates: Vec<_> = ordered
        .iter()
        .map(|r| Candidate {
            user_id: &r.user_id,
            text: &r.text,
            is_admin: r.is_admin,
            force_boundary: r.processing.is_some()
                || overrides.has_record(&r.record_id)
                || !r.routed.eq_ignore_ascii_case("message"),
        })
        .collect();
    let legacy_groups = if cfg.merge.enabled && cfg.merge.same_sender {
        coalesce::group_candidates(&legacy_candidates, &cfg.round.to_items())
    } else {
        (0..ordered.len()).map(|i| vec![i]).collect()
    };
    for group in legacy_groups {
        let legacy: Vec<_> = group
            .into_iter()
            .filter(|&i| {
                !assigned[i]
                    && ordered[i].processing.is_none()
                    && !overrides.has_record(&ordered[i].record_id)
            })
            .collect();
        if !legacy.is_empty() {
            for &i in &legacy {
                assigned[i] = true;
            }
            groups.push(legacy);
        }
    }
    for (i, used) in assigned.iter().enumerate() {
        if !used {
            groups.push(vec![i]);
        }
    }
    groups.sort_by_key(|g| *g.last().unwrap_or(&0));
    groups
}

pub(crate) fn prospective_groups(
    cfg: &AppConfig,
    ordered: &[&MessageRecord],
    overrides: &ParseOverrides,
) -> Vec<Vec<usize>> {
    if !(cfg.merge.enabled && cfg.merge.same_sender) {
        return (0..ordered.len()).map(|i| vec![i]).collect();
    }
    let candidates: Vec<_> = ordered
        .iter()
        .map(|record| Candidate {
            user_id: &record.user_id,
            text: &record.text,
            is_admin: record.is_admin,
            force_boundary: !record.routed.eq_ignore_ascii_case("message")
                || overrides.has_record(&record.record_id)
                || record.processing.as_ref().is_some_and(|p| {
                    p.results.first().is_some_and(|r| {
                        matches!(
                            r.understanding.source,
                            crate::messages::processing::UnderstandingSource::Duplicate
                        ) || matches!(
                            r.manual_phase_at_ingest,
                            Some(RoundPhase::Settling | RoundPhase::Locked)
                        )
                    })
                }),
        })
        .collect();
    coalesce::group_candidates(&candidates, &cfg.round.to_items())
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
