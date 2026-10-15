use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::services::members;
use crate::services::ServiceError;

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/members", get(get_members))
        .route("/api/members/refresh", post(refresh_members))
        .route("/api/members/cn-suggest", post(suggest_cn))
}

async fn get_members(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(members::get_members(&state).await)
}

async fn suggest_cn(State(state): State<Arc<ApiState>>) -> Result<Json<Value>, ApiError> {
    let value = members::suggest_cn(&state)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

/// 拉取群成员并写入缓存（每日调度 / 刷新路由复用）。只读动作，绝不发消息。
pub async fn refresh_members_from_gateway(state: &ApiState) -> anyhow::Result<Value> {
    members::refresh_members_from_gateway(state).await
}

async fn refresh_members(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match refresh_members_from_gateway(&state).await {
        Ok(data) => Ok(Json(json!({ "members": data, "source": "gateway" }))),
        Err(error) => Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": error.to_string() })),
        )),
    }
}
