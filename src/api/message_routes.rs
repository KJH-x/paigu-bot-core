use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::services::messages::{self, CreateInput, UpdateInput};
use crate::services::ServiceError;

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/messages", get(list_messages).post(create_message))
        .route(
            "/api/messages/:seq",
            put(update_message).delete(delete_message),
        )
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_messages(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::list_messages(&state, query.since.unwrap_or(0), query.limit)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn create_message(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<CreateInput>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::create_message(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn update_message(
    State(state): State<Arc<ApiState>>,
    Path(seq): Path<i64>,
    Json(body): Json<UpdateInput>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::update_message(&state, seq, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    recompute: Option<bool>,
}

async fn delete_message(
    State(state): State<Arc<ApiState>>,
    Path(seq): Path<i64>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::delete_message(&state, seq, query.recompute)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}
