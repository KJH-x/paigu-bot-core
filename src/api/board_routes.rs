use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use super::ApiState;

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/board", get(get_board))
        .route("/api/gateway/status", get(gateway_status))
        .route("/api/workflow", get(workflow))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// 工作流只读快照：阶段/锁定/网关/配置进度（供前端 Stepper，加性接口）。
async fn workflow(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(crate::services::workflow::snapshot(&state).await)
}

async fn get_board(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let (version, board) = state.pipeline.board().await;
    let status = state.gateway.status().await;
    Json(json!({ "version": version, "board": board, "status": status }))
}

async fn gateway_status(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(state.gateway.status().await)
}
