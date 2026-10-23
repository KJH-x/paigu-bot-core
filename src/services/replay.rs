use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::domain::snapshot::AllocationSnapshot;
use crate::replay::session::{self, ReplayDiff, ReplayOverrides, ReplayResult};

use super::ServiceError;

pub fn replay_result_json(result: &ReplayResult) -> Value {
    json!({
        "round_id": result.round_id,
        "revision": result.revision,
        "version": result.version,
        "messages": result.messages,
        "board": result.board,
        "outcomes": result.outcomes,
        "diff": result.diff,
        "settlement": result.settlement,
        "event_count": result.events.len(),
    })
}

pub async fn do_replay(
    state: &ApiState,
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    state
        .messages
        .read_ingress_records(&cfg.round.round_id)
        .await?;
    session::replay(state.messages.as_ref(), &cfg, overrides).await
}

/// 消息变更后的权威重算：原消息不改写，一次发布派生投影。
/// 调用方持有共同写锁（实时/重放同源，含合并分组）。
pub(crate) async fn recompute_locked(
    state: &ApiState,
    cfg: &crate::settings::AppConfig,
) -> anyhow::Result<Value> {
    let started = std::time::Instant::now();
    let result = state.pipeline.recompute_locked(cfg).await?;

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for outcome in &result.outcomes {
        *counts.entry(outcome.status.as_str()).or_default() += 1;
    }
    let mut value = replay_result_json(&result);
    value["summary"] = json!({
        "applied": counts.get("Applied").copied().unwrap_or(0),
        "dropped": counts.get("Dropped").copied().unwrap_or(0),
        "rejected": counts.get("Rejected").copied().unwrap_or(0),
        "ignored": counts.get("Ignored").copied().unwrap_or(0),
        "merged": counts.get("Merged").copied().unwrap_or(0),
        "elapsed_ms": started.elapsed().as_millis(),
    });
    Ok(value)
}

#[derive(Deserialize, Default)]
pub struct ReplayBody {
    #[serde(default)]
    pub overrides: ReplayOverrides,
}

pub async fn run_replay(state: &ApiState, body: ReplayBody) -> Result<Value, ServiceError> {
    let result = do_replay(state, body.overrides)
        .await
        .map_err(ServiceError::from)?;
    Ok(replay_result_json(&result))
}

pub async fn active_view(state: &ApiState) -> Result<Value, ServiceError> {
    state
        .pipeline
        .replay_view()
        .await
        .map_err(ServiceError::from)
}

pub async fn replay_diff(state: &ApiState) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.read().await;
    {
        let _fence = state.messages.ingress_commit_guard().await;
        super::configuration::ensure_no_pending_ingress(&state.messages, &cfg.round.round_id)?;
    }
    state
        .messages
        .read_ingress_records(&cfg.round.round_id)
        .await?;
    let result = session::replay(&state.messages, &cfg, ReplayOverrides::default()).await?;
    let (live_version, live_board) = state.pipeline.board().await;
    let live_snapshot: Option<AllocationSnapshot> = if live_board.is_null() {
        None
    } else {
        Some(serde_json::from_value(live_board).map_err(anyhow::Error::from)?)
    };
    if live_snapshot
        .as_ref()
        .is_some_and(|board| board.round_id.0 != cfg.round.round_id)
    {
        return Err(ServiceError::Conflict(
            json!({"error":"replay_context_changed",
            "message":"当前实时排表不属于激活轮次，未拼接不同轮次；请重新读取"}),
        ));
    }
    let live_diff: Option<ReplayDiff> =
        live_snapshot.map(|board| ReplayDiff::from_boards(&board, &result.board));

    Ok(json!({
        "round_id": cfg.round.round_id,
        "revision": cfg.revision,
        "source": "active_round",
        "comparison": "saved_actions_vs_live_allocation",
        "message_count": result.messages.len(),
        "live_available": live_diff.is_some(),
        "consistent": live_diff.as_ref().map(|diff| !diff.changed),
        "version": result.version,
        "base_version": result.diff.base_version,
        "diff": result.diff,
        "live_version": live_version,
        "live_diff": live_diff,
    }))
}

/// 原始事件日志（C-3）：返回**最后 `limit` 条**（`limit=0` 为全部）。
pub async fn list_raw_events(state: &ApiState, limit: usize) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let events = state
        .messages
        .read_raw_events(&cfg.round.round_id, limit)
        .await
        .map_err(ServiceError::from)?;
    Ok(json!({
        "round_id": cfg.round.round_id,
        "count": events.len(),
        "limit": limit,
        "events": events,
    }))
}
