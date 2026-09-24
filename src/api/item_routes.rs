use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use crate::services::aliases::{self, SuggestAliasesBody};

use super::ApiState;

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new().route("/api/items/suggest-aliases", post(suggest_aliases))
}

/// §U6 别名建议：成功与降级均返回 HTTP 200（前端据 `degraded`/`verdict` 处理）。
async fn suggest_aliases(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<SuggestAliasesBody>,
) -> Json<Value> {
    Json(aliases::suggest_aliases(&state, body).await)
}
