//! 服务层：从 API 路由 handler 下沉的纯逻辑与编排。
//!
//! 路由只负责解析请求、调用本层、把结果映射为 `Json<Value>` / `ApiError`；
//! 对外 HTTP 契约（路径、方法、请求体字段、响应体字段、状态码）保持不变。

pub mod aliases;
pub mod configuration;
pub mod display;
pub mod member_pull;
pub mod members;
pub mod message_actions;
pub mod messages;
pub mod replay;
pub mod rounds;
pub mod settlement;
pub mod snapshot;

/// 工作流只读快照（Stepper 用）。
pub mod workflow;

use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

use crate::api::{api_bad_request, api_internal, api_not_found, api_stale_revision, ApiError};

/// 服务层错误：由路由映射为与重构前一致的 HTTP 错误响应。
#[derive(Debug)]
pub enum ServiceError {
    BadRequest(String),
    PayloadTooLarge(String),
    NotFound(String),
    /// 乐观并发：`revision` 不一致 → 409 `stale_revision`。
    StaleRevision(u64),
    Conflict(Value),
    Validation(Value),
    Unprocessable(String),
    Locked(String),
    Upstream(String),
    Internal(anyhow::Error),
}

impl ServiceError {
    pub fn into_api(self) -> ApiError {
        match self {
            ServiceError::BadRequest(message) => api_bad_request(message),
            ServiceError::PayloadTooLarge(message) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"error":message})),
            ),
            ServiceError::NotFound(message) => api_not_found(message),
            ServiceError::StaleRevision(revision) => api_stale_revision(revision),
            ServiceError::Conflict(value) => (StatusCode::CONFLICT, Json(value)),
            ServiceError::Validation(value) => (StatusCode::UNPROCESSABLE_ENTITY, Json(value)),
            ServiceError::Unprocessable(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "error": message })),
            ),
            ServiceError::Locked(message) => (
                StatusCode::LOCKED,
                Json(json!({ "error": "locked", "message": message })),
            ),
            ServiceError::Internal(error) => api_internal(error),
            ServiceError::Upstream(message) => {
                (StatusCode::BAD_GATEWAY, Json(json!({"error":message})))
            }
        }
    }
}

impl From<anyhow::Error> for ServiceError {
    fn from(error: anyhow::Error) -> Self {
        ServiceError::Internal(error)
    }
}
