use super::ServiceError;
use crate::{api::ApiState, settings::AppConfig};
use serde_json::{json, Value};

pub fn business_inputs(cfg: &AppConfig) -> Value {
    crate::messages::transactions::policies(cfg)
}

/// Caller holds the common writer. Prepare the complete next view before the
/// recoverable config/round commit, then publish only that prepared generation.
pub(crate) async fn apply_locked(
    state: &ApiState,
    mut next: AppConfig,
    expected: u64,
) -> Result<Value, ServiceError> {
    let previous = state.cfg.get().await;
    if previous.revision != expected {
        return Err(ServiceError::StaleRevision(previous.revision));
    }
    if state.pipeline.is_locked().await
        || super::settlement::round_locked(&previous.round.round_id)?
    {
        return Err(ServiceError::Locked(
            "轮次已锁定，请先解锁再修改配置".into(),
        ));
    }
    if !crate::settings::rounds::is_valid_round_id(&next.round.round_id) {
        return Err(ServiceError::BadRequest("轮次标识无效".into()));
    }
    if !next.gateway.whitelist_groups.contains(&next.round.group_id) {
        return Err(ServiceError::Unprocessable(
            "轮次群号不在接入白名单中".into(),
        ));
    }
    if let Some(issue) =
        super::rounds::category_issues(&next.round, next.gateway.test_group_id.as_deref())
            .iter()
            .find(|i| i["level"] == "error")
    {
        return Err(ServiceError::Unprocessable(
            issue["message"]
                .as_str()
                .unwrap_or("轮次类别与群绑定不匹配")
                .into(),
        ));
    }
    let business_changed = business_inputs(&previous) != business_inputs(&next);
    next.revision = expected.saturating_add(1);
    next.active_round_id = Some(next.round.round_id.clone());
    let records = state.messages.read_only(&next.round.round_id).await?;
    let persistent_lock = super::settlement::round_locked(&next.round.round_id)?;
    let computed = if business_changed {
        Some(
            crate::replay::session::replay(
                &state.messages,
                &next,
                crate::replay::session::ReplayOverrides::default(),
            )
            .await?,
        )
    } else {
        None
    };
    let revision = state
        .cfg
        .put(next.clone(), expected)
        .await
        .map_err(|e| match e {
            crate::settings::ConfigError::StaleRevision { actual, .. } => {
                ServiceError::StaleRevision(actual)
            }
            other => ServiceError::Internal(other.into()),
        })?;
    if previous.round.round_id != next.round.round_id {
        state
            .pipeline
            .switch_round_locked(&previous.round.round_id, &next.round.round_id, false)
            .await;
    }
    if let Some(computed) = computed {
        state.pipeline.apply_replay(&computed, &records).await;
        super::replay::remember_replay(&computed);
        state
            .pipeline
            .restore_settlement_lock(persistent_lock)
            .await;
    }
    Ok(json!({"config":next,"revision":revision,"recomputed":business_changed}))
}

pub async fn reload(state: &ApiState, force: bool) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let previous = state.cfg.get().await;
    let mut next = state.cfg.read_candidate()?;
    let disk_revision = next.revision;
    // Our own atomic writes must not create an endless watcher/revision loop.
    next.revision = previous.revision;
    if !force
        && disk_revision <= previous.revision
        && serde_json::to_value(&next).ok() == serde_json::to_value(&previous).ok()
    {
        return Ok(json!({"config":previous,"revision":previous.revision,"recomputed":false}));
    }
    apply_locked(state, next, previous.revision).await
}
