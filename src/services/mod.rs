//! 服务层：从 API 路由 handler 下沉的纯逻辑与编排。
//!
//! 路由只负责解析请求、调用本层、把结果映射为 `Json<Value>` / `ApiError`；
//! 对外 HTTP 契约（路径、方法、请求体字段、响应体字段、状态码）保持不变。

pub mod display;
pub mod members;
pub mod messages;
pub mod replay;
pub mod settlement;
pub mod snapshot;

/// 工作流只读快照（Stepper 用）。
pub mod workflow;

use crate::api::{api_bad_request, api_internal, api_not_found, api_stale_revision, ApiError};

/// 服务层错误：由路由映射为与重构前一致的 HTTP 错误响应。
#[derive(Debug)]
pub enum ServiceError {
    BadRequest(String),
    NotFound(String),
    /// 乐观并发：`revision` 不一致 → 409 `stale_revision`。
    StaleRevision(u64),
    Internal(anyhow::Error),
}

impl ServiceError {
    pub fn into_api(self) -> ApiError {
        match self {
            ServiceError::BadRequest(message) => api_bad_request(message),
            ServiceError::NotFound(message) => api_not_found(message),
            ServiceError::StaleRevision(revision) => api_stale_revision(revision),
            ServiceError::Internal(error) => api_internal(error),
        }
    }
}

impl From<anyhow::Error> for ServiceError {
    fn from(error: anyhow::Error) -> Self {
        ServiceError::Internal(error)
    }
}
