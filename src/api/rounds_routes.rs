use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::services::rounds::{self, ActivateBody, CreateRoundBody};
use crate::services::ServiceError;

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/rounds", get(list_rounds).post(create_round))
        .route("/api/rounds/:id/activate", post(activate_round))
        .route("/api/rounds/:id/check", post(check_round))
        .route("/api/rounds/:id", delete(delete_round))
}

async fn list_rounds(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(rounds::list_rounds(&state).await)
}

async fn create_round(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<CreateRoundBody>,
) -> Result<Json<Value>, ApiError> {
    rounds::create_round(&state, body)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}

async fn activate_round(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
    body: Option<Json<ActivateBody>>,
) -> Result<Json<Value>, ApiError> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    rounds::activate_round(&state, &id, body)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}

async fn check_round(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    rounds::check_round(&state, &id)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}

async fn delete_round(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    rounds::delete_round(&state, &id)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}
