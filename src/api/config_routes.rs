use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::settings::{AppConfig, ConfigError};

use super::ApiState;

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/config/reload", post(reload_config))
}

async fn get_config(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let config = state.cfg.get().await;
    let revision = config.revision;
    Json(json!({ "config": config, "revision": revision }))
}

#[derive(Deserialize)]
struct ConfigBody {
    config: AppConfig,
    revision: u64,
}

async fn put_config(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ConfigBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.cfg.put(body.config, body.revision).await {
        Ok(revision) => {
            let config = state.cfg.get().await;
            Ok(Json(json!({ "config": config, "revision": revision })))
        }
        Err(error) => Err(config_error_response(&error)),
    }
}

async fn reload_config(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.cfg.reload().await {
        Ok(revision) => {
            let config = state.cfg.get().await;
            Ok(Json(json!({ "config": config, "revision": revision })))
        }
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )),
    }
}

fn config_error_response(error: &ConfigError) -> (StatusCode, Json<Value>) {
    match error {
        ConfigError::StaleRevision { actual, .. } => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "stale_revision", "revision": actual })),
        ),
        other => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": other.to_string() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_revision_maps_to_409() {
        let error = ConfigError::StaleRevision {
            expected: 1,
            actual: 5,
        };
        let (status, Json(body)) = config_error_response(&error);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "stale_revision");
        assert_eq!(body["revision"], 5);
    }

    #[test]
    fn other_error_maps_to_500() {
        let error = ConfigError::Io(std::io::Error::new(std::io::ErrorKind::Other, "boom"));
        let (status, Json(body)) = config_error_response(&error);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["error"].as_str().unwrap().contains("boom"));
    }
}
