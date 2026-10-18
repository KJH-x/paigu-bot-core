use super::*;
use crate::settings::AppConfig;
use sha2::{Digest, Sha256};

pub(super) fn source_token(
    cfg: &AppConfig,
    allocation: &AllocationSnapshot,
) -> Result<String, ServiceError> {
    let source = json!({
        "round": cfg.round,
        "rules": cfg.settlement,
        "members": cfg.members.cn_overrides,
        "whitelist_groups": cfg.gateway.whitelist_groups,
        "whitelist_members": cfg.gateway.whitelist_members,
        "allocation": allocation.item_allocations,
        "users": allocation.user_summaries,
    });
    let bytes = serde_json::to_vec(&source).map_err(|e| ServiceError::Internal(e.into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(super) fn response(
    draft: &SettlementDraft,
    cfg: &AppConfig,
    allocation: &AllocationSnapshot,
    include_draft: bool,
) -> Result<Value, ServiceError> {
    let token = source_token(cfg, allocation)?;
    let existing = draft.revision > 0 || !draft.order_table.packages.is_empty();
    let mut value = json!({
        "round_id": cfg.round.round_id,
        "config_revision": cfg.revision,
        "revision": draft.revision,
        "locked": draft.locked,
        "source_token": token,
        "needs_review": existing && draft.reviewed_source.as_deref() != Some(&token),
        "result_stale": draft.last_result.is_some() && draft.calculated_source.as_deref() != Some(&token),
        "issues": issues(&draft.order_table, &draft.order_numbers, allocation, &cfg.round),
    });
    if include_draft {
        value["draft"] =
            serde_json::to_value(draft).map_err(|e| ServiceError::Internal(e.into()))?;
    }
    Ok(value)
}

pub(super) fn ensure_source(expected: &str, current: &str) -> Result<(), ServiceError> {
    if expected != current {
        return Err(ServiceError::Conflict(json!({
            "error": "settlement_source_changed",
            "message": "排位、目录、权限或结算规则已变更；订单与输入保留，请重新核对",
            "source_token": current,
            "needs_review": true,
        })));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub(super) struct Issue {
    code: &'static str,
    message: String,
    package_index: Option<usize>,
    line_index: Option<usize>,
    field: &'static str,
    item_id: Option<String>,
    subitem_id: Option<String>,
}

pub(super) fn issues(
    table: &OrderTable,
    numbers: &BTreeMap<String, String>,
    allocation: &AllocationSnapshot,
    round: &RoundSettings,
) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut add = |code, message, package_index, line_index, field, item_id, subitem_id| {
        out.push(Issue {
            code,
            message,
            package_index,
            line_index,
            field,
            item_id,
            subitem_id,
        });
    };
    let mut package_ids = BTreeSet::new();
    let mut order_ids = BTreeSet::new();
    let mut prices = BTreeMap::new();
    let mut nonempty = false;
    for (pi, package) in table.packages.iter().enumerate() {
        if !package.lines.iter().any(|line| line.qty > 0) {
            continue;
        }
        nonempty = true;
        let name = if package.package_id.trim().is_empty() {
            "未命名包"
        } else {
            &package.package_id
        };
        if package.package_id.trim().is_empty()
            || !package_ids.insert(package.package_id.trim().to_owned())
        {
            add(
                "package_name",
                format!("第 {} 包：包名不能为空且必须唯一", pi + 1),
                Some(pi),
                None,
                "package_name",
                None,
                None,
            );
        }
        let number = numbers
            .get(&package.package_id)
            .map(|s| s.trim())
            .unwrap_or("");
        if number.is_empty() {
            add(
                "order_missing",
                format!("{name}：请填写订单号"),
                Some(pi),
                None,
                "order_number",
                None,
                None,
            );
        } else if !order_ids.insert(number.to_owned()) {
            add(
                "order_duplicate",
                format!("{name}：订单号 {number} 重复"),
                Some(pi),
                None,
                "order_number",
                None,
                None,
            );
        }
        for (li, line) in package
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.qty > 0)
        {
            let item = round.items.iter().find(|i| i.item_id == line.item_id);
            let label = item.map(|i| i.name.as_str()).unwrap_or(&line.item_id);
            if item.is_none()
                || line.subitem_id.as_ref().is_some_and(|id| {
                    !item.is_some_and(|i| i.subitems.iter().any(|s| &s.subitem_id == id))
                })
            {
                add(
                    "catalog_missing",
                    format!("{name}：{label} 的商品或款式已不在当前目录"),
                    Some(pi),
                    Some(li),
                    "item",
                    Some(line.item_id.clone()),
                    line.subitem_id.clone(),
                );
            }
            if !line.is_gift {
                if line.unit_price_cents <= 0 {
                    add(
                        "price_missing",
                        format!("{name}：{label} 需要大于零的标价"),
                        Some(pi),
                        Some(li),
                        "price",
                        Some(line.item_id.clone()),
                        line.subitem_id.clone(),
                    );
                }
                if let Some(price) = prices.insert(
                    (line.item_id.clone(), line.subitem_id.clone()),
                    line.unit_price_cents,
                ) {
                    if price != line.unit_price_cents {
                        add(
                            "price_conflict",
                            format!("{name}：{label} 与其他包标价不一致"),
                            Some(pi),
                            Some(li),
                            "price",
                            Some(line.item_id.clone()),
                            line.subitem_id.clone(),
                        );
                    }
                }
            }
        }
    }
    if !nonempty {
        add(
            "empty",
            "尚无可锁定的订单包".into(),
            None,
            None,
            "allocation",
            None,
            None,
        );
    }
    let completeness = check_completeness(table, allocation);
    for gap in completeness.missing.iter().chain(&completeness.extra) {
        let position = table.packages.iter().enumerate().find_map(|(pi, p)| {
            p.lines
                .iter()
                .position(|l| {
                    l.item_id == gap.item_id
                        && l.subitem_id == gap.subitem_id
                        && l.is_gift == gap.is_gift
                })
                .map(|li| (pi, li))
        });
        let label = round
            .items
            .iter()
            .find(|i| i.item_id == gap.item_id)
            .map(|i| i.name.as_str())
            .unwrap_or(&gap.item_id);
        add(
            "quantity_gap",
            format!(
                "{label}{}：排位 {} 件，订单 {} 件",
                gap.subitem_id
                    .as_ref()
                    .map(|id| round
                        .items
                        .iter()
                        .filter(|i| i.item_id == gap.item_id)
                        .flat_map(|i| &i.subitems)
                        .find(|s| &s.subitem_id == id)
                        .map(|s| format!(" / {}", s.name))
                        .unwrap_or_default())
                    .unwrap_or_default(),
                gap.expected,
                gap.actual
            ),
            position.map(|p| p.0),
            position.map(|p| p.1),
            if position.is_some() {
                "quantity"
            } else {
                "allocation"
            },
            Some(gap.item_id.clone()),
            gap.subitem_id.clone(),
        );
    }
    out
}
