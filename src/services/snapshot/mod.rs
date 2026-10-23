use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::messages::MessageRecord;
use crate::replay::session::{self, ReplayOverrides, ReplayResult};
use crate::settings::AppConfig;
use crate::snapshot_bundle::SnapshotBundle;

use super::ServiceError;

fn default_snapshot_dir(messages: &crate::messages::MessageLog, cfg: &AppConfig) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    messages.snapshot_dir().join(format!(
        "{}-{}-{}",
        cfg.round.round_id,
        stamp,
        uuid::Uuid::new_v4().simple()
    ))
}

fn build_bundle(
    cfg: &AppConfig,
    records: &[MessageRecord],
    result: &ReplayResult,
    journal: Value,
    draft: Value,
    created_at: String,
) -> anyhow::Result<SnapshotBundle> {
    SnapshotBundle::seal(
        cfg.round.round_id.clone(),
        cfg.revision,
        created_at,
        serde_json::to_value(cfg)?,
        serde_json::to_value(records)?,
        journal,
        serde_json::to_value(&result.board)?,
        json!({"calculated":result.settlement,"draft":draft}),
    )
}

#[derive(Deserialize, Default)]
pub struct ExportBody {
    #[serde(default)]
    pub round_id: Option<String>,
    #[serde(default)]
    pub config_revision: Option<u64>,
    #[serde(default)]
    pub overrides: ReplayOverrides,
    #[serde(default)]
    pub out_dir: Option<String>,
    /// `"file"` = 单一 JSON 快照（C-2）；缺省 `"dir"` = 目录式。
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn snapshot_export(state: &ApiState, body: ExportBody) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    export_locked(&state.messages, &cfg, body).await
}

pub(crate) async fn complete_bundle(
    messages: &crate::messages::MessageLog,
    cfg: &AppConfig,
    overrides: ReplayOverrides,
) -> Result<(SnapshotBundle, ReplayResult), ServiceError> {
    {
        let _fence = messages.ingress_commit_guard().await;
        super::configuration::ensure_no_pending_ingress(messages, &cfg.round.round_id)?;
    }
    completed_bundle(messages, cfg, overrides).await
}

async fn completed_bundle(
    messages: &crate::messages::MessageLog,
    cfg: &AppConfig,
    overrides: ReplayOverrides,
) -> Result<(SnapshotBundle, ReplayResult), ServiceError> {
    let records = messages
        .read_ingress_records(&cfg.round.round_id)
        .await
        .map_err(ServiceError::from)?;
    ensure_completed_records(&records)?;
    let result = session::replay(messages, cfg, overrides)
        .await
        .map_err(ServiceError::from)?;

    let bundle = build_bundle(
        cfg,
        &records,
        &result,
        json!(messages.read_events_strict(&cfg.round.round_id).await?),
        super::settlement::checkpoint(&cfg.round.round_id)?,
        chrono::Utc::now().to_rfc3339(),
    )
    .map_err(ServiceError::from)?;
    Ok((bundle, result))
}

fn ensure_completed_records(records: &[MessageRecord]) -> Result<(), ServiceError> {
    if records.iter().any(|record| record.status == "Pending") {
        return Err(ServiceError::Conflict(
            json!({"error":"pending_saved_message",
            "message":"存在未完成的消息；未导出或应用，请先恢复源消息处理并重新备份"}),
        ));
    }
    Ok(())
}

pub(crate) async fn export_locked(
    messages: &crate::messages::MessageLog,
    cfg: &AppConfig,
    body: ExportBody,
) -> Result<Value, ServiceError> {
    if body
        .round_id
        .as_ref()
        .is_some_and(|round| round != &cfg.round.round_id)
        || body
            .config_revision
            .is_some_and(|revision| revision != cfg.revision)
    {
        return Err(ServiceError::Conflict(
            json!({"error":"snapshot_context_changed","message":"轮次或配置已变化，未保存另一上下文的备份"}),
        ));
    }
    let (bundle, result) = complete_bundle(messages, cfg, body.overrides).await?;
    persist_bundle(
        messages,
        cfg,
        bundle,
        result,
        body.out_dir,
        body.format.as_deref() == Some("file"),
    )
}

/// Caller owns the writer. Preserve the command's pre-receipt completed-action
/// cut; do not wait on, delete, acknowledge or replay the current intake frame.
pub(crate) async fn export_completed_locked(
    messages: &crate::messages::MessageLog,
    cfg: &AppConfig,
) -> Result<Value, ServiceError> {
    let (bundle, result) = completed_bundle(messages, cfg, ReplayOverrides::default()).await?;
    persist_bundle(messages, cfg, bundle, result, None, true)
}

fn persist_bundle(
    messages: &crate::messages::MessageLog,
    cfg: &AppConfig,
    bundle: SnapshotBundle,
    result: ReplayResult,
    out_dir: Option<String>,
    single_file: bool,
) -> Result<Value, ServiceError> {
    let base = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_snapshot_dir(messages, cfg));
    let path = if single_file {
        let target = if base.extension().is_some() {
            base
        } else {
            base.join(format!("{}.snapshot.json", cfg.round.round_id))
        };
        bundle.export_file(&target).map_err(ServiceError::from)?
    } else {
        bundle.export_dir(&base).map_err(ServiceError::from)?
    };
    let manifest = bundle.manifest_typed().map_err(ServiceError::from)?;

    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "manifest": manifest,
        "board": result.board,
        "diff": result.diff,
    }))
}

mod restore;
pub mod transfer;
pub use restore::{snapshot_import, ImportBody};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_config;

    #[test]
    fn default_snapshot_dir_contains_round_id() {
        let cfg = default_config();
        let messages =
            crate::messages::MessageLog::new("placeholder/messages", "placeholder/events");
        let dir = default_snapshot_dir(&messages, &cfg);
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with(&cfg.round.round_id));
        let nonce = name.rsplit('-').next().unwrap();
        assert_eq!(nonce.len(), 32);
        assert!(nonce.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(
            dir,
            default_snapshot_dir(&messages, &cfg),
            "same-second default exports must not overwrite"
        );
    }
}
