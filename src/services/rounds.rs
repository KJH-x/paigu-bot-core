//! 轮次库服务（U3）：列表 / 新建 / 切换激活 / 校验 / 删除。

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::replay::session::{self, ReplayOverrides};
use crate::settings::rounds;
use crate::settings::RoundSettings;

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
}

/// `POST /api/rounds`：新建轮次（`copy_from` 存在则复制其商品/阶段）。
pub async fn create_round(
    state: &ApiState,
    body: CreateRoundBody,
) -> Result<Value, ServiceError> {
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
            let mut base = rounds::read_round(from).ok_or_else(|| {
                ServiceError::BadRequest(format!("copy_from 不存在：{from}"))
            })?;
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
        },
    };
    if let Some(title) = body.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        round.title = title.to_string();
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
            let cfg_now = state.cfg.get().await;
            let records = state
                .messages
                .read_all(&id)
                .await
                .map_err(ServiceError::from)?;
            let result =
                session::replay_messages(&cfg_now, &records, ReplayOverrides::default())
                    .await
                    .map_err(ServiceError::from)?;
            replay::remember_replay(&result);
            replayed = Some(replay::replay_result_json(&result));
        }
        _ => {}
    }

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

/// `POST /api/rounds/:id/check`：轮次结构校验。
pub async fn check_round(_state: &ApiState, id: &str) -> Result<Value, ServiceError> {
    let round = rounds::read_round(id.trim())
        .ok_or_else(|| ServiceError::NotFound(format!("轮次不存在：{id}")))?;
    let issues = check_issues(&round);
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
        return Err(ServiceError::BadRequest(
            "不能删除激活中的轮次".to_string(),
        ));
    }
    let path = rounds::round_path(&id);
    if !path.exists() {
        return Err(ServiceError::NotFound(format!("轮次不存在：{id}")));
    }
    std::fs::remove_file(&path).map_err(|e| ServiceError::Internal(e.into()))?;
    Ok(json!({ "ok": true, "removed": id }))
}

fn round_summary(round: &RoundSettings, active: bool, modified: Option<SystemTime>) -> Value {
    let variants: usize = round.items.iter().map(|it| it.variants.len()).sum();
    let updated_at = modified.map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    json!({
        "round_id": round.round_id,
        "title": round.title,
        "group_id": round.group_id,
        "items": round.items.len(),
        "variants": variants,
        "active": active,
        "updated_at": updated_at,
    })
}

