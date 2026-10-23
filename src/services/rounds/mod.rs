//! 轮次库服务（U3）：列表 / 新建 / 切换激活 / 校验 / 删除。

use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::round::{PhaseWindow, RoundCategory};
use crate::settings::rounds;
use crate::settings::{ItemConfig, PriorityWindow, RoundSettings};

use super::{replay, ServiceError};

mod check;
pub(crate) use check::category_issues;
use check::{check_issues, issue};

#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod tests;

/// `GET /api/rounds`
pub async fn list_rounds(state: &ApiState) -> Value {
    let cfg = state.cfg.get().await;
    let active = cfg
        .active_round_id
        .clone()
        .unwrap_or_else(|| cfg.round.round_id.clone());

    let mut rounds: Vec<Value> = Vec::new();
    for info in rounds::list_round_files() {
        let Some(round) = rounds::read_round(&info.round_id) else {
            continue;
        };
        rounds.push(round_summary(
            &round,
            info.round_id == active,
            info.modified,
        ));
    }
    let present = rounds
        .iter()
        .any(|r| r["round_id"].as_str() == Some(active.as_str()));
    if !present && !active.trim().is_empty() {
        rounds.push(round_summary(&cfg.round, true, None));
    }

    json!({ "rounds": rounds, "active_round_id": active })
}

#[derive(Deserialize)]
pub struct CreateRoundBody {
    #[serde(default)]
    pub round_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub copy_from: Option<String>,
    /// 轮次类别（缺省沿用 `copy_from` 或 `normal`）。
    #[serde(default)]
    pub category: Option<RoundCategory>,
}

/// `POST /api/rounds`：新建轮次（`copy_from` 存在则复制其商品/阶段）。
pub async fn create_round(state: &ApiState, body: CreateRoundBody) -> Result<Value, ServiceError> {
    let round_id = body.round_id.trim().to_string();
    if !rounds::is_valid_round_id(&round_id) {
        return Err(ServiceError::BadRequest(
            "round_id 非法（非空、≤64 且仅允许字母/数字/中文/_-）".to_string(),
        ));
    }
    if rounds::read_round(&round_id).is_some() {
        return Err(ServiceError::BadRequest(format!(
            "round_id 已存在：{round_id}"
        )));
    }
    let cfg = state.cfg.get().await;

    let mut round = match body
        .copy_from
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(from) => {
            let mut base = rounds::read_round(from)
                .ok_or_else(|| ServiceError::BadRequest(format!("copy_from 不存在：{from}")))?;
            base.round_id = round_id.clone();
            base
        }
        None => RoundSettings {
            round_id: round_id.clone(),
            title: round_id.clone(),
            group_id: cfg.round.group_id.clone(),
            priority_users: Vec::new(),
            priority_window: None,
            phases: Vec::new(),
            items: Vec::new(),
            category: RoundCategory::Normal,
        },
    };
    if let Some(title) = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        round.title = title.to_string();
    }
    if let Some(category) = body.category {
        round.category = category;
    }
    if let Some(issue) = category_issues(&round, cfg.gateway.test_group_id.as_deref())
        .into_iter()
        .find(|issue| issue["level"] == "error")
    {
        return Err(ServiceError::Unprocessable(
            issue["message"]
                .as_str()
                .unwrap_or("轮次类别与群绑定不一致")
                .to_string(),
        ));
    }
    if !cfg
        .gateway
        .whitelist_groups
        .iter()
        .any(|group| group == &round.group_id)
    {
        return Err(ServiceError::Unprocessable(format!(
            "复制出的轮次群号 {} 不在网关群白名单中，请先修正来源轮次",
            round.group_id
        )));
    }

    rounds::write_round(&round).map_err(|e| ServiceError::Internal(e.into()))?;
    Ok(json!({ "ok": true, "round": round_summary(&round, false, None) }))
}

#[derive(Deserialize, Default)]
pub struct ActivateBody {
    /// `continue`（默认）| `fresh` | `replay`。
    #[serde(default)]
    pub mode: Option<String>,
}

