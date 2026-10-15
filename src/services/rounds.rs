//! 轮次库服务（U3）：列表 / 新建 / 切换激活 / 校验 / 删除。

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::round::{PhaseWindow, RoundCategory};
use crate::settings::rounds;
use crate::settings::{ItemConfig, PriorityWindow, RoundSettings};

use super::{replay, ServiceError};

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

    let mut round = rounds::read_round(&id)
        .ok_or_else(|| ServiceError::NotFound(format!("轮次不存在：{id}")))?;
    round.round_id = id.clone();

    // 切换（continue）：仅更新激活轮次与配置，不动内存状态。
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.round = round;
    cfg.active_round_id = Some(id.clone());
    match state.cfg.put(cfg, revision).await {
        Ok(_) => {}
        Err(crate::settings::ConfigError::StaleRevision { actual, .. }) => {
            return Err(ServiceError::StaleRevision(actual));
        }
        Err(e) => return Err(ServiceError::Internal(e.into())),
    }

    let mut reset = false;
    let mut replayed: Option<Value> = None;
    match mode.as_str() {
        "fresh" => {
            state.pipeline.reset().await;
            reset = true;
        }
        "replay" => {
            state.pipeline.reset().await;
            reset = true;
            replayed = Some(replay::recompute(state).await.map_err(ServiceError::from)?);
        }
        _ => {}
    }
    crate::services::settlement::restore_persistent_lock(state).await?;

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

    rounds::write_round(&round).map_err(|e| ServiceError::Internal(e.into()))?;

    let mut revision = cfg.revision;
    let mut applied_to_active = false;
    if is_active {
        // 同步热载：`ConfigStore::put` 会一并落盘 config 与轮次文件并递增 revision。
        let mut next = cfg.clone();
        next.round = round.clone();
        match state.cfg.put(next, cfg.revision).await {
            Ok(rev) => {
                revision = rev;
                applied_to_active = true;
            }
            Err(crate::settings::ConfigError::StaleRevision { actual, .. }) => {
                return Err(ServiceError::StaleRevision(actual));
            }
            Err(e) => return Err(ServiceError::Internal(e.into())),
        }
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

fn issue(level: &str, code: &str, message: String, where_: &str) -> Value {
    json!({ "level": level, "code": code, "message": message, "where": where_ })
}

/// 轮次类别与群绑定的校验：**测试轮只绑定测试群，正常轮只绑定正常群**。
pub(crate) fn category_issues(round: &RoundSettings, test_group_id: Option<&str>) -> Vec<Value> {
    let mut issues = Vec::new();
    match round.category {
        RoundCategory::Test => match test_group_id {
            Some(test_group) if test_group == round.group_id => {}
            Some(test_group) => issues.push(issue(
                "error",
                "test_round_group_mismatch",
                format!(
                    "测试轮群号 {} 与测试群 {} 不一致",
                    round.group_id, test_group
                ),
                "round.category",
            )),
            None => issues.push(issue(
                "error",
                "test_round_group_mismatch",
                "未配置 gateway.test_group_id，无法作为测试轮".to_string(),
                "round.category",
            )),
        },
        RoundCategory::Normal => {
            if test_group_id == Some(round.group_id.as_str()) {
                issues.push(issue(
                    "error",
                    "normal_round_uses_test_group",
                    format!("正常轮不能绑定测试群 {}", round.group_id),
                    "round.category",
                ));
            }
        }
    }
    issues
}

/// 两个 owner 集合是否指向**不同商品**（用于跨商品冲突判定）。
fn crosses(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    b.iter().any(|x| !a.contains(x))
}

fn owners_label(owner: &BTreeSet<String>) -> String {
    owner.iter().cloned().collect::<Vec<_>>().join(", ")
}

fn check_issues(round: &RoundSettings) -> Vec<Value> {
    let mut issues = Vec::new();
    let mut item_ids: BTreeMap<&str, usize> = BTreeMap::new();
    // 跨商品冲突：item 级（商品名/别名）与 subitem 级（子项名/别名）分别记 owner。
    let mut name_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut alias_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut subitem_token_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for it in &round.items {
        let where_item = format!("item:{}", it.item_id);
        if it.item_id.trim().is_empty() {
            issues.push(issue(
                "error",
                "empty_item_id",
                "商品 item_id 为空".to_string(),
                "item:(empty)",
            ));
        } else {
            *item_ids.entry(it.item_id.as_str()).or_insert(0) += 1;
        }
        if it.name.trim().is_empty() {
            issues.push(issue(
                "error",
                "empty_name",
                format!("商品 {} 的 name 为空", it.item_id),
                &where_item,
            ));
        } else {
            name_owner
                .entry(it.name.trim().to_string())
                .or_default()
                .insert(it.item_id.clone());
        }
        // 价格缺失
        if it.subitems.is_empty() && it.unit_price_cents == 0 {
            issues.push(issue(
                "warn",
                "missing_price",
                format!("商品 {} 缺少标价（0）", it.item_id),
                &where_item,
            ));
        }
        // 种类与子项一致性（U6）
        if let Some(cat) = it.category() {
            if cat.requires_subitems() != it.has_subitems() {
                issues.push(issue(
                    "error",
                    "category_subitem_mismatch",
                    format!(
                        "商品 {} 种类 {} 与子项不一致（有子项必须是拼团/特典，无子项必须是单领/整盒）",
                        it.item_id,
                        cat.as_str()
                    ),
                    &where_item,
                ));
            }
        }
        // class 推导一致性（U6；显式 class 与推导不符时提示）
        if let Some(explicit) = it.class.as_deref().and_then(crate::round::ItemClass::parse) {
            if explicit != it.derived_class() {
                issues.push(issue(
                    "warn",
                    "class_derived_mismatch",
                    format!(
                        "商品 {} 的 class 与「有子项⇒A/无子项⇒B」推导不一致",
                        it.item_id
                    ),
                    &where_item,
                ));
            }
        }
        for alias in &it.aliases {
            let alias = alias.trim();
            if !alias.is_empty() {
                alias_owner
                    .entry(alias.to_string())
                    .or_default()
                    .insert(it.item_id.clone());
            }
        }
        // subitem_id 只在**同一商品内**唯一（error）。
        let mut seen_subitem_ids: BTreeSet<&str> = BTreeSet::new();
        for v in &it.subitems {
            let vw = format!("subitem:{}/{}", it.item_id, v.subitem_id);
            if v.subitem_id.trim().is_empty() {
                issues.push(issue(
                    "error",
                    "empty_subitem_id",
                    format!("商品 {} 存在空 subitem_id", it.item_id),
                    &where_item,
                ));
            } else if !seen_subitem_ids.insert(v.subitem_id.as_str()) {
                issues.push(issue(
                    "error",
                    "duplicate_subitem_id",
                    format!(
                        "subitem_id 在同一商品内重复：{}（商品 {}）",
                        v.subitem_id, it.item_id
                    ),
                    &vw,
                ));
            }
            if v.name.trim().is_empty() {
                issues.push(issue(
                    "error",
                    "empty_subitem_name",
                    format!("子项 {} 的 name 为空", v.subitem_id),
                    &vw,
                ));
            } else {
                subitem_token_owner
                    .entry(v.name.trim().to_string())
                    .or_default()
                    .insert(it.item_id.clone());
            }
            if v.unit_price_cents == 0 {
                issues.push(issue(
                    "warn",
                    "missing_subitem_price",
                    format!("子项 {} 缺少标价（0）", v.subitem_id),
                    &vw,
                ));
            }
            for alias in &v.aliases {
                let alias = alias.trim();
                if !alias.is_empty() {
                    subitem_token_owner
                        .entry(alias.to_string())
                        .or_default()
                        .insert(it.item_id.clone());
                }
            }
        }
    }

    for (item_id, count) in item_ids {
        if count > 1 {
            issues.push(issue(
                "error",
                "duplicate_item_id",
                format!("item_id 重复：{item_id}（{count} 次）"),
                &format!("item:{item_id}"),
            ));
        }
    }

    // 商品名：跨商品重复，或与别家商品别名/子项名冲突 → error。
    for (token, owners) in &name_owner {
        let hit = owners.len() > 1
            || alias_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false)
            || subitem_token_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false);
        if hit {
            issues.push(issue(
                "error",
                "name_conflict",
                format!("商品名跨商品冲突：{token}（{}）", owners_label(owners)),
                &format!("item:{token}"),
            ));
        }
    }
    // 商品别名：跨商品重复，或与别家商品名/子项名冲突 → error。
    for (token, owners) in &alias_owner {
        let hit = owners.len() > 1
            || name_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false)
            || subitem_token_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false);
        if hit {
            issues.push(issue(
                "error",
                "alias_conflict",
                format!("别名跨商品冲突：{token}（{}）", owners_label(owners)),
                "aliases",
            ));
        }
    }
    // 子项名/别名：与别家**商品名/别名**冲突 → error；子项之间跨商品重名（角色名）不报错。
    for (token, owners) in &subitem_token_owner {
        let hit = name_owner
            .get(token)
            .map(|other| crosses(owners, other))
            .unwrap_or(false)
            || alias_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false);
        if hit {
            issues.push(issue(
                "error",
                "alias_conflict",
                format!(
                    "子项名/别名与商品名冲突：{token}（{}）",
                    owners_label(owners)
                ),
                "subitems",
            ));
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ItemConfig, SubitemConfig};

    fn subitem(subitem_id: &str, unit_price_cents: i64) -> SubitemConfig {
        SubitemConfig {
            subitem_id: subitem_id.to_string(),
            name: subitem_id.to_string(),
            unit_price_cents,
            adjust_cents: 0,
            capacity: None,
            aliases: vec![],
        }
    }

    fn item(
        item_id: &str,
        kind: &str,
        aliases: Vec<&str>,
        subitems: Vec<SubitemConfig>,
    ) -> ItemConfig {
        ItemConfig {
            item_id: item_id.to_string(),
            name: item_id.to_string(),
            kind: kind.to_string(),
            class: None,
            aliases: aliases.into_iter().map(str::to_string).collect(),
            unit_price_cents: 100,
            max_quantity: None,
            box_item_id: None,
            subitems,
        }
    }

    #[test]
    fn check_flags_duplicates_aliases_and_category_mismatch() {
        let round = RoundSettings {
            round_id: "t".to_string(),
            title: "t".to_string(),
            group_id: "g".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![
                item("a", "拼团", vec!["通用"], vec![subitem("v1", 100)]),
                item("b", "单领", vec!["通用"], vec![]),
                item(
                    "c",
                    "单领",
                    vec![],
                    vec![subitem("v1", 0), subitem("v1", 0)],
                ),
            ],
            category: RoundCategory::Normal,
        };
        let issues = check_issues(&round);
        let codes: Vec<&str> = issues.iter().filter_map(|i| i["code"].as_str()).collect();
        assert!(codes.contains(&"alias_conflict"));
        assert!(codes.contains(&"duplicate_subitem_id"));
        assert!(codes.contains(&"category_subitem_mismatch"));
        assert!(codes.contains(&"missing_subitem_price"));
        assert!(!issues.iter().any(|i| i["level"] == "no_such_level"));
    }

    /// 真实 `config.example.json` 结构：多个商品复用 `v_jcl` 等 subitem_id、
    /// 且角色名可在多商品重复，不应报 error（§U7/§U8 口径）。
    #[test]
    fn check_ok_for_example_config_structure() {
        let cfg = crate::settings::default_config();
        let issues = check_issues(&cfg.round);
        assert!(
            !issues.iter().any(|i| i["level"] == "error"),
            "真实配置不应有 error：{issues:?}"
        );
    }

    #[test]
    fn check_flags_cross_item_name_and_alias_conflicts() {
        let round = RoundSettings {
            round_id: "t".to_string(),
            title: "t".to_string(),
            group_id: "g".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![
                item("a", "拼团", vec!["甲"], vec![subitem("v1", 100)]),
                item("b", "单领", vec!["乙"], vec![]),
            ],
            category: RoundCategory::Normal,
        };
        // 让 b 的别名与 a 的商品名冲突。
        let mut round = round;
        round.items[1].aliases = vec!["a".to_string()];
        let issues = check_issues(&round);
        // item() 的 name = item_id，此处应命中 name_conflict（"a" 同名）。
        assert!(
            issues.iter().any(|i| i["code"] == "name_conflict"),
            "{issues:?}"
        );
    }

    #[test]
    fn check_ok_for_consistent_round() {
        let round = RoundSettings {
            round_id: "t".to_string(),
            title: "t".to_string(),
            group_id: "g".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![
                item("a", "拼团", vec!["甲"], vec![subitem("v1", 100)]),
                item("b", "单领", vec!["乙"], vec![]),
            ],
            category: RoundCategory::Normal,
        };
        let issues = check_issues(&round);
        assert!(!issues.iter().any(|i| i["level"] == "error"));
    }

    #[test]
    fn setup_merge_keeps_absent_and_replaces_provided() {
        let base = RoundSettings {
            round_id: "r0".to_string(),
            title: "旧标题".to_string(),
            group_id: "123456789".to_string(),
            priority_users: vec!["u1".to_string()],
            priority_window: Some(PriorityWindow {
                start_ms: 1,
                end_ms: 2,
            }),
            phases: vec![],
            items: vec![item("a", "单领", vec![], vec![])],
            category: RoundCategory::Normal,
        };

        // 全缺省：仅 `round_id` 归一为路径 id，其余保留。
        let same = merge_setup("r1", base.clone(), &SetupBody::default());
        assert_eq!(same.round_id, "r1");
        assert_eq!(same.title, "旧标题");
        assert_eq!(same.group_id, "123456789");
        assert_eq!(same.priority_users, vec!["u1".to_string()]);
        assert!(same.priority_window.is_some());
        assert_eq!(same.items.len(), 1);

        // 提供即整体替换（列表可清空）。
        let body = SetupBody {
            title: Some(" 新标题 ".to_string()),
            priority_users: Some(vec![]),
            items: Some(vec![item(
                "b",
                "拼团",
                vec!["甲"],
                vec![subitem("v1", 100)],
            )]),
            ..SetupBody::default()
        };
        let changed = merge_setup("r1", base, &body);
        assert_eq!(changed.title, "新标题");
        assert!(changed.priority_users.is_empty());
        assert_eq!(changed.items.len(), 1);
        assert_eq!(changed.items[0].item_id, "b");
        // 未提供字段保留。
        assert_eq!(changed.group_id, "123456789");
    }

    #[test]
    fn clean_opt_trims_and_drops_blank() {
        assert_eq!(clean_opt(None::<&str>), None);
        assert_eq!(clean_opt(Some("   ")), None);
        assert_eq!(clean_opt(Some(" 甲 ")), Some("甲".to_string()));
    }

    #[test]
    fn category_binding_checks_test_and_normal() {
        let base = RoundSettings {
            round_id: "t".to_string(),
            title: "t".to_string(),
            group_id: "111".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![],
            category: RoundCategory::Test,
        };
        // 测试轮 + 匹配测试群 → 无 issue。
        assert!(category_issues(&base, Some("111")).is_empty());
        // 测试轮 + 不匹配 → error。
        assert!(category_issues(&base, Some("222"))
            .iter()
            .any(|i| i["code"] == "test_round_group_mismatch"));
        // 测试轮 + 未配置测试群 → error。
        assert!(category_issues(&base, None)
            .iter()
            .any(|i| i["level"] == "error"));

        let normal = RoundSettings {
            category: RoundCategory::Normal,
            ..base
        };
        // 正常轮 + 正常群 → 无 issue。
        assert!(category_issues(&normal, Some("222")).is_empty());
        // 正常轮 + 测试群 → error。
        assert!(category_issues(&normal, Some("111"))
            .iter()
            .any(|i| i["code"] == "normal_round_uses_test_group"));
    }

    #[test]
    fn merge_setup_carries_category() {
        let base = RoundSettings {
            round_id: "r0".to_string(),
            title: "t".to_string(),
            group_id: "111".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![],
            category: RoundCategory::Test,
        };
        let same = merge_setup("r1", base.clone(), &SetupBody::default());
        assert_eq!(same.category, RoundCategory::Test);
        let body = SetupBody {
            category: Some(RoundCategory::Normal),
            ..SetupBody::default()
        };
        assert_eq!(
            merge_setup("r1", base, &body).category,
            RoundCategory::Normal
        );
    }
}
