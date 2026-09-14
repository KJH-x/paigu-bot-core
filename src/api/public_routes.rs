//! DEPRECATED: 旧栈（Postgres/R2/旧 WS），仅 `legacy` 路径使用，勿新增依赖。
#![allow(dead_code)]

use axum::{Router, routing::get, extract::{Path, State}, Json};
use std::sync::Arc;
use crate::app_state::AppState;
use serde_json::Value;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/rounds/{id}/current", get(get_current_snapshot))
        .with_state(state)
}

async fn get_current_snapshot(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let round_id = crate::domain::ids::RoundId(id);
    match state.services.snapshot.get_latest(&round_id).await {
        Ok(Some(snapshot)) => Json(serde_json::json!(snapshot)),
        Ok(None) => Json(serde_json::json!({ "round_id": round_id.0, "version": 0, "items": [] })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
