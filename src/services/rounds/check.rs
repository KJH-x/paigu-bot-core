use crate::{round::RoundCategory, settings::RoundSettings};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn issue(level: &str, code: &str, message: String, where_: &str) -> Value {
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

pub(super) fn check_issues(round: &RoundSettings) -> Vec<Value> {
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