fn issue(level: &str, code: &str, message: String, where_: &str) -> Value {
    json!({ "level": level, "code": code, "message": message, "where": where_ })
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
    // 跨商品冲突：item 级（商品名/别名）与 variant 级（变体名/别名）分别记 owner。
    let mut name_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut alias_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut variant_token_owner: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

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
        if it.variants.is_empty() && it.unit_price_cents == 0 {
            issues.push(issue(
                "warn",
                "missing_price",
                format!("商品 {} 缺少标价（0）", it.item_id),
                &where_item,
            ));
        }
        // 种类与变体一致性（U6）
        if let Some(cat) = it.category() {
            if cat.requires_variants() != it.has_variants() {
                issues.push(issue(
                    "error",
                    "category_variant_mismatch",
                    format!(
                        "商品 {} 种类 {} 与变体不一致（有变体必须是拼团/特典，无变体必须是单领/整盒）",
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
                        "商品 {} 的 class 与「有变体⇒A/无变体⇒B」推导不一致",
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
        // variant_id 只在**同一商品内**唯一（error）。
        let mut seen_variant_ids: BTreeSet<&str> = BTreeSet::new();
        for v in &it.variants {
            let vw = format!("variant:{}/{}", it.item_id, v.variant_id);
            if v.variant_id.trim().is_empty() {
                issues.push(issue(
                    "error",
                    "empty_variant_id",
                    format!("商品 {} 存在空 variant_id", it.item_id),
                    &where_item,
                ));
            } else if !seen_variant_ids.insert(v.variant_id.as_str()) {
                issues.push(issue(
                    "error",
                    "duplicate_variant_id",
                    format!(
                        "variant_id 在同一商品内重复：{}（商品 {}）",
                        v.variant_id, it.item_id
                    ),
                    &vw,
                ));
            }
            if v.name.trim().is_empty() {
                issues.push(issue(
                    "error",
                    "empty_variant_name",
                    format!("变体 {} 的 name 为空", v.variant_id),
                    &vw,
                ));
            } else {
                variant_token_owner
                    .entry(v.name.trim().to_string())
                    .or_default()
                    .insert(it.item_id.clone());
            }
            if v.unit_price_cents == 0 {
                issues.push(issue(
                    "warn",
                    "missing_variant_price",
                    format!("变体 {} 缺少标价（0）", v.variant_id),
                    &vw,
                ));
            }
            for alias in &v.aliases {
                let alias = alias.trim();
                if !alias.is_empty() {
                    variant_token_owner
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

    // 商品名：跨商品重复，或与别家商品别名/变体名冲突 → error。
    for (token, owners) in &name_owner {
        let hit = owners.len() > 1
            || alias_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false)
            || variant_token_owner
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
    // 商品别名：跨商品重复，或与别家商品名/变体名冲突 → error。
    for (token, owners) in &alias_owner {
        let hit = owners.len() > 1
            || name_owner
                .get(token)
                .map(|other| crosses(owners, other))
                .unwrap_or(false)
            || variant_token_owner
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
    // 变体名/别名：与别家**商品名/别名**冲突 → error；变体之间跨商品重名（角色名）不报错。
    for (token, owners) in &variant_token_owner {
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
                format!("变体名/别名与商品名冲突：{token}（{}）", owners_label(owners)),
                "variants",
            ));
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ItemConfig, VariantConfig};

    fn variant(variant_id: &str, unit_price_cents: i64) -> VariantConfig {
        VariantConfig {
            variant_id: variant_id.to_string(),
            name: variant_id.to_string(),
            unit_price_cents,
            adjust_cents: 0,
            capacity: None,
            aliases: vec![],
        }
    }

    fn item(item_id: &str, kind: &str, aliases: Vec<&str>, variants: Vec<VariantConfig>) -> ItemConfig {
        ItemConfig {
            item_id: item_id.to_string(),
            name: item_id.to_string(),
            kind: kind.to_string(),
            class: None,
            aliases: aliases.into_iter().map(str::to_string).collect(),
            unit_price_cents: 100,
            max_quantity: None,
            variants,
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
                item("a", "拼团", vec!["通用"], vec![variant("v1", 100)]),
                item("b", "单领", vec!["通用"], vec![]),
                item("c", "单领", vec![], vec![variant("v1", 0), variant("v1", 0)]),
            ],
        };
        let issues = check_issues(&round);
        let codes: Vec<&str> = issues.iter().filter_map(|i| i["code"].as_str()).collect();
        assert!(codes.contains(&"alias_conflict"));
        assert!(codes.contains(&"duplicate_variant_id"));
        assert!(codes.contains(&"category_variant_mismatch"));
        assert!(codes.contains(&"missing_variant_price"));
        assert!(!issues.iter().any(|i| i["level"] == "no_such_level"));
    }

    /// 真实 `config.example.json` 结构：多个商品复用 `v_jcl` 等 variant_id、
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
                item("a", "拼团", vec!["甲"], vec![variant("v1", 100)]),
                item("b", "单领", vec!["乙"], vec![]),
            ],
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
                item("a", "拼团", vec!["甲"], vec![variant("v1", 100)]),
                item("b", "单领", vec!["乙"], vec![]),
            ],
        };
        let issues = check_issues(&round);
        assert!(!issues.iter().any(|i| i["level"] == "error"));
    }
}
