use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::messages::{new_record_id, next_seq, MessageRecord};

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
        "record_id": rec.record_id,
        "record_revision": rec.record_revision,
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
        "original_automatic":crate::messages::processing::original_summary(rec),
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
    let live = state.pipeline.messages_since(0).await;
    let page: Vec<Value> = filtered
        .into_iter()
        .take(limit)
        .map(|record| {
            let mut view = message_view(record);
            if let Some(current) = live
                .iter()
                .find(|m| m["record_id"] == record.record_id && m["round_id"] == cfg.round.round_id)
            {
                for key in [
                    "record_revision",
                    "status",
                    "detail",
                    "manual_note",
                    "round_id",
                ] {
                    view[key] = current[key].clone();
                }
            }
            view
        })
        .collect();
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

pub async fn actions(state: &ApiState, key: &str, round_id: &str) -> Result<Value, ServiceError> {
    super::message_actions::read(state, key, round_id).await
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
    if crate::services::settlement::is_locked(state).await? {
        return Err(ServiceError::Locked("轮次已锁定，不能新增消息".to_string()));
    }
    let cfg = state.cfg.get().await;
    if let Some(rev) = body.revision {
        if rev != cfg.revision {
            return Err(ServiceError::StaleRevision(cfg.revision));
        }
    }

    let rec = MessageRecord {
        record_id: new_record_id(),
        record_revision: 1,
        processing: None,
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
        sender_user_id: None,
        sender_nickname: None,
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
    pub record_revision: Option<u64>,
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
    pub recompute: Option<bool>,
}

pub async fn update_message(
    state: &ApiState,
    key: &str,
    body: UpdateInput,
) -> Result<Value, ServiceError> {
    if crate::services::settlement::is_locked(state).await? {
        return Err(ServiceError::Locked("轮次已锁定，不能修改消息".to_string()));
    }
    let cfg = state.cfg.get().await;
    let record_id = resolve_key(state, &cfg.round.round_id, key).await?;

    let mut updated: Option<MessageRecord> = None;
    if crate::replay::session::load_parse_overrides(&state.messages, &cfg.round.round_id)
        .await
        .has_record(&record_id)
    {
        return Err(ServiceError::BadRequest(
            "已人工修订的消息须保留原文、身份与时间，请编辑动作".into(),
        ));
    }
    let mut invalid: Option<String> = None;
    let mut conflict: Option<MessageRecord> = None;
    let stored = state
        .messages
        .update(&cfg.round.round_id, &record_id, |target| {
            if let Some(expected) = body.record_revision {
                if expected != target.record_revision {
                    conflict = Some(target.clone());
                    return;
                }
            }
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
            if target.processing.is_some()
                && crate::messages::processing::ProcessingMember::from_record(target)
                    != crate::messages::processing::ProcessingMember::from_record(&candidate)
            {
                invalid =
                    Some("已自动处理的消息须保留原文、身份和时间；请使用动作修订".to_string());
                return;
            }
            match validate_record(&candidate) {
                Ok(()) => {
                    candidate.record_revision = target.record_revision.saturating_add(1);
                    *target = candidate.clone();
                    updated = Some(candidate);
                }
                Err(e) => invalid = Some(e),
            }
        })
        .await
        .map_err(ServiceError::from)?;
    if stored.is_none() {
        return Err(ServiceError::NotFound(format!("message {key} not found")));
    }
    if let Some(current) = conflict {
        return Err(ServiceError::Conflict(json!({
            "error": "stale_record_revision",
            "message": message_view(&current),
        })));
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
    let message = if recomputed {
        find_record(state, &cfg.round.round_id, &record_id).await?
    } else {
        updated
    };
    Ok(json!({
        "ok": true,
        "message": message_view(&message),
        "recomputed": recomputed,
        "result": result,
    }))
}

pub async fn delete_message(
    state: &ApiState,
    key: &str,
    record_revision: Option<u64>,
    recompute: Option<bool>,
) -> Result<Value, ServiceError> {
    if crate::services::settlement::is_locked(state).await? {
        return Err(ServiceError::Locked("轮次已锁定，不能删除消息".to_string()));
    }
    let cfg = state.cfg.get().await;
    let record_id = resolve_key(state, &cfg.round.round_id, key).await?;
    let current = find_record(state, &cfg.round.round_id, &record_id).await?;
    if current.processing.is_some()
        || crate::replay::session::load_parse_overrides(&state.messages, &cfg.round.round_id)
            .await
            .has_record(&record_id)
    {
        return Err(ServiceError::Unprocessable(
            "已自动处理的消息不能删除；请使用人工忽略/拒绝保留原始记录".to_string(),
        ));
    }
    if let Some(expected) = record_revision {
        if expected != current.record_revision {
            return Err(ServiceError::Conflict(json!({
                "error": "stale_record_revision",
                "message": message_view(&current),
            })));
        }
    }
    if !state
        .messages
        .delete(&cfg.round.round_id, &record_id)
        .await
        .map_err(ServiceError::from)?
    {
        return Err(ServiceError::NotFound(format!("message {key} not found")));
    }

    let recomputed = recompute.unwrap_or(true);
    let result = if recomputed {
        Some(replay::recompute(state).await.map_err(ServiceError::from)?)
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "removed": record_id,
        "recomputed": recomputed,
        "result": result,
    }))
}

