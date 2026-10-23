use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

use super::{ApiError, ApiState};
use crate::services::{
    snapshot::transfer::{self, DownloadBody, ImportFileQuery},
    ServiceError,
};
use crate::snapshot_bundle::SnapshotFile;

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/snapshot/download", post(download))
        .route(
            "/api/snapshot/import-file",
            post(import_file).layer(DefaultBodyLimit::max(transfer::MAX_FILE_BYTES)),
        )
        .layer(middleware::from_fn(private_transport))
}

pub(crate) fn allowed_origin(value: &str) -> bool {
    let Ok(origin) = reqwest::Url::parse(value) else {
        return false;
    };
    value == value.trim()
        && origin.scheme() == "http"
        && matches!(origin.host_str(), Some("127.0.0.1" | "localhost"))
        && origin.username().is_empty()
        && origin.password().is_none()
        && origin.path() == "/"
        && origin.query().is_none()
        && origin.fragment().is_none()
}

async fn private_transport(request: Request, next: Next) -> Response {
    let origins: Vec<_> = request.headers().get_all(header::ORIGIN).iter().collect();
    let allowed =
        origins.is_empty() || (origins.len() == 1 && origins[0].to_str().is_ok_and(allowed_origin));
    let mut response = if allowed {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, Json(json!({"error":"private_snapshot_origin","message":"私有备份仅允许本机HTTP页面或本机命令行，不接受外站或文件页面请求"}))).into_response()
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store, private"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}

async fn download(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<DownloadBody>,
) -> Result<Response, ApiError> {
    let output = transfer::download(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", output.filename),
        )
        .body(Body::from(output.bytes))
        .map_err(super::api_internal)
}

async fn import_file(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<ImportFileQuery>,
    Json(file): Json<SnapshotFile>,
) -> Result<Json<serde_json::Value>, ApiError> {
    transfer::import_file(&state, query, file)
        .await
        .map(Json)
        .map_err(ServiceError::into_api)
}
