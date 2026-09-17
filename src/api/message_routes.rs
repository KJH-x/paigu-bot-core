use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::messages::{next_seq, MessageRecord, MessageStore};
use crate::replay::session::{self, ReplayOverrides};

use super::{api_bad_request, api_internal, api_not_found, api_stale_revision, ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route("/api/messages", get(list_messages).post(create_message))
        .route(
            "/api/messages/:seq",
            put(update_message).delete(delete_message),
        )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn validate_record(rec: &MessageRecord) -> Result<(), String> {
    if rec.user_id.trim().is_empty() {
        return Err("user_id 不能为空".to_string());
    }
    if rec.text.trim().is_empty() {
        return Err("text 不能为空".to_string());
    }
    if rec.timestamp_ms <= 0 {
        return Err("timestamp_ms 必须为正".to_string());
    }
    if rec.group_id.trim().is_empty() {
        return Err("group_id 不能为空".to_string());
    }
    Ok(())
}

fn message_view(rec: &MessageRecord) -> Value {
    json!({
        "seq": rec.seq,
        "group_id": rec.group_id,
        "user_id": rec.user_id,
        "nickname": rec.nickname,
        "display": rec.nickname,
        "message_id": rec.message_id,
        "text": rec.text,
        "timestamp_ms": rec.timestamp_ms,
        "is_admin": rec.is_admin,
        "routed": rec.routed,
        "status": rec.status,
        "detail": rec.detail,
    })
}

async fn recompute(state: &ApiState) -> anyhow::Result<Value> {
    let cfg = state.cfg.get().await;
    let store = state.messages.store_for(&cfg.round.round_id);
    let result = session::replay(&store, &cfg, ReplayOverrides::default()).await?;
    Ok(super::replay_routes::replay_result_json(&result))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_messages(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    let mut records = state.messages.store_for(&cfg.round.round_id).read_all().await.map_err(api_internal)?;
    records.sort_by_key(|r| r.seq);

    let since = query.since.unwrap_or(0);
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    let filtered: Vec<&MessageRecord> = records.iter().filter(|r| r.seq > since).collect();
    let total = filtered.len();
    let page: Vec<Value> = filtered.into_iter().take(limit).map(message_view).collect();
    let returned = page.len();
    let (version, _) = state.pipeline.board().await;

    Ok(Json(json!({
        "messages": page,
        "version": version,
        "count": returned,
        "total": total,
        "since": since,
        "limit": limit,
    })))
}

#[derive(Deserialize)]
struct CreateBody {
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    timestamp_ms: Option<i64>,
    #[serde(default)]
    is_admin: bool,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    routed: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    recompute: Option<bool>,
}

async fn create_message(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(api_stale_revision(cfg.revision));
        }
    }

    let rec = MessageRecord {
        seq: next_seq(),
        group_id: body
            .group_id
            .filter(|g| !g.trim().is_empty())
            .unwrap_or_else(|| cfg.round.group_id.clone()),
        user_id: body.user_id,
        nickname: body.nickname,
        message_id: body
            .message_id
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("admin-{}", uuid::Uuid::new_v4())),
        text: body.text,
        timestamp_ms: body.timestamp_ms.unwrap_or_else(now_ms),
        is_admin: body.is_admin,
        routed: body.routed.unwrap_or_else(|| "message".to_string()),
        status: body.status.unwrap_or_else(|| "Pending".to_string()),
        detail: body.detail.unwrap_or_default(),
    };
    validate_record(&rec).map_err(api_bad_request)?;
    state.messages.store_for(&cfg.round.round_id).append(&rec).await.map_err(api_internal)?;

    let recomputed = body.recompute.unwrap_or(true);
    let result = if recomputed {
        Some(recompute(&state).await.map_err(api_internal)?)
    } else {
        None
    };
    Ok(Json(json!({
        "ok": true,
        "message": message_view(&rec),
        "recomputed": recomputed,
        "result": result,
    })))
}

