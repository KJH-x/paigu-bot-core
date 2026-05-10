use axum::Router;
use std::sync::Arc;

use super::webhook_routes;
use super::admin_routes;
use super::public_routes;
use crate::app_state::AppState;

pub fn build_router(state: AppState) -> Router {
    let shared_state = Arc::new(state);

    Router::new()
        .nest("/webhook", webhook_routes::routes(shared_state.clone()))
        .nest("/admin", admin_routes::routes(shared_state.clone()))
        .nest("/public", public_routes::routes(shared_state.clone()))
        .nest("/api/replays", replay_routes::routes(shared_state.clone()))
        .nest("/api/simulations", simulation_routes::routes(shared_state.clone()))
}

pub mod replay_routes {
    use axum::{Router, routing::get, extract::{Path, State}, Json};
    use std::sync::Arc;
    use crate::app_state::AppState;
    use serde_json::Value;

    pub fn routes(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/{round_id}/replays", get(list_replays))
            .route("/{round_id}/replays/{replay_id}/manifest", get(get_manifest))
            .route("/{round_id}/replays/{replay_id}/steps", get(list_steps))
            .route("/{round_id}/replays/{replay_id}/steps/{step_index}", get(get_step))
            .route("/{round_id}/replays/{replay_id}/snapshots/{step_index}", get(get_snapshot))
            .route("/{round_id}/replays/{replay_id}/diff/{step_index}", get(get_diff))
            .with_state(state)
    }

    async fn list_replays(State(_state): State<Arc<AppState>>, Path(_round_id): Path<String>) -> Json<Value> {
        Json(serde_json::json!({ "replays": [] }))
    }

    async fn get_manifest(State(_state): State<Arc<AppState>>, Path((round_id, replay_id)): Path<(String, String)>) -> Json<Value> {
        Json(serde_json::json!({ "round_id": round_id, "replay_id": replay_id }))
    }

    async fn list_steps(State(_state): State<Arc<AppState>>, Path((_round_id, _replay_id)): Path<(String, String)>) -> Json<Value> {
        Json(serde_json::json!({ "steps": [] }))
    }

    async fn get_step(State(_state): State<Arc<AppState>>, Path((_round_id, _replay_id, _step_index)): Path<(String, String, u64)>) -> Json<Value> {
        Json(serde_json::json!({}))
    }

    async fn get_snapshot(State(_state): State<Arc<AppState>>, Path((_round_id, _replay_id, _step_index)): Path<(String, String, u64)>) -> Json<Value> {
        Json(serde_json::json!({}))
    }

    async fn get_diff(State(_state): State<Arc<AppState>>, Path((_round_id, _replay_id, _step_index)): Path<(String, String, u64)>) -> Json<Value> {
        Json(serde_json::json!({}))
    }
}

pub mod simulation_routes {
    use axum::{Router, routing::post, extract::State, Json};
    use std::sync::Arc;
    use crate::app_state::AppState;
    use serde_json::Value;

    pub fn routes(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/", post(run_simulation))
            .with_state(state)
    }

    async fn run_simulation(State(_state): State<Arc<AppState>>, Json(_body): Json<Value>) -> Json<Value> {
        Json(serde_json::json!({ "status": "not_implemented" }))
    }
}
