use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::services::replay::{self, ReplayBody};
use crate::services::snapshot::{self, ExportBody, ImportBody};
use crate::services::ServiceError;

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/replay", post(run_replay))
        .route("/api/replay/diff", get(replay_diff))
        .route("/api/events", get(list_raw_events))
        .route("/api/snapshot/export", post(snapshot_export))
        .route("/api/snapshot/import", post(snapshot_import))
}

async fn run_replay(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ReplayBody>,
) -> Result<Json<Value>, ApiError> {
    let value = replay::run_replay(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn replay_diff(State(state): State<Arc<ApiState>>) -> Result<Json<Value>, ApiError> {
    let value = replay::replay_diff(&state)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn list_raw_events(
    State(state): State<Arc<ApiState>>,
    axum::extract::Query(query): axum::extract::Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    // 默认只回最后 1000 条；显式 limit=0 才取全部（事件日志可达数十 MB）
    let limit = query.limit.unwrap_or(1000).min(100_000);
    let value = replay::list_raw_events(&state, limit)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

#[derive(serde::Deserialize, Default)]
struct EventsQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn snapshot_export(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ExportBody>,
) -> Result<Json<Value>, ApiError> {
    let value = snapshot::snapshot_export(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn snapshot_import(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    let value = snapshot::snapshot_import(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}