#[derive(Deserialize, Default)]
pub struct RerouteInput {
    #[serde(default)]
    pub record_revision: Option<u64>,
}

pub async fn reroute_message(
    state: &ApiState,
    key: &str,
    body: RerouteInput,
) -> Result<Value, ServiceError> {
    if crate::services::settlement::is_locked(state).await? {
        return Err(ServiceError::Locked("轮次已锁定，不能重新路由".to_string()));
    }
    let cfg = state.cfg.get().await;
    let record_id = resolve_key(state, &cfg.round.round_id, key).await?;
    let current = find_record(state, &cfg.round.round_id, &record_id).await?;
    if let Some(expected) = body.record_revision {
        if expected != current.record_revision {
            return Err(ServiceError::Conflict(json!({
                "error": "stale_record_revision",
                "message": message_view(&current),
            })));
        }
    }
    let (identity, _) = crate::parser::normalize::clean_nickname(&current.nickname);
    let routed = if !crate::parser::policy::group_allowed(
        &cfg.gateway.whitelist_groups,
        &current.group_id,
    ) {
        "drop:not_whitelisted_group"
    } else if !crate::parser::policy::member_allowed(
        &cfg.gateway.whitelist_members,
        &[
            current.user_id.as_str(),
            current.nickname.as_str(),
            identity.as_str(),
        ],
    ) {
        "drop:not_whitelisted_member"
    } else {
        "message"
    };
    state
        .messages
        .update(&cfg.round.round_id, &record_id, |target| {
            target.routed = routed.to_string();
            target.record_revision = target.record_revision.saturating_add(1);
        })
        .await
        .map_err(ServiceError::from)?;
    let result = replay::recompute(state).await.map_err(ServiceError::from)?;
    let message = find_record(state, &cfg.round.round_id, &record_id).await?;
    Ok(json!({ "ok": true, "message": message_view(&message), "result": result }))
}

async fn resolve_key(state: &ApiState, round_id: &str, key: &str) -> Result<String, ServiceError> {
    match state
        .messages
        .resolve_record_id(round_id, key)
        .await
        .map_err(ServiceError::from)?
    {
        Ok(record_id) => Ok(record_id),
        Err(0) => Err(ServiceError::NotFound(format!("message {key} not found"))),
        Err(matches) => Err(ServiceError::Conflict(json!({
            "error": "ambiguous_legacy_seq",
            "seq": key,
            "matches": matches,
        }))),
    }
}

async fn find_record(
    state: &ApiState,
    round_id: &str,
    record_id: &str,
) -> Result<MessageRecord, ServiceError> {
    state
        .messages
        .read_all(round_id)
        .await
        .map_err(ServiceError::from)?
        .into_iter()
        .find(|record| record.record_id == record_id)
        .ok_or_else(|| ServiceError::NotFound(format!("message {record_id} not found")))
}

