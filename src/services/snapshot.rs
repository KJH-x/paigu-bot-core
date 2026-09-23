use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::messages::MessageRecord;
use crate::replay::session::{self, ReplayOverrides, ReplayResult};
use crate::settings::AppConfig;
use crate::snapshot_bundle::SnapshotBundle;

use super::replay::replay_result_json;
use super::ServiceError;

fn default_snapshot_dir(cfg: &AppConfig) -> PathBuf {
    let base = std::env::var("PAIGU_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/snapshots"));
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    base.join(format!("{}-{}", cfg.round.round_id, stamp))
}

fn build_bundle(
    cfg: &AppConfig,
    records: &[MessageRecord],
    result: &ReplayResult,
    created_at: String,
) -> anyhow::Result<SnapshotBundle> {
    SnapshotBundle::seal(
        cfg.round.round_id.clone(),
        cfg.revision,
        created_at,
        serde_json::to_value(cfg)?,
        serde_json::to_value(records)?,
        serde_json::to_value(&result.events)?,
        serde_json::to_value(&result.board)?,
        serde_json::to_value(&result.settlement)?,
    )
}

#[derive(Deserialize, Default)]
pub struct ExportBody {
    #[serde(default)]
    pub overrides: ReplayOverrides,
    #[serde(default)]
    pub out_dir: Option<String>,
    /// `"file"` = 单一 JSON 快照（C-2）；缺省 `"dir"` = 目录式。
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn snapshot_export(state: &ApiState, body: ExportBody) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let records = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .map_err(ServiceError::from)?;
    let result = session::replay_messages(&cfg, &records, body.overrides)
        .await
        .map_err(ServiceError::from)?;

    let bundle = build_bundle(&cfg, &records, &result, chrono::Utc::now().to_rfc3339())
        .map_err(ServiceError::from)?;
    let base = body
        .out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_snapshot_dir(&cfg));
    let single_file = body.format.as_deref() == Some("file");
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

#[derive(Deserialize, Default)]
pub struct ImportBody {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub bundle: Option<SnapshotBundle>,
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub revision: Option<u64>,
}

pub async fn snapshot_import(state: &ApiState, body: ImportBody) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(ServiceError::StaleRevision(cfg.revision));
        }
    }

    let bundle = match (body.path, body.bundle) {
        (Some(path), _) => {
            let p = PathBuf::from(&path);
            if p.is_file() {
                SnapshotBundle::import_file(&p)
                    .map_err(|e| ServiceError::BadRequest(e.to_string()))?
            } else {
                SnapshotBundle::import_dir(&p)
                    .map_err(|e| ServiceError::BadRequest(e.to_string()))?
            }
        }
        (None, Some(bundle)) => {
            bundle
                .verify()
                .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
            bundle
        }
        (None, None) => return Err(ServiceError::BadRequest("需要 path 或 bundle".to_string())),
    };
    let manifest = bundle
        .verify()
        .map_err(|e| ServiceError::BadRequest(e.to_string()))?;

    let mut applied = false;
    let mut result = None;
    if body.apply {
        let records: Vec<MessageRecord> = serde_json::from_value(bundle.messages.clone())
            .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
        let imported_cfg: AppConfig = serde_json::from_value(bundle.config.clone())
            .map_err(|e| ServiceError::BadRequest(e.to_string()))?;
        state
            .messages
            .replace_all(&cfg.round.round_id, &records)
            .await
            .map_err(ServiceError::from)?;
        let replayed =
            session::replay_messages(&imported_cfg, &records, ReplayOverrides::default())
                .await
                .map_err(ServiceError::from)?;
        applied = true;
        result = Some(replay_result_json(&replayed));
    }

    Ok(json!({
        "ok": true,
        "verified": true,
        "manifest": manifest,
        "applied": applied,
        "result": result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_config;

    #[test]
    fn default_snapshot_dir_contains_round_id() {
        let cfg = default_config();
        let dir = default_snapshot_dir(&cfg);
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with(&cfg.round.round_id));
    }
}
