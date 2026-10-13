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
        .route("/api/config/reply", post(set_reply))
        .route("/api/config/admin-commands", post(set_admin_commands))
}

#[derive(Deserialize)]
struct ToggleBody {
    enabled: bool,
    #[serde(default)]
    revision: Option<u64>,
}

/// 运行时热切换 `gateway.reply_enabled`（B-1：默认关闭，需管理员显式开启）。
async fn set_reply(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ToggleBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    toggle_gateway(state, body, |gw, v| gw.reply_enabled = v, "reply_enabled").await
}

/// 运行时热切换 `gateway.admin_commands_enabled`（D-1）。
async fn set_admin_commands(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ToggleBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    toggle_gateway(
        state,
        body,
        |gw, v| gw.admin_commands_enabled = v,
        "admin_commands_enabled",
    )
    .await
}

async fn toggle_gateway(
    state: Arc<ApiState>,
    body: ToggleBody,
    apply: fn(&mut crate::settings::GatewayConfig, bool),
    key: &str,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut config = state.cfg.get().await;
    let revision = body.revision.unwrap_or(config.revision);
    apply(&mut config.gateway, body.enabled);
    match state.cfg.put(config, revision).await {
        Ok(revision) => {
            let config = state.cfg.get().await;
            Ok(Json(json!({
                "ok": true,
                "key": key,
                "enabled": body.enabled,
                "reply_enabled": config.gateway.reply_enabled,
                "admin_commands_enabled": config.gateway.admin_commands_enabled,
                "revision": revision,
            })))
        }
        Err(error) => Err(config_error_response(&error)),
    }
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
    let current_round = state.cfg.get().await.round.round_id;
    if state.pipeline.is_locked().await
        || crate::services::settlement::round_locked(&current_round).unwrap_or(false)
    {
        return Err((
            StatusCode::LOCKED,
            Json(json!({ "error": "locked", "message": "轮次已锁定，请先解锁" })),
        ));
    }
    if !body
        .config
        .gateway
        .whitelist_groups
        .iter()
        .any(|group| group == &body.config.round.group_id)
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "round_group_not_whitelisted",
                "message": "轮次群号不在网关群白名单中",
            })),
        ));
    }
    if !crate::settings::rounds::is_valid_round_id(&body.config.round.round_id) {
        return Err(super::api_bad_request("invalid_round_id"));
    }
    if let Some(issue) = crate::services::rounds::category_issues(
        &body.config.round,
        body.config.gateway.test_group_id.as_deref(),
    )
    .into_iter()
    .find(|issue| issue["level"] == "error")
    {
        return Err(super::api_bad_request(
            issue["message"]
                .as_str()
                .unwrap_or("category_binding_invalid"),
        ));
    }
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
        let error = ConfigError::Io(std::io::Error::other("boom"));
        let (status, Json(body)) = config_error_response(&error);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["error"].as_str().unwrap().contains("boom"));
    }
}
