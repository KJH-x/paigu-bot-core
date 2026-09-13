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
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn get_board(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let (version, board) = state.pipeline.board().await;
    let status = state.gateway.status().await;
    Json(json!({ "version": version, "board": board, "status": status }))
}

async fn gateway_status(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(state.gateway.status().await)
}
