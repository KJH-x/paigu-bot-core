use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Work {
    pub evaluated_groups: usize,
    pub reused_groups: usize,
}

#[derive(Clone, Default)]
pub(crate) struct Memo {
    pub(super) context: Option<String>,
    pub(super) groups: Vec<GroupProjection>,
    pub work: Work,
}

#[derive(Clone)]
pub(super) struct GroupProjection {
    pub key: String,
    pub events: Vec<EventEnvelope>,
    pub eligibility: Vec<Eligibility>,
    pub messages: Vec<ReplayMessageView>,
    pub outcomes: Vec<ReplayOutcome>,
}

pub(super) fn group_key(members: &[&MessageRecord], overrides: &ParseOverrides) -> Option<String> {
    let choices: Vec<_> = members
        .iter()
        .map(|r| overrides.input_for(&r.record_id, &r.message_id))
        .collect();
    serde_json::to_vec(&(members, choices))
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

/// Same evaluator as full replay. Only an identical prefix can be reused.
pub(crate) async fn replay_incremental(
    cfg: &AppConfig,
    records: &[MessageRecord],
    journal: &[serde_json::Value],
    previous: &Memo,
) -> anyhow::Result<(ReplayResult, Memo)> {
    let (overrides, phase) = journal_context(journal)?;
    let context = crate::replay::computed::incremental_context(cfg, phase).ok();
    let (computed, memo) = compute_with_memo(
        cfg,
        records,
        &overrides,
        phase,
        &cfg.round.to_items(),
        &[],
        context.map(|context| (previous, context)),
    )
    .await;
    let baseline = computed.board.clone();
    Ok((computed.into_result(cfg, &baseline, None), memo))
}
