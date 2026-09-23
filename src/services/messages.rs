use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::messages::{next_seq, MessageRecord};

use super::{replay, ServiceError};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn validate_record(rec: &MessageRecord) -> Result<(), String> {
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

pub fn message_view(rec: &MessageRecord) -> Value {
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

pub async fn list_messages(
    state: &ApiState,
    since: i64,
    limit: Option<usize>,
) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let mut records = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .map_err(ServiceError::from)?;
    records.sort_by_key(|r| r.seq);

    let limit = limit.unwrap_or(500).clamp(1, 5000);
    let filtered: Vec<&MessageRecord> = records.iter().filter(|r| r.seq > since).collect();
    let total = filtered.len();
    let page: Vec<Value> = filtered.into_iter().take(limit).map(message_view).collect();
    let returned = page.len();
    let (version, _) = state.pipeline.board().await;

    Ok(json!({
        "messages": page,
        "version": version,
        "count": returned,
        "total": total,
        "since": since,
        "limit": limit,
    }))
}

#[derive(Deserialize)]
pub struct CreateInput {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub routed: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub recompute: Option<bool>,
}

pub async fn create_message(state: &ApiState, body: CreateInput) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(ServiceError::StaleRevision(cfg.revision));
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
    validate_record(&rec).map_err(ServiceError::BadRequest)?;
    state
        .messages
        .append(&cfg.round.round_id, &rec)
        .await
        .map_err(ServiceError::from)?;

    let recomputed = body.recompute.unwrap_or(true);
    let result = if recomputed {
        Some(replay::recompute(state).await.map_err(ServiceError::from)?)
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "message": message_view(&rec),
        "recomputed": recomputed,
        "result": result,
    }))
}

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub is_admin: Option<bool>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub routed: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub recompute: Option<bool>,
}

pub async fn update_message(
    state: &ApiState,
    seq: i64,
    body: UpdateInput,
) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(ServiceError::StaleRevision(cfg.revision));
        }
    }

    let mut updated: Option<MessageRecord> = None;
    let mut invalid: Option<String> = None;
    let hit = state
        .messages
        .update(&cfg.round.round_id, seq, |target| {
            let mut candidate = target.clone();
            if let Some(v) = &body.user_id {
                candidate.user_id = v.clone();
            }
            if let Some(v) = &body.nickname {
                candidate.nickname = v.clone();
            }
            if let Some(v) = &body.text {
                candidate.text = v.clone();
            }
            if let Some(v) = &body.timestamp_ms {
                candidate.timestamp_ms = *v;
            }
            if let Some(v) = &body.is_admin {
                candidate.is_admin = *v;
            }
            if let Some(v) = &body.group_id {
                candidate.group_id = v.clone();
            }
            if let Some(v) = &body.routed {
                candidate.routed = v.clone();
            }
            if let Some(v) = &body.status {
                candidate.status = v.clone();
            }
            if let Some(v) = &body.detail {
                candidate.detail = v.clone();
            }
            match validate_record(&candidate) {
                Ok(()) => {
                    *target = candidate.clone();
                    updated = Some(candidate);
                }
                Err(e) => invalid = Some(e),
            }
        })
        .await
        .map_err(ServiceError::from)?;
    if !hit {
        return Err(ServiceError::NotFound(format!("message {seq} not found")));
    }
    if let Some(error) = invalid {
        return Err(ServiceError::BadRequest(error));
    }
    let updated = updated.expect("命中且校验通过的记录");

    let recomputed = body.recompute.unwrap_or(true);
    let result = if recomputed {
        Some(replay::recompute(state).await.map_err(ServiceError::from)?)
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "message": message_view(&updated),
        "recomputed": recomputed,
        "result": result,
    }))
}

pub async fn delete_message(
    state: &ApiState,
    seq: i64,
    recompute: Option<bool>,
) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    if !state
        .messages
        .delete(&cfg.round.round_id, seq)
        .await
        .map_err(ServiceError::from)?
    {
        return Err(ServiceError::NotFound(format!("message {seq} not found")));
    }

    let recomputed = recompute.unwrap_or(true);
    let result = if recomputed {
        Some(replay::recompute(state).await.map_err(ServiceError::from)?)
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "removed": seq,
        "recomputed": recomputed,
        "result": result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MessageRecord {
        MessageRecord {
            seq: 7,
            group_id: "123456789".to_string(),
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