#[derive(Deserialize)]
pub struct OverrideInput {
    pub round_id: String,
    pub record_revision: u64,
    #[serde(default)]
    pub reason: Option<String>,
    pub parsed: crate::parser::parsed_event::ParsedMessage,
}

/// §10：人工确认「待确认」消息 → 写 `ParseOverride` 并重算。
pub async fn override_message(
    state: &ApiState,
    key: &str,
    body: OverrideInput,
) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    // Keep the active round/config fixed until this manual write has been recomputed.
    let cfg = state.cfg.read().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed", "round_id":cfg.round.round_id}),
        ));
    }
    if crate::services::settlement::round_locked(&cfg.round.round_id)?
        || state.pipeline.is_locked().await
    {
        return Err(ServiceError::Locked("轮次已锁定，不能修改解析".to_string()));
    }
    // No numeric/seq fallback for this administrative write.
    let record_id = key.to_string();
    let mut current = find_record(state, &cfg.round.round_id, &record_id).await?;
    let raw = state
        .messages
        .read_raw_events(&cfg.round.round_id, 0)
        .await
        .map_err(ServiceError::from)?;
    let events: Vec<crate::domain::event::EventEnvelope> = raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    let overrides = crate::parser::parsed_event::collect_parse_overrides(&events);
    current.record_revision = overrides.record_revision(&record_id, current.record_revision);
    if current.record_revision != body.record_revision {
        return Err(ServiceError::Conflict(
            json!({"error":"stale_record_revision", "message":message_view(&current)}),
        ));
    }
    if matches!(
        body.parsed.intent,
        crate::parser::parsed_event::ParsedIntent::AdminCommand
    ) || body.parsed.admin_command.is_some()
        || body
            .parsed
            .items
            .iter()
            .any(|item| item.quantity == 0 || item.quantity > 99)
    {
        return Err(ServiceError::BadRequest(
            "修订只能包含排谷动作，数量应为 1–99 的整数".to_string(),
        ));
    }
    // Until MP-3 provides a multi-record transaction, never partially detach an automatic group.
    if cfg.merge.enabled && cfg.merge.same_sender {
        let mut records = state
            .messages
            .read_all(&cfg.round.round_id)
            .await
            .map_err(ServiceError::from)?;
        records.sort_by_key(|r| (r.timestamp_ms, r.seq));
        let candidates: Vec<_> = records
            .iter()
            .map(|r| crate::parser::coalesce::Candidate {
                user_id: &r.user_id,
                text: &r.text,
                is_admin: r.is_admin,
                force_boundary: !r.routed.eq_ignore_ascii_case("message")
                    || overrides.has_record(&r.record_id),
            })
            .collect();
        let groups = crate::parser::coalesce::group_candidates(&candidates, &cfg.round.to_items());
        if groups
            .iter()
            .any(|g| g.len() > 1 && g.iter().any(|&i| records[i].record_id == record_id))
        {
            return Err(ServiceError::Conflict(
                json!({"error":"auto_group_requires_transaction", "message":"此消息属于自动合并组；完整组修订尚未接线，未修改任何记录"}),
            ));
        }
    }
    let reason = body.reason.unwrap_or_else(|| "人工确认".to_string());
    state
        .pipeline
        .apply_parse_override(&cfg, &current, &body.parsed, &reason)
        .await
        .map_err(ServiceError::from)?;
    let computed = state
        .pipeline
        .recompute_locked(&cfg)
        .await
        .map_err(ServiceError::from)?;
    replay::remember_replay(&computed);
    let result = replay::replay_result_json(&computed);
    let message = find_record(state, &cfg.round.round_id, &record_id).await?;
    Ok(json!({
        "ok": true,
        "message": message_view(&message),
        "result": result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> MessageRecord {
        MessageRecord {
            record_id: "record-7".to_string(),
            record_revision: 1,
            processing: None,
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
            sender_user_id: None,
            sender_nickname: None,
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
