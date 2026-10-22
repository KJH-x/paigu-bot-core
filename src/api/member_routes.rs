use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

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

async fn suggest_cn(
    State(state): State<Arc<ApiState>>,
    body: Result<Json<members::MemberContext>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(context) =
        body.map_err(|_| super::api_bad_request("成员请求须提供轮次、配置版本和名单指纹"))?;
    let value = members::suggest_cn(&state, context)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn refresh_members(
    State(state): State<Arc<ApiState>>,
    body: Result<Json<members::MemberContext>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(context) =
        body.map_err(|_| super::api_bad_request("成员请求须提供轮次、配置版本和名单指纹"))?;
    members::refresh_members(&state, context)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}