#[derive(Deserialize)]
struct UpdateBody {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    timestamp_ms: Option<i64>,
    #[serde(default)]
    is_admin: Option<bool>,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    routed: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    recompute: Option<bool>,
}

async fn update_message(
    State(state): State<Arc<ApiState>>,
    Path(seq): Path<i64>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(api_stale_revision(cfg.revision));
        }
    }

    let store = state.messages.store_for(&cfg.round.round_id);
    let mut records = store.read_all().await.map_err(api_internal)?;
    let target = records
        .iter_mut()
        .find(|r| r.seq == seq)
        .ok_or_else(|| api_not_found(format!("message {seq} not found")))?;

    if let Some(v) = body.user_id {
        target.user_id = v;
    }
    if let Some(v) = body.nickname {
        target.nickname = v;
    }
    if let Some(v) = body.text {
        target.text = v;
    }
    if let Some(v) = body.timestamp_ms {
        target.timestamp_ms = v;
    }
    if let Some(v) = body.is_admin {
        target.is_admin = v;
    }
    if let Some(v) = body.group_id {
        target.group_id = v;
    }
    if let Some(v) = body.routed {
        target.routed = v;
    }
    if let Some(v) = body.status {
        target.status = v;
    }
    if let Some(v) = body.detail {
        target.detail = v;
    }
    validate_record(target).map_err(api_bad_request)?;
    let updated = target.clone();
    store.replace_all(&records).await.map_err(api_internal)?;

    let recomputed = body.recompute.unwrap_or(true);
    let result = if recomputed {
        Some(recompute(&state).await.map_err(api_internal)?)
    } else {
        None
    };
    Ok(Json(json!({
        "ok": true,
        "message": message_view(&updated),
        "recomputed": recomputed,
        "result": result,
    })))
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    recompute: Option<bool>,
}

async fn delete_message(
    State(state): State<Arc<ApiState>>,
    Path(seq): Path<i64>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let cfg = state.cfg.get().await;
    if !state
        .messages
        .delete(&cfg.round.round_id, seq)
        .await
        .map_err(api_internal)?
    {
        return Err(api_not_found(format!("message {seq} not found")));
    }

    let recomputed = query.recompute.unwrap_or(true);
    let result = if recomputed {
        Some(recompute(&state).await.map_err(api_internal)?)
    } else {
        None
    };
    Ok(Json(json!({
        "ok": true,
        "removed": seq,
        "recomputed": recomputed,
        "result": result,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MessageRecord {
        MessageRecord {
            seq: 7,
            group_id: "720675572".to_string(),
            user_id: "u1".to_string(),
            nickname: "成员01".to_string(),
            message_id: "m7".to_string(),
            text: "排 徽章 甲 1".to_string(),
            timestamp_ms: 1_000,
            is_admin: false,
            routed: "message".to_string(),
            status: "Applied".to_string(),
            detail: "claim".to_string(),
        }
    }

    #[test]
    fn validate_record_rejects_bad_fields() {
        assert!(validate_record(&sample()).is_ok());

        let mut missing_user = sample();
        missing_user.user_id = "  ".to_string();
        assert!(validate_record(&missing_user).is_err());

        let mut empty_text = sample();
        empty_text.text = String::new();
        assert!(validate_record(&empty_text).is_err());

        let mut bad_ts = sample();
        bad_ts.timestamp_ms = 0;
        assert!(validate_record(&bad_ts).is_err());
    }

    #[test]
    fn message_view_exposes_store_and_display_fields() {
        let view = message_view(&sample());
        assert_eq!(view["seq"], 7);
        assert_eq!(view["display"], "成员01");
        assert_eq!(view["nickname"], "成员01");
        assert_eq!(view["routed"], "message");
        assert_eq!(view["status"], "Applied");
        assert_eq!(view["timestamp_ms"], 1_000);
    }
}
