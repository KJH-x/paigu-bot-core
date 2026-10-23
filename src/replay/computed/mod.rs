//! Disposable private projection. Source validation always precedes a cache read.
use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::session::{self, ReplayOverrides, ReplayResult};
use crate::{
    domain::claim::Eligibility,
    messages::{MessageLog, MessageRecord},
    settings::AppConfig,
};

const FORMAT: u32 = 1;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const BUILD: &str = env!("PAIGU_COMPUTE_BUILD");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadPolicy {
    Prefer,
    Force,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    result: ReplayResult,
    eligibilities: Vec<Eligibility>,
    display_names: BTreeMap<String, String>,
}

impl Payload {
    fn new(result: &ReplayResult) -> Self {
        Self {
            result: result.clone(),
            eligibilities: result.eligibilities.clone(),
            display_names: result
                .display_names
                .iter()
                .map(|(id, name)| (id.0.clone(), name.clone()))
                .collect(),
        }
    }
    fn into_result(mut self, revision: u64) -> ReplayResult {
        self.result.eligibilities = self.eligibilities;
        self.result.display_names = self
            .display_names
            .into_iter()
            .map(|(id, name)| (crate::domain::ids::UserId(id), name))
            .collect();
        self.result.revision = revision;
        self.result
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    format: u32,
    build: String,
    context: String,
    integrity: String,
    payload: Payload,
}

fn digest(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn member_source(cfg: &AppConfig) -> anyhow::Result<String> {
    if cfg.members.cache_path.trim().is_empty() || cfg.members.group_id != cfg.round.group_id {
        return Ok("unused".into());
    }
    match std::fs::read(&cfg.members.cache_path) {
        Ok(bytes) => Ok(format!("{:x}", Sha256::digest(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("missing".into()),
        // A broken optional name source must not turn cached old names into current names.
        Err(_) => anyhow::bail!("成员来源不可校验，未使用派生缓存"),
    }
}

fn context(
    cfg: &AppConfig,
    records: &[MessageRecord],
    journal: &[serde_json::Value],
) -> anyhow::Result<String> {
    context_at(cfg, records, journal, chrono::Utc::now().timestamp_millis())
}

fn context_at(
    cfg: &AppConfig,
    records: &[MessageRecord],
    journal: &[serde_json::Value],
    now_ms: i64,
) -> anyhow::Result<String> {
    let mut semantics = cfg.clone();
    semantics.revision = 0;
    semantics.active_round_id = None;
    digest(&(
        FORMAT,
        BUILD,
        semantics,
        records,
        journal,
        member_source(cfg)?,
        crate::parser::policy::phase_at(&cfg.round.phases, now_ms),
    ))
}

fn load(
    path: &std::path::Path,
    key: &str,
    cfg: &AppConfig,
    records: &[MessageRecord],
) -> Option<ReplayResult> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let saved: File = serde_json::from_slice(&bytes).ok()?;
    let result = &saved.payload.result;
    let mut ids: Vec<_> = records.iter().map(|r| (&r.record_id, r.seq)).collect();
    let mut projected: Vec<_> = result
        .messages
        .iter()
        .map(|r| (&r.record_id, r.seq))
        .collect();
    ids.sort();
    projected.sort();
    if saved.format != FORMAT
        || saved.build != BUILD
        || saved.context != key
        || saved.integrity != digest(&saved.payload).ok()?
        || result.round_id != cfg.round.round_id
        || result.board.round_id.0 != cfg.round.round_id
        || result.version != result.board.version
        || ids != projected
        || result
            .events
            .iter()
            .any(|e| e.round_id.0 != cfg.round.round_id)
    {
        return None;
    }
    Some(saved.payload.into_result(cfg.revision))
}

pub(crate) fn invalidate(store: &MessageLog, round: &str) {
    if let Ok(path) = store.computed_path(round) {
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("派生缓存未清理；后续恢复仍须严格校验完整来源");
            }
        }
    }
}

fn save(path: &std::path::Path, key: &str, result: &ReplayResult) -> anyhow::Result<()> {
    let payload = Payload::new(result);
    let file = File {
        format: FORMAT,
        build: BUILD.into(),
        context: key.into(),
        integrity: digest(&payload)?,
        payload,
    };
    let bytes = serde_json::to_vec(&file)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_BYTES, "派生文件超限");
    crate::persistence::atomic_write(path, &bytes)?;
    Ok(())
}

/// Only committed default-policy computations may be published here, under the writer.
pub(crate) async fn checkpoint(
    store: &MessageLog,
    cfg: &AppConfig,
    result: &ReplayResult,
    expected_records: &[MessageRecord],
) {
    let write = async {
        let records = store.read_ingress_records(&cfg.round.round_id).await?;
        anyhow::ensure!(
            serde_json::to_value(&records)? == serde_json::to_value(expected_records)?
                && session::resolve_display_names(cfg, &records) == result.display_names,
            "派生来源在准备后发生变化"
        );
        anyhow::ensure!(
            records.iter().all(|r| r.status != "Pending"),
            "尚有未完成记录"
        );
        anyhow::ensure!(
            result.round_id == cfg.round.round_id
                && result.revision == cfg.revision
                && result.messages.len() == records.len(),
            "派生切面不匹配"
        );
        let journal = store.read_events_strict(&cfg.round.round_id).await?;
        let key = context(cfg, &records, &journal)?;
        save(&store.computed_path(&cfg.round.round_id)?, &key, result)
    }
    .await;
    if write.is_err() {
        tracing::warn!("派生缓存未写入；已提交操作有效，下次按真源重算");
    }
}

/// Caller holds the shared writer. Cache failure cannot roll back committed work.
pub(crate) async fn replay(
    store: &MessageLog,
    cfg: &AppConfig,
    policy: ReadPolicy,
) -> anyhow::Result<(ReplayResult, bool)> {
    let path = store.computed_path(&cfg.round.round_id)?;
    let records = store.read_ingress_records(&cfg.round.round_id).await?;
    let journal = store.read_events_strict(&cfg.round.round_id).await?;
    // Do this before load too: valid JSON with a damaged domain row is not a cache miss.
    for value in journal.iter().filter(|v| v.get("event_type").is_some()) {
        serde_json::from_value::<crate::domain::event::EventEnvelope>(value.clone())?;
    }
    let complete = records.iter().all(|r| r.status != "Pending");
    let key = context(cfg, &records, &journal).ok();
    if policy == ReadPolicy::Prefer && complete {
        if let Some(result) = key.as_ref().and_then(|key| load(&path, key, cfg, &records)) {
            if context(cfg, &records, &journal).ok() == key {
                return Ok((result, true));
            }
        }
    }
    let result =
        session::replay_from_journal(cfg, &records, &journal, ReplayOverrides::default()).await?;
    if complete {
        if let Some(key) =
            key.filter(|key| context(cfg, &records, &journal).ok().as_ref() == Some(key))
        {
            if save(&path, &key, &result).is_err() {
                tracing::warn!("派生缓存未写入；权威结果有效，下次按真源重算");
            }
        }
    }
    Ok((result, false))
}

#[cfg(test)]
mod tests;
