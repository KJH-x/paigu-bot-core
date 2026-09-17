use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::allocation::{ItemAllocation, SingleAllocation};
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;
use crate::planner::{self, PlanLimits, PlanRequest};
use crate::settlement::{
    evaluate, order_table_from_allocation, OrderTable, SettlementConfig, SettlementResult, UnitPrice,
};

use super::{api_bad_request, api_internal, api_stale_revision, ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route(
            "/api/settlement/config",
            get(get_settlement_config).put(put_settlement_config),
        )
        .route("/api/settlement/evaluate", post(evaluate_order))
        .route("/api/settlement/plan", post(plan_order))
        .route("/api/settlement/from-allocation", post(from_allocation))
}

async fn get_settlement_config(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let cfg = state.cfg.get().await;
    Json(json!({ "config": cfg.settlement, "revision": cfg.revision }))
}

#[derive(Deserialize)]
struct ConfigBody {
    config: SettlementConfig,
    revision: u64,
}

async fn put_settlement_config(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<ConfigBody>,
) -> Result<Json<Value>, ApiError> {
    let mut cfg = state.cfg.get().await;
    cfg.settlement = body.config;
    match state.cfg.put(cfg, body.revision).await {
        Ok(revision) => {
            let cfg = state.cfg.get().await;
            Ok(Json(json!({ "config": cfg.settlement, "revision": revision })))
        }
        Err(crate::settings::ConfigError::StaleRevision { actual, .. }) => {
            Err(api_stale_revision(actual))
        }
        Err(err) => Err(api_internal(err)),
    }
}

#[derive(Deserialize)]
struct EvaluateBody {
    order_table: OrderTable,
    #[serde(default)]
    config: Option<SettlementConfig>,
}

async fn evaluate_order(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<EvaluateBody>,
) -> Result<Json<SettlementResult>, ApiError> {
    let config = match body.config {
        Some(config) => config,
        None => state.cfg.get().await.settlement,
    };
    Ok(Json(evaluate(&config, &body.order_table)))
}

#[derive(Deserialize)]
struct PlanBody {
    strategy: String,
    #[serde(default)]
    order_table: Option<OrderTable>,
    #[serde(default)]
    allocation: Option<AllocationSnapshot>,
    #[serde(default)]
    config: Option<SettlementConfig>,
    #[serde(default)]
    prices: Vec<UnitPrice>,
    #[serde(default)]
    limits: Option<PlanLimits>,
}

async fn plan_order(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<PlanBody>,
) -> Result<Json<Value>, ApiError> {
    let strategy_name = body.strategy.clone();
    let strategy = parse_strategy(&strategy_name)?;
    let cfg_now = state.cfg.get().await;
    let config = match body.config {
        Some(config) => config,
        None => cfg_now.settlement.clone(),
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
            None => current_allocation(&state).await?,
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
    Ok(Json(json!({
        "strategy": strategy_name,
        "best": result.best,
        "best_result": result.best_result,
        "best_score": result.best_score,
        "candidates": result.candidates,
        "stats": result.stats,
        "planner": "t5",
    })))
}

#[derive(Deserialize, Default)]
struct FromAllocationBody {
    #[serde(default)]
    prices: Vec<UnitPrice>,
}

async fn from_allocation(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<FromAllocationBody>,
) -> Result<Json<Value>, ApiError> {
    let (version, board) = state.pipeline.board().await;
    if board.is_null() {
        return Ok(Json(json!({
            "order_table": OrderTable::default(),
            "version": version,
            "empty": true,
        })));
    }
    let snapshot: AllocationSnapshot = serde_json::from_value(board).map_err(api_internal)?;
    let prices = if body.prices.is_empty() {
        state.cfg.get().await.round.to_unit_prices()
    } else {
        body.prices
    };
    let order_table = order_table_from_allocation(&snapshot, &prices);
    let empty = order_table.packages.is_empty();
    Ok(Json(json!({
        "order_table": order_table,
        "version": version,
        "empty": empty,
    })))
}

fn parse_strategy(raw: &str) -> Result<planner::Strategy, ApiError> {
    match raw {
        "gift_max" => Ok(planner::Strategy::GiftMax),
        "discount_max" => Ok(planner::Strategy::DiscountMax),
        _ => Err(api_bad_request("strategy 必须是 gift_max 或 discount_max")),
    }
}

async fn current_allocation(state: &ApiState) -> Result<AllocationSnapshot, ApiError> {
    let (_version, board) = state.pipeline.board().await;
    if board.is_null() {
        return Ok(empty_allocation());
    }
    serde_json::from_value(board).map_err(api_internal)
}

fn empty_allocation() -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: RoundId("settlement-plan".to_string()),
        version: 0,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![],
        user_summaries: vec![],
        warnings: vec![],
    }
}

fn order_table_to_allocation(table: &OrderTable) -> AllocationSnapshot {
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
        assert!(matches!(parse_strategy("gift_max"), Ok(planner::Strategy::GiftMax)));
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
}