/// `POST /api/rounds/:id/activate`：切换激活（与重放解耦）。
pub async fn activate_round(
    state: &ApiState,
    id: &str,
    body: ActivateBody,
) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let id = id.trim().to_string();
    let mode = body
        .mode
        .unwrap_or_else(|| "continue".to_string())
        .trim()
        .to_ascii_lowercase();
    if !matches!(mode.as_str(), "continue" | "fresh" | "replay") {
        return Err(ServiceError::BadRequest(
            "mode 必须是 continue|fresh|replay".to_string(),
        ));
    }
    let _ingress_fence = if mode == "fresh" {
        Some(state.messages.ingress_commit_guard().await)
    } else {
        None
    };
    if mode == "fresh" {
        super::configuration::ensure_no_pending_ingress(&state.messages, &id)?;
    }

    let mut round = rounds::read_round(&id)
        .ok_or_else(|| ServiceError::NotFound(format!("轮次不存在：{id}")))?;
    round.round_id = id.clone();

    // Prepare every fallible read/replay before committing the selected round.
    // A broken target journal must not clear the previous board or activate it.
    let mut cfg = state.cfg.get().await;
    let previous_round = cfg.round.round_id.clone();
    let revision = cfg.revision;
    cfg.round = round;
    cfg.active_round_id = Some(id.clone());
    if !cfg.gateway.whitelist_groups.contains(&cfg.round.group_id) {
        return Err(ServiceError::Unprocessable(
            "轮次群号不在接入白名单中".into(),
        ));
    }
    if let Some(issue) = category_issues(&cfg.round, cfg.gateway.test_group_id.as_deref())
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
    let persistent_lock = super::settlement::round_locked(&id)?;
    cfg.revision = revision.saturating_add(1);
    let fresh_records = if mode == "fresh" {
        Some(state.messages.read_ingress_records(&id).await?)
    } else {
        None
    };
    let cold_continue =
        mode == "continue" && previous_round != id && !state.pipeline.has_parked_round(&id).await;
    let prepared = if mode == "replay" || cold_continue {
        let records = state.messages.read_only(&id).await?;
        let (result, _) = crate::replay::computed::replay(
            &state.messages,
            &cfg,
            if mode == "replay" {
                crate::replay::computed::ReadPolicy::Force
            } else {
                crate::replay::computed::ReadPolicy::Prefer
            },
        )
        .await?;
        Some((records, result))
    } else {
        None
    };
    match state.cfg.put(cfg.clone(), revision).await {
        Ok(_) => {}
        Err(crate::settings::ConfigError::StaleRevision { actual, .. }) => {
            return Err(ServiceError::StaleRevision(actual));
        }
        Err(e) => return Err(ServiceError::Internal(e.into())),
    }

    let mut reset = false;
    let mut replayed: Option<Value> = None;
    state
        .pipeline
        .switch_round_locked(&previous_round, &id, mode != "continue")
        .await;
    match mode.as_str() {
        "fresh" => {
            reset = true;
            state
                .pipeline
                .fresh_cut_locked(
                    fresh_records
                        .as_ref()
                        .expect("fresh prepared before config commit"),
                )
                .await;
            crate::replay::computed::invalidate(&state.messages, &id);
        }
        "replay" => {
            reset = true;
            let (records, result) = prepared.expect("replay prepared before config commit");
            state.pipeline.apply_replay(&cfg, &result, &records).await;
            replayed = Some(replay::replay_result_json(&result));
        }
        _ => {
            if let Some((records, result)) = prepared {
                state.pipeline.apply_replay(&cfg, &result, &records).await;
            }
        }
    }
    state
        .pipeline
        .restore_settlement_lock(persistent_lock)
        .await;

    let (version, _) = state.pipeline.board().await;
    Ok(json!({
        "ok": true,
        "active_round_id": id,
        "mode": mode,
        "reset": reset,
        "replayed": replayed,
        "version": version,
    }))
}

