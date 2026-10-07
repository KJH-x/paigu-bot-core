use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::services::messages::{self, CreateInput, RerouteInput, UpdateInput};
use crate::services::ServiceError;

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/messages", get(list_messages).post(create_message))
        .route(
            "/api/messages/:key",
            put(update_message).delete(delete_message),
        )
        .route("/api/messages/:key/reroute", post(reroute_message))
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
    Path(key): Path<String>,
    Json(body): Json<UpdateInput>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::update_message(&state, &key, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    record_revision: Option<u64>,
    #[serde(default)]
    recompute: Option<bool>,
}

async fn delete_message(
    State(state): State<Arc<ApiState>>,
    Path(key): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::delete_message(&state, &key, query.record_revision, query.recompute)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn reroute_message(
    State(state): State<Arc<ApiState>>,
    Path(key): Path<String>,
    Json(body): Json<RerouteInput>,
) -> Result<Json<Value>, ApiError> {
    let value = messages::reroute_message(&state, &key, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}
