use axum::{Router, routing::post, extract::State, Json};
use std::sync::Arc;
use crate::app_state::AppState;
use crate::inbound::qq_message::IncomingQqMessage;
use serde_json::Value;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/qq-message", post(webhook_handler))
        .with_state(state)
}

async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<Value> {
    match serde_json::from_value::<IncomingQqMessage>(body) {
        Ok(msg) => {
            match state.services.message.handle_incoming(msg).await {
                Ok(reply) => Json(serde_json::json!({ "status": "ok", "reply": reply })),
                Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
            }
        }
        Err(e) => Json(serde_json::json!({ "status": "error", "message": format!("Invalid message format: {}", e) })),
    }
}
