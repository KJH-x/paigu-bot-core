use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::bus::IncomingEvent;
use crate::llm::pipeline::parse_timestamp_ms;
use crate::settings::AppConfig;

use super::{api_bad_request, ApiError, ApiState};

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

fn clear_identities() {
    if let Ok(mut map) = identities().lock() {
        map.clear();
    }
}

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/sim/message", post(sim_message))
        .route("/api/sim/messages", post(sim_messages))
        .route("/api/sim/identity", post(sim_identity))
        .route("/api/sim/reset", post(sim_reset))
}

/// 测试注入的**完整消息**（单条与批量共用）。
/// `user_id`（也接受别名 `id`）、`text`、`timestamp` 为必需；缺任一即该条 `Dropped`。
#[derive(Deserialize, Clone)]
struct TestMessage {
    #[serde(default, alias = "id")]
    user_id: String,
    #[serde(default)]
    text: String,
    /// 整数毫秒 / 纯数字串 / RFC3339 / 本地 `YYYY-MM-DD HH:MM[:SS]`。
    #[serde(default)]
    timestamp: Option<Value>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    is_admin: Option<bool>,
    #[serde(default)]
    message_id: Option<String>,
    /// 缺省 = 测试群；非测试群则该条 `Dropped`。
    #[serde(default)]
    group_id: Option<String>,
}

/// 批量体：允许**裸数组**，也允许 `{ "messages": [ … ] }`（R6）。
#[derive(Deserialize)]
#[serde(untagged)]
enum BatchBody {
    List(Vec<TestMessage>),
    Wrapped { messages: Vec<TestMessage> },
}

impl BatchBody {
    fn into_messages(self) -> Vec<TestMessage> {
        match self {
            BatchBody::List(list) => list,
            BatchBody::Wrapped { messages } => messages,
        }
    }
}

/// 测试注入仅测试轮可用（R10）。
fn ensure_test_round(cfg: &AppConfig) -> Result<(), ApiError> {
    if cfg.round.category.is_test() {
        Ok(())
    } else {
        Err(api_bad_request("not_a_test_round"))
    }
}

/// 测试群：仅取 `gateway.test_group_id`；未配置则无测试群（不再回退轮次群号）。
fn test_group(cfg: &AppConfig) -> Option<String> {
    cfg.gateway
        .test_group_id
        .clone()
        .filter(|g| !g.trim().is_empty())
}

enum Prepared {
    Event(Box<IncomingEvent>),
    Dropped(&'static str),
}

/// 逐条准备（R9）：缺必需字段 / timestamp 非法 / 群号非测试群 → `Dropped`，不进入处理。
fn prepare(msg: &TestMessage, cfg: &AppConfig) -> Prepared {
    if msg.user_id.trim().is_empty() || msg.text.trim().is_empty() {
        return Prepared::Dropped("missing_required_field");
    }
    let Some(timestamp_ms) = msg.timestamp.as_ref().and_then(parse_timestamp_ms) else {
        return Prepared::Dropped("missing_timestamp");
    };
    let Some(group) = test_group(cfg) else {
        return Prepared::Dropped("not_test_group");
    };
    let group_id = msg
        .group_id
        .clone()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or_else(|| group.clone());
    if group_id != group {
        return Prepared::Dropped("not_test_group");
    }
    Prepared::Event(Box::new(IncomingEvent {
        group_id,
        user_id: msg.user_id.trim().to_string(),
        nickname: msg.nickname.clone().unwrap_or_default(),
        message_id: msg
            .message_id
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("sim-{}", uuid::Uuid::new_v4())),
        text: msg.text.clone(),
        timestamp_ms,
        is_admin: msg.is_admin.unwrap_or(false),
        sender_user_id: None,
        ingress: None,
        sender_nickname: None,
        raw: None,
    }))
}

#[derive(Default)]
struct Summary {
    applied: usize,
    dropped: usize,
    rejected: usize,
    ignored: usize,
    merged: usize,
}

impl Summary {
    fn count(&mut self, status: &str) {
        match status {
            "Applied" => self.applied += 1,
            "Dropped" => self.dropped += 1,
            "Rejected" => self.rejected += 1,
            "Merged" => self.merged += 1,
            "Ignored" | "Duplicate" => self.ignored += 1,
            _ => {}
        }
    }

    fn json(&self) -> Value {
        json!({
            "applied": self.applied,
            "dropped": self.dropped,
            "rejected": self.rejected,
            "ignored": self.ignored,
            "merged": self.merged,
        })
    }
}

