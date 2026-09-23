use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::snapshot::AllocationSnapshot;
use crate::messages::{MessageRecord, MessageStore};
use crate::replay::session::{self, ReplayDiff, ReplayOverrides, ReplayResult};
use crate::settings::AppConfig;
use crate::snapshot_bundle::SnapshotBundle;

use super::{api_bad_request, api_internal, api_stale_revision, ApiError, ApiState};

fn last_replay() -> &'static Mutex<Option<ReplayResult>> {
    static LAST: OnceLock<Mutex<Option<ReplayResult>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/replay", post(run_replay))
        .route("/api/replay/diff", get(replay_diff))
        .route("/api/events", get(list_raw_events))
        .route("/api/snapshot/export", post(snapshot_export))
        .route("/api/snapshot/import", post(snapshot_import))
}

pub(super) fn replay_result_json(result: &ReplayResult) -> Value {
    json!({
        "round_id": result.round_id,
        "revision": result.revision,
        "version": result.version,
        "messages": result.messages,
        "board": result.board,
        "outcomes": result.outcomes,
        "diff": result.diff,
        "settlement": result.settlement,
        "event_count": result.events.len(),
    })
}

async fn do_replay(state: &ApiState, overrides: ReplayOverrides) -> anyhow::Result<ReplayResult> {
    let cfg = state.cfg.get().await;
    let store = state.messages.store_for(&cfg.round.round_id);
    let result = session::replay(&store, &cfg, overrides).await?;
    if let Ok(mut slot) = last_replay().lock() {
        *slot = Some(result.clone());
    }
    Ok(result)
}

#[derive(Deserialize, Default)]
struct ReplayBody {
    #[serde(default)]
    overrides: ReplayOverrides,
}

async fn run_replay(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ReplayBody>,
) -> Result<Json<Value>, ApiError> {
    let result = do_replay(&state, body.overrides)
        .await
        .map_err(api_internal)?;
    Ok(Json(replay_result_json(&result)))
}

async fn replay_diff(State(state): State<Arc<ApiState>>) -> Result<Json<Value>, ApiError> {
    let stored = last_replay().lock().ok().and_then(|guard| guard.clone());
    let result = match stored {
        Some(result) => result,
        None => do_replay(&state, ReplayOverrides::default())
            .await
            .map_err(api_internal)?,
    };

    let (live_version, live_board) = state.pipeline.board().await;
    let live_snapshot: Option<AllocationSnapshot> = serde_json::from_value(live_board).ok();
    let live_diff: Option<ReplayDiff> =
        live_snapshot.map(|board| ReplayDiff::from_boards(&board, &result.board));

    Ok(Json(json!({
        "version": result.version,
        "base_version": result.diff.base_version,
        "diff": result.diff,
        "live_version": live_version,
        "live_diff": live_diff,
    })))
}

/// 原始事件日志（C-3）：保留的所有入站成员原始事件 JSON。
async fn list_raw_events(State(state): State<Arc<ApiState>>) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    let events = state
        .messages
        .read_raw_events(&cfg.round.round_id)
        .await
        .map_err(api_internal)?;
    Ok(Json(json!({
        "round_id": cfg.round.round_id,
        "count": events.len(),
        "events": events,
    })))
}

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
struct ExportBody {
    #[serde(default)]
    overrides: ReplayOverrides,
    #[serde(default)]
    out_dir: Option<String>,
    /// `"file"` = 单一 JSON 快照（C-2）；缺省 `"dir"` = 目录式。
    #[serde(default)]
    format: Option<String>,
}

async fn snapshot_export(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ExportBody>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    let store = state.messages.store_for(&cfg.round.round_id);
    let records = store.read_all().await.map_err(api_internal)?;
    let result = session::replay_messages(&cfg, &records, body.overrides)
        .await
        .map_err(api_internal)?;

    let bundle = build_bundle(&cfg, &records, &result, chrono::Utc::now().to_rfc3339())
        .map_err(api_internal)?;
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
        bundle.export_file(&target).map_err(api_internal)?
    } else {
        bundle.export_dir(&base).map_err(api_internal)?
    };
    let manifest = bundle.manifest_typed().map_err(api_internal)?;

    Ok(Json(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "manifest": manifest,
        "board": result.board,
        "diff": result.diff,
    })))
}

#[derive(Deserialize, Default)]
struct ImportBody {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    bundle: Option<SnapshotBundle>,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    revision: Option<u64>,
}

async fn snapshot_import(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(api_stale_revision(cfg.revision));
        }
    }

    let bundle = match (body.path, body.bundle) {
        (Some(path), _) => {
            let p = PathBuf::from(&path);
            if p.is_file() {
                SnapshotBundle::import_file(&p).map_err(api_bad_request)?
            } else {
                SnapshotBundle::import_dir(&p).map_err(api_bad_request)?
            }
        }
        (None, Some(bundle)) => {
            bundle.verify().map_err(api_bad_request)?;
            bundle
        }
        (None, None) => return Err(api_bad_request("需要 path 或 bundle")),
    };
    let manifest = bundle.verify().map_err(api_bad_request)?;

    let mut applied = false;
    let mut result = None;
    if body.apply {
        let records: Vec<MessageRecord> =
            serde_json::from_value(bundle.messages.clone()).map_err(api_bad_request)?;
        let imported_cfg: AppConfig =
            serde_json::from_value(bundle.config.clone()).map_err(api_bad_request)?;
        let store = state.messages.store_for(&cfg.round.round_id);
        store.replace_all(&records).await.map_err(api_internal)?;
        let replayed =
            session::replay_messages(&imported_cfg, &records, ReplayOverrides::default())
                .await
                .map_err(api_internal)?;
        applied = true;
        result = Some(replay_result_json(&replayed));
    }

    Ok(Json(json!({
        "ok": true,
        "verified": true,
        "manifest": manifest,
        "applied": applied,
        "result": result,
    })))
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
