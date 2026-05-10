use axum::{Router, routing::{get, post}, extract::{Path, State}, Json};
use std::sync::Arc;
use crate::app_state::AppState;
use serde_json::Value;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(admin_dashboard))
        .route("/rounds", post(create_round))
        .route("/rounds/{id}/items", post(add_item))
        .route("/rounds/{id}/discounts", post(set_discount))
        .route("/rounds/{id}/close", post(close_round))
        .route("/rounds/{id}/export", get(export))
        .with_state(state)
}

async fn admin_dashboard(State(_state): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running"
    }))
}

async fn create_round(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Json<Value> {
    let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("unnamed");
    let group_id = body.get("group_id").and_then(|v| v.as_str()).unwrap_or("default");
    let created_by = body.get("created_by").and_then(|v| v.as_str()).unwrap_or("admin");

    match state.services.round.create_round(
        title.to_string(),
        group_id.to_string(),
        created_by.to_string(),
        None,
        None,
    ).await {
        Ok(round) => Json(serde_json::json!(round)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn add_item(State(_state): State<Arc<AppState>>, Path(_id): Path<String>, Json(_body): Json<Value>) -> Json<Value> {
    Json(serde_json::json!({ "status": "not_implemented" }))
}

async fn set_discount(State(_state): State<Arc<AppState>>, Path(_id): Path<String>, Json(_body): Json<Value>) -> Json<Value> {
    Json(serde_json::json!({ "status": "not_implemented" }))
}

async fn close_round(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let round_id = crate::domain::ids::RoundId(id);
    match state.services.round.close_round(&round_id).await {
        Ok(()) => Json(serde_json::json!({ "status": "closed", "round_id": round_id.0 })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

async fn export(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let round_id = crate::domain::ids::RoundId(id);
    match state.services.settlement.get_latest_settlement(&round_id).await {
        Ok(Some(snapshot)) => {
            match state.services.export.export_user_bills(&snapshot) {
                Ok(csv_bytes) => {
                    let csv_str = String::from_utf8_lossy(&csv_bytes).to_string();
                    Json(serde_json::json!({ "round_id": round_id.0, "csv": csv_str }))
                }
                Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
            }
        }
        Ok(None) => Json(serde_json::json!({ "round_id": round_id.0, "message": "No settlement snapshot available. Close the round first." })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
