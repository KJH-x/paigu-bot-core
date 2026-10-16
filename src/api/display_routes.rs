use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::services::display;

use super::ApiState;

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new().route("/api/display", get(display_board))
}

#[derive(Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    epoch: Option<String>,
    #[serde(default)]
    view_revision: Option<u64>,
}

async fn display_board(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<SinceQuery>,
) -> Json<Value> {
    Json(
        display::display_with_cursor(
            &state,
            query.since.unwrap_or(0),
            query.epoch.as_deref(),
            query.view_revision,
        )
        .await,
    )
}
