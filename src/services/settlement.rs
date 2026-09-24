use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::domain::allocation::{ItemAllocation, SingleAllocation};
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;
use crate::planner::{self, PlanLimits, PlanRequest};
use crate::settings::RoundSettings;
use crate::settlement::{
    check_completeness, evaluate, order_table_from_allocation, CompletenessReport, OrderTable,
    PricingEntry, PricingMode, SettlementConfig, SettlementResult, UnitPrice,
};

use super::ServiceError;

/// §U6：把目录变体的 `adjust_cents`（B）合成为结算定价（`mode=AdjustBy`，精确匹配
/// `item_id`/`variant_id`），使 `evaluate` 的「调后价」列反映 C = A + B。
///
/// **仅内存合并，不持久化**。已有同 `(item_id, variant_id)` 的显式定价优先保留，
/// 以便配置/覆盖中的显式定价仍以显式为准；请求自带 `pricing` 时整体以请求为准（不调用本函数）。
pub fn merge_catalog_pricing(
    mut config: SettlementConfig,
    round: &RoundSettings,
) -> SettlementConfig {
    for item in &round.items {
        for variant in &item.variants {
            if variant.adjust_cents == 0 {
                continue;
            }
            let exists = config.pricing.iter().any(|entry| {
                entry.item_id == item.item_id
                    && entry.variant_id.as_deref() == Some(variant.variant_id.as_str())
            });
            if exists {
                continue;
            }
            config.pricing.push(PricingEntry {
                item_id: item.item_id.clone(),
                variant_id: Some(variant.variant_id.clone()),
                mode: PricingMode::AdjustBy,
                value: variant.adjust_cents,
            });
        }
    }
    config
}

pub async fn get_settlement_config(state: &ApiState) -> Value {
    let cfg = state.cfg.get().await;
    json!({ "config": cfg.settlement, "revision": cfg.revision })
}

#[derive(Deserialize)]
pub struct SettlementConfigBody {
    pub config: SettlementConfig,
    pub revision: u64,
}

pub async fn put_settlement_config(
    state: &ApiState,
    body: SettlementConfigBody,
) -> Result<Value, ServiceError> {
    let mut cfg = state.cfg.get().await;
    cfg.settlement = body.config;
    match state.cfg.put(cfg, body.revision).await {
        Ok(revision) => {
            let cfg = state.cfg.get().await;
            Ok(json!({ "config": cfg.settlement, "revision": revision }))
        }
        Err(crate::settings::ConfigError::StaleRevision { actual, .. }) => {
            Err(ServiceError::StaleRevision(actual))
        }
        Err(err) => Err(ServiceError::Internal(err.into())),
    }
}

#[derive(Deserialize)]
pub struct EvaluateBody {
    pub order_table: OrderTable,
    #[serde(default)]
    pub config: Option<SettlementConfig>,
    /// 提供后即启用「排包完成」校验（未完成 → 拒绝计算）。
    #[serde(default)]
    pub allocation: Option<AllocationSnapshot>,
}

pub async fn evaluate_order(
    state: &ApiState,
    body: EvaluateBody,
) -> Result<SettlementResult, ServiceError> {
    let config = match body.config {
        // 请求显式带 config（含 pricing）→ 以请求为准，不合并目录调价。
        Some(config) => config,
        None => {
            let cfg = state.cfg.get().await;
            merge_catalog_pricing(cfg.settlement, &cfg.round)
        }
    };
    if let Some(allocation) = &body.allocation {
        let report = check_completeness(&body.order_table, allocation);
        if !report.complete {
            return Err(ServiceError::BadRequest(format!(
                "排包未完成，拒绝计算：{}",
                if report.messages.is_empty() {
                    "数量与排谷结果不一致".to_string()
                } else {
                    report.messages.join("；")
                }
            )));
        }
    }
    Ok(evaluate(&config, &body.order_table))
}

#[derive(Deserialize)]
pub struct CompletenessBody {
    pub order_table: OrderTable,
    #[serde(default)]
    pub allocation: Option<AllocationSnapshot>,
}

pub async fn completeness_check(
    state: &ApiState,
    body: CompletenessBody,
) -> Result<CompletenessReport, ServiceError> {
    let allocation = match body.allocation {
        Some(allocation) => allocation,
        None => current_allocation(state).await?,
    };
    Ok(check_completeness(&body.order_table, &allocation))
}

