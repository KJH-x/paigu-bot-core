//! 工作流只读快照（`GET /api/workflow`）。
//!
//! 用途：让前端在各页一致地呈现「当前阶段 / 锁定态 / 网关连接 / 配置进度」，
//! 从而按阶段切换工作面板（Stepper）。**纯只读**，不改变任何业务状态。

use serde_json::{json, Value};

use crate::api::ApiState;

pub async fn snapshot(state: &ApiState) -> Value {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    let now_ms = chrono::Utc::now().timestamp_millis();

    let phase = crate::round::phase_at(&cfg.round.phases, now_ms);
    let phases: Vec<Value> = cfg
        .round
        .phases
        .iter()
        .map(|w| {
            json!({
                "phase": w.phase.as_str(),
                "label": w.phase.label(),
                "start_ms": w.start_ms,
                "end_ms": w.end_ms,
            })
        })
        .collect();
    let priority_window = cfg.round.priority_window.as_ref().map(|w| {
        json!({
            "start_ms": w.start_ms,
            "end_ms": w.end_ms,
            "active": now_ms >= w.start_ms && now_ms < w.end_ms,
        })
    });

    let settlement = &cfg.settlement;
    let settlement_configured = !settlement.pricing.is_empty()
        || !settlement.discounts.is_empty()
        || !settlement.gift_tiers.is_empty();

    let members_cached = crate::services::members::get_members(state)
        .await
        .get("members")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let mut body = json!({
        "round_id": cfg.round.round_id,
        "title": cfg.round.title,
        "group_id": cfg.round.group_id,
        "category": cfg.round.category,
        "phase": phase.map(|p| p.as_str()),
        "phase_label": phase.map(|p| p.label()),
        "phases": phases,
        "phases_configured": !cfg.round.phases.is_empty(),
        "priority_window": priority_window,
        "items": cfg.round.items.len(),
        "settlement_configured": settlement_configured,
        "members_cached": members_cached,
        "reply_enabled": cfg.gateway.reply_enabled,
        "admin_commands_enabled": cfg.gateway.admin_commands_enabled,
        "revision": cfg.revision,
        "gateway": state.gateway.status().await,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });

    // 合并 Pipeline 侧字段（locked/version/events/messages/claims/eligibilities）
    let pipeline = state.pipeline.workflow().await;
    let (_, board) = state.pipeline.board().await;
    let messages = state.pipeline.messages_since(0).await;
    let (epoch, revision, _) = state
        .pipeline
        .view_stamp(cfg.revision, &board, &messages)
        .await;
    body["view_epoch"] = json!(epoch);
    body["view_revision"] = json!(revision);
    if let (Some(dst), Some(src)) = (body.as_object_mut(), pipeline.as_object()) {
        for (key, value) in src {
            dst.insert(key.clone(), value.clone());
        }
        match crate::services::settlement::round_locked(&cfg.round.round_id) {
            Ok(locked) => {
                let phase_locked = dst.get("locked").and_then(Value::as_bool).unwrap_or(false);
                dst.insert("locked".to_string(), json!(locked || phase_locked));
            }
            Err(_) => {
                dst.insert("locked".to_string(), json!(true));
                dst.insert("lock_error".to_string(), json!(true));
            }
        }
    }
    body
}
