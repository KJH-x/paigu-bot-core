pub mod routes;
pub mod admin_routes;
pub mod public_routes;
pub mod webhook_routes;
pub mod board_routes;
pub mod config_routes;
pub mod display_routes;
pub mod member_routes;
pub mod message_routes;
pub mod replay_routes;
pub mod sim_routes;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::get_service;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::gateway::Gateway;
use crate::llm::Pipeline;
use crate::settings::ConfigStore;

pub struct ApiState {
    pub cfg: Arc<ConfigStore>,
    pub pipeline: Arc<Pipeline>,
    pub gateway: Arc<Gateway>,
}

pub fn web_dir() -> PathBuf {
    std::env::var("PAIGU_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web"))
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            let value = origin.to_str().unwrap_or("");
            value == "null"
                || value.starts_with("http://127.0.0.1:")
                || value.starts_with("http://localhost:")
        }))
        .allow_methods(Any)
        .allow_headers(Any)
}

pub(crate) type ApiError = (StatusCode, Json<Value>);

pub(crate) fn api_internal(error: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": error.to_string() })),
    )
}

pub(crate) fn api_bad_request(error: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": error.to_string() })),
    )
}

pub(crate) fn api_not_found(error: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": error.to_string() })),
    )
}

pub(crate) fn api_stale_revision(revision: u64) -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": "stale_revision", "revision": revision })),
    )
}

pub fn build_router(state: Arc<ApiState>) -> Router {
    let web = web_dir();
    let display_page = ServeFile::new(web.join("display.html"));
    let admin_page = ServeFile::new(web.join("admin.html"));
    let sim_page = ServeFile::new(web.join("sim.html"));
    let replay_page = ServeFile::new(web.join("replay.html"));

    Router::new()
        .merge(config_routes::routes())
        .merge(board_routes::routes())
        .merge(display_routes::routes())
        .merge(sim_routes::routes())
        .merge(member_routes::routes())
        .merge(message_routes::routes())
        .merge(replay_routes::routes())
        .route("/", get_service(display_page))
        .route("/admin", get_service(admin_page))
        .route("/sim", get_service(sim_page))
        .route("/replay", get_service(replay_page))
        .nest_service("/web", ServeDir::new(web.clone()))
        .fallback_service(ServeDir::new(web))
        .with_state(state)
        .layer(cors_layer())
}

pub async fn serve(state: Arc<ApiState>, port: u16) -> anyhow::Result<()> {
    let app = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "HTTP API listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ConfigStore;

    #[test]
    fn web_dir_defaults_to_web() {
        if std::env::var("PAIGU_WEB_DIR").is_err() {
            assert_eq!(web_dir(), PathBuf::from("web"));
        }
    }

    #[test]
    fn cors_layer_builds() {
        let _ = cors_layer();
    }

    #[tokio::test]
    async fn build_router_registers_message_replay_and_snapshot_routes() {
        let path = std::env::temp_dir().join(format!(
            "paigu-api-router-{}.json",
            uuid::Uuid::new_v4()
        ));
        let cfg = Arc::new(ConfigStore::load(&path).expect("load config"));
        let pipeline = crate::llm::Pipeline::new(cfg.clone());
        let gateway = crate::gateway::Gateway::new(cfg.clone(), pipeline.clone());
        let state = Arc::new(ApiState {
            cfg,
            pipeline,
            gateway,
        });
        let _router = build_router(state);
        let _ = std::fs::remove_file(&path);
    }
}
