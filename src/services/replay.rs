use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use crate::api::ApiState;
use crate::domain::snapshot::AllocationSnapshot;
use crate::replay::session::{self, ReplayDiff, ReplayOverrides, ReplayResult};

use super::ServiceError;

/// 最近一次重放结果，按 `round_id` 分键存储，避免跨轮次互相污染。
fn last_replays() -> &'static Mutex<HashMap<String, ReplayResult>> {
    static LAST: OnceLock<Mutex<HashMap<String, ReplayResult>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(HashMap::new()))
}

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

/// 记录一次重放结果（按 `round_id` 归档，供 `/api/replay/diff` 与 activate replay 复用）。
pub fn remember_replay(result: &ReplayResult) {
    if let Ok(mut map) = last_replays().lock() {
        map.insert(result.round_id.clone(), result.clone());
    }
}

pub async fn do_replay(
    state: &ApiState,
    overrides: ReplayOverrides,
) -> anyhow::Result<ReplayResult> {
    let cfg = state.cfg.get().await;
    let result = session::replay(state.messages.as_ref(), &cfg, overrides).await?;
    remember_replay(&result);
    Ok(result)
}

/// 消息变更后的重算：与 `do_replay` 不同，不写入 `last_replays`（保持原语义）。
pub async fn recompute(state: &ApiState) -> anyhow::Result<Value> {
    let cfg = state.cfg.get().await;
    let result =
        session::replay(state.messages.as_ref(), &cfg, ReplayOverrides::default()).await?;
    Ok(replay_result_json(&result))
}

#[derive(Deserialize, Default)]
pub struct ReplayBody {
    #[serde(default)]
    pub overrides: ReplayOverrides,
}

pub async fn run_replay(state: &ApiState, body: ReplayBody) -> Result<Value, ServiceError> {
    let result = do_replay(state, body.overrides).await.map_err(ServiceError::from)?;
    Ok(replay_result_json(&result))
}

pub async fn replay_diff(state: &ApiState) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let round_id = cfg.round.round_id.clone();
    let stored = last_replays()
        .lock()
        .ok()
        .and_then(|map| map.get(&round_id).cloned());
    let result = match stored {
        Some(result) => result,
        None => do_replay(state, ReplayOverrides::default())
            .await
            .map_err(ServiceError::from)?,
    };

    let (live_version, live_board) = state.pipeline.board().await;
    let live_snapshot: Option<AllocationSnapshot> = match serde_json::from_value(live_board) {
        Ok(board) => Some(board),
        Err(e) => {
            warn!("实时 board 反序列化失败: {e}");
            None
        }
    };
    let live_diff: Option<ReplayDiff> =
        live_snapshot.map(|board| ReplayDiff::from_boards(&board, &result.board));

    Ok(json!({
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