#[derive(Deserialize)]
pub struct PlanBody {
    pub strategy: String,
    #[serde(default)]
    pub order_table: Option<OrderTable>,
    #[serde(default)]
    pub allocation: Option<AllocationSnapshot>,
    #[serde(default)]
    pub config: Option<SettlementConfig>,
    #[serde(default)]
    pub prices: Vec<UnitPrice>,
    #[serde(default)]
    pub limits: Option<PlanLimits>,
}

pub async fn plan_order(state: &ApiState, body: PlanBody) -> Result<Value, ServiceError> {
    let strategy_name = body.strategy.clone();
    let strategy = parse_strategy(&strategy_name).map_err(ServiceError::BadRequest)?;
    let cfg_now = state.cfg.get().await;
    let config = match body.config {
        Some(config) => config,
        None => merge_catalog_pricing(cfg_now.settlement.clone(), &cfg_now.round),
    };
    // 标价表：请求未带则取「商品目录」（A-4 口径）
    let prices = if body.prices.is_empty() {
        cfg_now.round.to_unit_prices()
    } else {
        body.prices
    };
    let allocation = match body.allocation {
        Some(allocation) => allocation,
        None => match body.order_table {
            Some(table) => order_table_to_allocation(&table),
            None => current_allocation(state).await?,
        },
    };
    let request = PlanRequest {
        config,
        allocation,
        strategy,
        limits: body.limits.unwrap_or_default(),
        prices,
    };
    let result = planner::plan(&request);
    Ok(json!({
        "strategy": strategy_name,
        "best": result.best,
        "best_result": result.best_result,
        "best_score": result.best_score,
        "candidates": result.candidates,
        "stats": result.stats,
        "planner": "t5",
    }))
}

#[derive(Deserialize, Default)]
pub struct FromAllocationBody {
    #[serde(default)]
    pub prices: Vec<UnitPrice>,
}

pub async fn from_allocation(
    state: &ApiState,
    body: FromAllocationBody,
) -> Result<Value, ServiceError> {
    let (version, board) = state.pipeline.board().await;
    if board.is_null() {
        return Ok(json!({
            "order_table": OrderTable::default(),
            "version": version,
            "empty": true,
        }));
    }
    let snapshot: AllocationSnapshot =
        serde_json::from_value(board).map_err(|e| ServiceError::Internal(e.into()))?;
    let prices = if body.prices.is_empty() {
        state.cfg.get().await.round.to_unit_prices()
    } else {
        body.prices
    };
    let order_table = order_table_from_allocation(&snapshot, &prices);
    let empty = order_table.packages.is_empty();
    Ok(json!({
        "order_table": order_table,
        "version": version,
        "empty": empty,
    }))
}

pub fn parse_strategy(raw: &str) -> Result<planner::Strategy, String> {
    match raw {
        "gift_max" => Ok(planner::Strategy::GiftMax),
        "discount_max" => Ok(planner::Strategy::DiscountMax),
        _ => Err("strategy 必须是 gift_max 或 discount_max".to_string()),
    }
}

pub async fn current_allocation(state: &ApiState) -> Result<AllocationSnapshot, ServiceError> {
    let (_version, board) = state.pipeline.board().await;
    if board.is_null() {
        return Ok(empty_allocation());
    }
    serde_json::from_value(board).map_err(|e| ServiceError::Internal(e.into()))
}

pub fn empty_allocation() -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: RoundId("settlement-plan".to_string()),
        version: 0,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![],
        user_summaries: vec![],
        warnings: vec![],
    }
}