/// `POST /api/rounds/:id/setup` 请求体：`RoundSettings` 形态（缺省字段保留既有值，
/// 列表字段提供即整体替换）+ 可选 `dry_run`（仅校验不落盘）。
#[derive(Deserialize, Default)]
pub struct SetupBody {
    /// 若提供须与路径 `:id` 一致。
    #[serde(default)]
    pub round_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub priority_users: Option<Vec<String>>,
    #[serde(default)]
    pub priority_window: Option<PriorityWindow>,
    #[serde(default)]
    pub phases: Option<Vec<PhaseWindow>>,
    #[serde(default)]
    pub items: Option<Vec<ItemConfig>>,
    /// 轮次类别（缺省保留现值）。
    #[serde(default)]
    pub category: Option<RoundCategory>,
    /// 仅校验并返回 `issues`，不写盘（默认 false）。
    #[serde(default)]
    pub dry_run: bool,
}

/// `POST /api/rounds/:id/setup`：**开团前填写**（Phase 0 档案）。
///
/// 请求体为 `RoundSettings` 形态（`title`/`group_id`/`priority_users`/`priority_window`/
/// `phases`/`items`；缺省字段保留既有值，列表字段提供即整体替换）：写入
/// `data/rounds/<id>.json`；若该轮次为**激活轮次**，同步热载到运行配置
/// （`config/app.json` 的 `round`，写入前后均落盘轮次文件）。
/// `dry_run=true` 时只校验并返回 `issues`。返回
/// `{ok,dry_run,written,applied_to_active,revision?,issues,round?}`（`ok` = 无 error 级 issue）。
pub async fn setup_round(
    state: &ApiState,
    id: &str,
    body: SetupBody,
) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let id = id.trim().to_string();
    if !rounds::is_valid_round_id(&id) {
        return Err(ServiceError::BadRequest(
            "round_id 非法（非空、≤64 且仅允许字母/数字/中文/_-）".to_string(),
        ));
    }
    if let Some(body_id) = clean_opt(body.round_id.as_deref()) {
        if body_id != id {
            return Err(ServiceError::BadRequest(format!(
                "请求体 round_id（{body_id}）与路径（{id}）不一致"
            )));
        }
    }

    let cfg = state.cfg.get().await;
    let active = cfg
        .active_round_id
        .clone()
        .unwrap_or_else(|| cfg.round.round_id.clone());
    let is_active = id == active || id == cfg.round.round_id;

    let base = rounds::read_round(&id).unwrap_or_else(|| {
        if id == cfg.round.round_id {
            cfg.round.clone()
        } else {
            RoundSettings {
                round_id: id.clone(),
                title: id.clone(),
                group_id: cfg.round.group_id.clone(),
                priority_users: Vec::new(),
                priority_window: None,
                phases: Vec::new(),
                items: Vec::new(),
                category: RoundCategory::Normal,
            }
        }
    });

    let previous_binding = (base.group_id.clone(), base.category);
    let round = merge_setup(&id, base, &body);

    let mut issues = check_issues(&round);
    if !cfg
        .gateway
        .whitelist_groups
        .iter()
        .any(|group| group == &round.group_id)
    {
        issues.push(issue(
            "error",
            "group_not_whitelisted",
            format!("轮次群号 {} 不在网关群白名单中", round.group_id),
            "round.group_id",
        ));
    }
    issues.extend(category_issues(
        &round,
        cfg.gateway.test_group_id.as_deref(),
    ));
    let ok = !issues.iter().any(|i| i["level"] == "error");

    if body.dry_run {
        return Ok(json!({
            "ok": ok,
            "round_id": id,
            "dry_run": true,
            "written": false,
            "applied_to_active": false,
            "issues": issues,
        }));
    }

    if is_active && crate::services::settlement::round_locked(&id).unwrap_or(false) {
        return Err(ServiceError::Locked(
            "轮次已锁定，不能修改开团档案".to_string(),
        ));
    }

    if !is_active {
        let _fence = state.messages.ingress_commit_guard().await;
        if previous_binding != (round.group_id.clone(), round.category) {
            super::configuration::ensure_no_pending_ingress(&state.messages, &id)?;
        }
        rounds::write_round(&round).map_err(|e| ServiceError::Internal(e.into()))?;
    }

    let mut revision = cfg.revision;
    let mut applied_to_active = false;
    if is_active {
        // 同步热载：`ConfigStore::put` 会一并落盘 config 与轮次文件并递增 revision。
        let mut next = cfg.clone();
        next.round = round.clone();
        let saved = super::configuration::apply_locked(state, next, cfg.revision).await?;
        revision = saved["revision"].as_u64().unwrap_or(cfg.revision);
        applied_to_active = true;
    }

    Ok(json!({
        "ok": ok,
        "round_id": id,
        "dry_run": false,
        "written": true,
        "applied_to_active": applied_to_active,
        "revision": revision,
        "issues": issues,
        "round": round_summary(&round, is_active, None),
    }))
}

