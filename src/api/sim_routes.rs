use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::bus::IncomingEvent;

use super::{display_routes, ApiState};

fn identities() -> &'static Mutex<BTreeMap<String, Value>> {
    static IDENTITIES: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();
    IDENTITIES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn identity_list() -> Vec<Value> {
    identities()
        .lock()
        .map(|map| map.values().cloned().collect())
        .unwrap_or_default()
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/sim/message", post(sim_message))
        .route("/api/sim/identity", post(sim_identity))
        .route("/api/sim/reset", post(sim_reset))
}

#[derive(Deserialize)]
struct SimMessage {
    user_id: String,
    nickname: String,
    text: String,
    #[serde(default)]
    offset_ms: i64,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    is_admin: bool,
}

async fn sim_message(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<SimMessage>,
) -> Json<Value> {
    let cfg = state.cfg.get().await;
    let group_id = body
        .group_id
        .filter(|g| !g.trim().is_empty())
        .unwrap_or_else(|| cfg.round.group_id.clone());
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let event = IncomingEvent {
        group_id,
        user_id: body.user_id,
        nickname: body.nickname,
        message_id: format!("sim-{}", uuid::Uuid::new_v4()),
        text: body.text,
        timestamp_ms: now_ms + body.offset_ms,
        is_admin: body.is_admin,
        raw: None,
    };

    let outcome = state.pipeline.process(event).await;
    let version = outcome.version;
    let board = outcome.snapshot.clone().unwrap_or(Value::Null);
    Json(json!({ "outcome": outcome, "board": board, "version": version }))
}

#[derive(Deserialize)]
struct SimIdentity {
    user_id: String,
    nickname: String,
    #[serde(default)]
    is_admin: bool,
    #[serde(default)]
    priority: Option<i32>,
}

async fn sim_identity(Json(body): Json<SimIdentity>) -> Json<Value> {
    let record = json!({
        "user_id": body.user_id,
        "nickname": body.nickname,
        "is_admin": body.is_admin,
        "priority": body.priority,
    });
    if let Ok(mut map) = identities().lock() {
        map.insert(body.user_id.clone(), record.clone());
    }
    Json(json!({ "ok": true, "identity": record, "identities": identity_list() }))
}

async fn sim_reset(State(state): State<Arc<ApiState>>) -> Json<Value> {
    state.pipeline.reset().await;
    display_routes::clear_cache();
    let (version, _) = state.pipeline.board().await;
    Json(json!({ "ok": true, "version": version }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_store_records_and_lists() {
        let record = json!({ "user_id": "u-test", "nickname": "测试" });
        identities()
            .lock()
            .unwrap()
            .insert("u-test".to_string(), record.clone());
        let listed = identity_list();
        assert!(listed.iter().any(|v| v["user_id"] == "u-test"));
    }
}