pub fn order_table_to_allocation(table: &OrderTable) -> AllocationSnapshot {
    let mut grouped: BTreeMap<(String, Option<String>, bool), ItemAllocation> = BTreeMap::new();
    for package in &table.packages {
        for line in &package.lines {
            if line.qty == 0 {
                continue;
            }
            let key = (line.item_id.clone(), line.variant_id.clone(), line.is_gift);
            let entry = grouped.entry(key).or_insert_with(|| ItemAllocation {
                item_id: ItemId(line.item_id.clone()),
                item_name: line.item_id.clone(),
                kind: if line.is_gift { "gift" } else { "split" }.to_string(),
                variant_id: line.variant_id.clone(),
                boxes: vec![],
                singles: vec![],
                waiting: vec![],
            });
            entry.singles.push(SingleAllocation {
                user_id: UserId(package.package_id.clone()),
                claim_id: ClaimId(package.package_id.clone()),
                item_id: ItemId(line.item_id.clone()),
                quantity: line.qty,
                unit_price: MoneyCents(line.unit_price_cents),
            });
        }
    }
    AllocationSnapshot {
        round_id: RoundId("settlement-plan".to_string()),
        version: 0,
        generated_at: chrono::Utc::now(),
        item_allocations: grouped.into_values().collect(),
        user_summaries: vec![],
        warnings: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settlement::{GiftTier, Line, Package};

    #[test]
    fn parse_strategy_accepts_known_values() {
        assert!(matches!(
            parse_strategy("gift_max"),
            Ok(planner::Strategy::GiftMax)
        ));
        assert!(matches!(
            parse_strategy("discount_max"),
            Ok(planner::Strategy::DiscountMax)
        ));
        assert!(parse_strategy("nope").is_err());
    }

    #[test]
    fn order_table_to_allocation_groups_lines_by_item() {
        let table = OrderTable::new(vec![
            Package::new("p1", vec![Line::new("a", 2, 100).variant("v1")]),
            Package::new("p2", vec![Line::new("a", 1, 100).variant("v1")]),
        ]);
        let allocation = order_table_to_allocation(&table);
        assert_eq!(allocation.item_allocations.len(), 1);
        let item = &allocation.item_allocations[0];
        assert_eq!(item.singles.len(), 2);
        let total: u32 = item.singles.iter().map(|s| s.quantity).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn synthesized_allocation_feeds_planner() {
        let mut config = SettlementConfig::default();
        config.gift_tiers.push(GiftTier {
            tier_id: "t100".to_string(),
            threshold: 100,
            gift_name: "占位特典".to_string(),
            unit_price: 10,
            claimed: 10,
        });
        let table = OrderTable::new(vec![
            Package::new("p1", vec![Line::new("a", 1, 50)]),
            Package::new("p2", vec![Line::new("a", 1, 50)]),
        ]);
        let request = PlanRequest {
            config,
            allocation: order_table_to_allocation(&table),
            strategy: planner::Strategy::GiftMax,
            limits: PlanLimits::default(),
            prices: vec![],
        };
        let result = planner::plan(&request);
        let hits: u32 = result
            .best_result
            .packages
            .iter()
            .map(|p| p.gift_count)
            .sum();
        assert!(hits >= 1);
    }

    fn round_with_adjust(adjust_cents: i64) -> RoundSettings {
        use crate::settings::{ItemConfig, RoundSettings, VariantConfig};
        RoundSettings {
            round_id: "r".to_string(),
            title: "t".to_string(),
            group_id: "g".to_string(),
            priority_users: vec![],
            priority_window: None,
            phases: vec![],
            items: vec![ItemConfig {
                item_id: "a".to_string(),
                name: "A".to_string(),
                kind: "group".to_string(),
                class: None,
                aliases: vec![],
                unit_price_cents: 5000,
                max_quantity: None,
                variants: vec![VariantConfig {
                    variant_id: "v1".to_string(),
                    name: "甲".to_string(),
                    unit_price_cents: 5000,
                    adjust_cents,
                    capacity: None,
                    aliases: vec![],
                }],
            }],
        }
    }

    #[test]
    fn merge_catalog_adjust_sets_adjusted_price_only() {
        let merged = merge_catalog_pricing(SettlementConfig::default(), &round_with_adjust(-500));
        assert_eq!(merged.pricing.len(), 1);
        assert!(matches!(merged.pricing[0].mode, PricingMode::AdjustBy));
        assert_eq!(merged.pricing[0].value, -500);

        let table = OrderTable::new(vec![Package::new(
            "p1",
            vec![Line::new("a", 1, 5000).variant("v1")],
        )]);
        let result = evaluate(&merged, &table);
        assert_eq!(result.lines[0].unit_price_cents, 5000, "标价 A 口径不变");
        assert_eq!(result.packages[0].gross_cents, 5000, "毛额以 A 计");
        assert_eq!(
            result.packages[0].adjusted_cents, 4500,
            "调后价 = C = A + B"
        );
    }

    #[test]
    fn explicit_pricing_wins_over_catalog_adjust() {
        let mut base = SettlementConfig::default();
        base.pricing.push(PricingEntry {
            item_id: "a".to_string(),
            variant_id: Some("v1".to_string()),
            mode: PricingMode::SetFinal,
            value: 6000,
        });
        let merged = merge_catalog_pricing(base, &round_with_adjust(-500));
        assert_eq!(merged.pricing.len(), 1, "已存在的显式定价保留、不追加");
        assert_eq!(merged.pricing[0].value, 6000);
    }
}