/// 合并 POST 体：`title`/`group_id` 缺省保留现值；`priority_users`/`phases`/`items`
/// 提供即整体替换、缺省保留；`priority_window` 提供即替换、缺省保留。`round_id` 以路径为准。
fn merge_setup(id: &str, base: RoundSettings, body: &SetupBody) -> RoundSettings {
    RoundSettings {
        round_id: id.to_string(),
        title: clean_opt(body.title.as_deref()).unwrap_or(base.title),
        group_id: clean_opt(body.group_id.as_deref()).unwrap_or(base.group_id),
        priority_users: body.priority_users.clone().unwrap_or(base.priority_users),
        priority_window: body.priority_window.clone().or(base.priority_window),
        phases: body.phases.clone().unwrap_or(base.phases),
        items: body.items.clone().unwrap_or(base.items),
        category: body.category.unwrap_or(base.category),
    }
}

fn clean_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `POST /api/rounds/:id/check`：轮次结构校验。
pub async fn check_round(state: &ApiState, id: &str) -> Result<Value, ServiceError> {
    let round = rounds::read_round(id.trim())
        .ok_or_else(|| ServiceError::NotFound(format!("轮次不存在：{id}")))?;
    let mut issues = check_issues(&round);
    let cfg = state.cfg.get().await;
    if !cfg
        .gateway
        .whitelist_groups
        .iter()
        .any(|group| group == &round.group_id)
    {
        issues.push(issue(
            "error",
            "group_not_whitelisted",
            format!("轮次群号 {} 不在网关群白名单中", round.group_id),
            "round.group_id",
        ));
    }
    issues.extend(category_issues(
        &round,
        cfg.gateway.test_group_id.as_deref(),
    ));
    let ok = !issues.iter().any(|i| i["level"] == "error");
    Ok(json!({ "ok": ok, "issues": issues }))
}

/// `DELETE /api/rounds/:id`：删除轮次（激活中的轮次拒绝）。
pub async fn delete_round(state: &ApiState, id: &str) -> Result<Value, ServiceError> {
    let id = id.trim().to_string();
    let cfg = state.cfg.get().await;
    let active = cfg
        .active_round_id
        .clone()
        .unwrap_or_else(|| cfg.round.round_id.clone());
    if id == active || id == cfg.round.round_id {
        return Err(ServiceError::BadRequest("不能删除激活中的轮次".to_string()));
    }
    let path = rounds::round_path(&id);
    if !path.exists() {
        return Err(ServiceError::NotFound(format!("轮次不存在：{id}")));
    }
    let _fence = state.messages.ingress_commit_guard().await;
    super::configuration::ensure_no_pending_ingress(&state.messages, &id)?;
    std::fs::remove_file(&path).map_err(|e| ServiceError::Internal(e.into()))?;
    Ok(json!({ "ok": true, "removed": id }))
}

fn round_summary(round: &RoundSettings, active: bool, modified: Option<SystemTime>) -> Value {
    let subitems: usize = round.items.iter().map(|it| it.subitems.len()).sum();
    let updated_at = modified.map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    json!({
        "round_id": round.round_id,
        "title": round.title,
        "group_id": round.group_id,
        "category": round.category.as_str(),
        "items": round.items.len(),
        "subitems": subitems,
        "active": active,
        "updated_at": updated_at,
    })
}