async fn sim_message(
    State(state): State<Arc<ApiState>>,
    Json(msg): Json<TestMessage>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    ensure_test_round(&cfg)?;
    match prepare(&msg, &cfg) {
        Prepared::Dropped(detail) => {
            let (version, board) = state.pipeline.board().await;
            Ok(Json(json!({
                "outcome": { "status": "Dropped", "detail": detail, "version": version },
                "board": board,
                "version": version,
            })))
        }
        Prepared::Event(ev) => {
            let outcome = state.pipeline.ingest(*ev).await;
            let version = outcome.version;
            let board = outcome.snapshot.clone().unwrap_or(Value::Null);
            Ok(Json(
                json!({ "outcome": outcome, "board": board, "version": version }),
            ))
        }
    }
}

async fn sim_messages(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<BatchBody>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    ensure_test_round(&cfg)?;
    let messages = body.into_messages();

    let mut summary = Summary::default();
    let mut results: Vec<Value> = Vec::with_capacity(messages.len());
    let mut version = 0i64;
    let mut board = Value::Null;

    for (index, msg) in messages.iter().enumerate() {
        match prepare(msg, &cfg) {
            Prepared::Dropped(detail) => {
                summary.dropped += 1;
                results.push(json!({
                    "index": index,
                    "user_id": msg.user_id,
                    "text": msg.text,
                    "status": "Dropped",
                    "detail": detail,
                    "version": version,
                }));
            }
            Prepared::Event(ev) => {
                let outcome = state.pipeline.ingest(*ev).await;
                version = outcome.version;
                if let Some(snapshot) = outcome.snapshot {
                    board = snapshot;
                }
                summary.count(&outcome.status);
                results.push(json!({
                    "index": index,
                    "user_id": msg.user_id,
                    "text": msg.text,
                    "status": outcome.status,
                    "detail": outcome.detail,
                    "version": outcome.version,
                }));
            }
        }
    }

    if board.is_null() {
        let (current_version, current_board) = state.pipeline.board().await;
        version = version.max(current_version);
        board = current_board;
    }

    Ok(Json(json!({
        "ok": true,
        "round_id": cfg.round.round_id,
        "category": cfg.round.category.as_str(),
        "count": messages.len(),
        "summary": summary.json(),
        "results": results,
        "board": board,
        "version": version,
    })))
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

async fn sim_reset(State(state): State<Arc<ApiState>>) -> Result<Json<Value>, ApiError> {
    let _serial = state.pipeline.message_write_guard().await;
    if !state.cfg.get().await.round.category.is_test() {
        return Err(api_bad_request("仅测试轮可重置模拟状态"));
    }
    let _fence = state.messages.ingress_commit_guard().await;
    crate::services::configuration::ensure_no_pending_ingress(
        &state.messages,
        &state.cfg.get().await.round.round_id,
    )
    .map_err(crate::services::ServiceError::into_api)?;
    state.pipeline.reset_locked().await;
    clear_identities();
    let (version, _) = state.pipeline.board().await;
    Ok(Json(json!({ "ok": true, "version": version })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(user_id: &str, text: &str, ts: i64) -> TestMessage {
        TestMessage {
            user_id: user_id.to_string(),
            text: text.to_string(),
            timestamp: Some(json!(ts)),
            nickname: None,
            is_admin: None,
            message_id: None,
            group_id: None,
        }
    }

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

    #[test]
    fn batch_body_accepts_bare_list_and_wrapped() {
        let bare: BatchBody = serde_json::from_value(json!([
            {"id": "1001", "text": "排 通行证 1", "timestamp": 1}
        ]))
        .unwrap();
        assert_eq!(bare.into_messages().len(), 1);

        let wrapped: BatchBody = serde_json::from_value(json!({
            "messages": [
                {"user_id": "1002", "text": "排 通行证 1", "timestamp": 2},
                {"user_id": "1003", "text": "排 通行证 1", "timestamp": 3}
            ]
        }))
        .unwrap();
        assert_eq!(wrapped.into_messages().len(), 2);
    }

    #[test]
    fn prepare_drops_missing_fields_and_foreign_group() {
        let mut cfg = crate::settings::default_config();
        cfg.gateway.test_group_id = Some("123456789".to_string());
        cfg.round.group_id = "123456789".to_string();

        // 缺 timestamp → Dropped。
        let mut no_ts = msg("1001", "排 通行证 1", 1);
        no_ts.timestamp = None;
        assert!(matches!(
            prepare(&no_ts, &cfg),
            Prepared::Dropped("missing_timestamp")
        ));

        // 缺 text → Dropped。
        assert!(matches!(
            prepare(&msg("1001", "   ", 1), &cfg),
            Prepared::Dropped("missing_required_field")
        ));

        // 非测试群 → Dropped。
        let mut foreign = msg("1001", "排 通行证 1", 1);
        foreign.group_id = Some("999".to_string());
        assert!(matches!(
            prepare(&foreign, &cfg),
            Prepared::Dropped("not_test_group")
        ));

        // 合法（缺省群 = 测试群）→ Event。
        assert!(matches!(
            prepare(&msg("1001", "排 通行证 1", 1), &cfg),
            Prepared::Event(_)
        ));
    }
}
