use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
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

fn draft_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn draft_dir() -> PathBuf {
    std::env::var("PAIGU_SETTLEMENT_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/settlement"))
}

fn draft_path(round_id: &str) -> PathBuf {
    draft_dir().join(format!("{round_id}.json"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementDraft {
    pub schema_version: u32,
    pub round_id: String,
    pub revision: u64,
    #[serde(default)]
    pub order_table: OrderTable,
    #[serde(default)]
    pub last_result: Option<SettlementResult>,
    #[serde(default)]
    pub selected_strategy: Option<String>,
    #[serde(default)]
    pub order_numbers: BTreeMap<String, String>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub locked_at_ms: Option<i64>,
    pub updated_at_ms: i64,
}

impl SettlementDraft {
    fn empty(round_id: &str) -> Self {
        Self {
            schema_version: 1,
            round_id: round_id.to_string(),
            revision: 0,
            order_table: OrderTable::default(),
            last_result: None,
            selected_strategy: None,
            order_numbers: BTreeMap::new(),
            locked: false,
            locked_at_ms: None,
            updated_at_ms: crate::services::messages::now_ms(),
        }
    }
}

fn read_draft(round_id: &str) -> Result<SettlementDraft, ServiceError> {
    let _guard = draft_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    read_draft_unlocked(round_id)
}

fn read_draft_unlocked(round_id: &str) -> Result<SettlementDraft, ServiceError> {
    let path = draft_path(round_id);
    if !path.exists() {
        return Ok(SettlementDraft::empty(round_id));
    }
    let raw =
        std::fs::read_to_string(path).map_err(|error| ServiceError::Internal(error.into()))?;
    serde_json::from_str(&raw).map_err(|error| ServiceError::Internal(error.into()))
}

pub fn round_locked(round_id: &str) -> Result<bool, ServiceError> {
    Ok(read_draft(round_id)?.locked)
}

pub async fn is_locked(state: &ApiState) -> Result<bool, ServiceError> {
    let round_id = state.cfg.get().await.round.round_id;
    Ok(round_locked(&round_id)? || state.pipeline.is_locked().await)
}

fn write_draft_unlocked(draft: &SettlementDraft) -> Result<(), ServiceError> {
    let path = draft_path(&draft.round_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ServiceError::Internal(error.into()))?;
    }
    let temp = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let raw =
        serde_json::to_vec_pretty(draft).map_err(|error| ServiceError::Internal(error.into()))?;
    std::fs::write(&temp, raw).map_err(|error| ServiceError::Internal(error.into()))?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| ServiceError::Internal(error.into()))?;
    }
    std::fs::rename(temp, path).map_err(|error| ServiceError::Internal(error.into()))?;
    Ok(())
}

fn write_draft_checked(draft: &mut SettlementDraft, expected: u64) -> Result<(), ServiceError> {
    let _guard = draft_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current = read_draft_unlocked(&draft.round_id)?;
    ensure_draft_revision(&current, expected)?;
    draft.revision = current.revision.saturating_add(1);
    write_draft_unlocked(draft)
}

pub async fn get_draft(state: &ApiState) -> Result<Value, ServiceError> {
    let round_id = state.cfg.get().await.round.round_id;
    let draft = read_draft(&round_id)?;
    Ok(json!({ "draft": draft }))
}

#[derive(Deserialize)]
pub struct DraftBody {
    pub revision: u64,
    pub order_table: OrderTable,
    #[serde(default)]
    pub selected_strategy: Option<String>,
    #[serde(default)]
    pub order_numbers: BTreeMap<String, String>,
}

pub async fn put_draft(state: &ApiState, body: DraftBody) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let mut draft = read_draft(&cfg.round.round_id)?;
    if draft.locked || state.pipeline.is_locked().await {
        return Err(ServiceError::Locked("结算草稿已锁定".to_string()));
    }
    ensure_draft_revision(&draft, body.revision)?;
    let result = evaluate(
        &merge_catalog_pricing(cfg.settlement, &cfg.round),
        &body.order_table,
    );
    draft.order_table = body.order_table;
    draft.last_result = Some(result);
    draft.selected_strategy = body.selected_strategy;
    draft.order_numbers = body.order_numbers;
    draft.updated_at_ms = crate::services::messages::now_ms();
    write_draft_checked(&mut draft, body.revision)?;
    Ok(json!({ "draft": draft }))
}

#[derive(Deserialize)]
pub struct DraftRevisionBody {
    pub revision: u64,
}

pub async fn lock_draft(state: &ApiState, body: DraftRevisionBody) -> Result<Value, ServiceError> {
    let cfg = state.cfg.get().await;
    let mut draft = read_draft(&cfg.round.round_id)?;
    ensure_draft_revision(&draft, body.revision)?;
    if draft.locked {
        return Ok(json!({ "draft": draft }));
    }
    let nonempty: Vec<_> = draft
        .order_table
        .packages
        .iter()
        .filter(|package| package.lines.iter().any(|line| line.qty > 0))
        .collect();
    if nonempty.is_empty() {
        return Err(ServiceError::Unprocessable(
            "没有可锁定的订单包".to_string(),
        ));
    }
    let mut unique = BTreeSet::new();
    let mut package_ids = BTreeSet::new();
    for package in &nonempty {
        if package.package_id.trim().is_empty() || !package_ids.insert(package.package_id.clone()) {
            return Err(ServiceError::Unprocessable(
                "订单包名称不能为空且必须唯一".to_string(),
            ));
        }
        let number = draft
            .order_numbers
            .get(&package.package_id)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ServiceError::Unprocessable(format!("订单包 {} 缺少订单号", package.package_id))
            })?;
        if !unique.insert(number.to_string()) {
            return Err(ServiceError::Unprocessable(format!("订单号重复：{number}")));
        }
    }
    let allocation = current_allocation(state).await?;
    let completeness = check_completeness(&draft.order_table, &allocation);
    if !completeness.complete {
        return Err(ServiceError::Unprocessable(format!(
            "排包未完成：{}",
            completeness.messages.join("；")
        )));
    }
    let prices = prices_from_order_table(&draft.order_table)?;
    validate_prices_for_allocation(&allocation, &prices)?;
    draft.last_result = Some(evaluate(
        &merge_catalog_pricing(cfg.settlement, &cfg.round),
        &draft.order_table,
    ));
    draft.locked = true;
    let now = crate::services::messages::now_ms();
    draft.locked_at_ms = Some(now);
    draft.updated_at_ms = now;
    write_draft_checked(&mut draft, body.revision)?;
    state.pipeline.set_locked(true).await;
    Ok(json!({ "draft": draft }))
}

pub async fn unlock_draft(
    state: &ApiState,
    body: DraftRevisionBody,
) -> Result<Value, ServiceError> {
    let round_id = state.cfg.get().await.round.round_id;
    let mut draft = read_draft(&round_id)?;
    ensure_draft_revision(&draft, body.revision)?;
    draft.locked = false;
    draft.locked_at_ms = None;
    draft.updated_at_ms = crate::services::messages::now_ms();
    write_draft_checked(&mut draft, body.revision)?;
    state.pipeline.set_locked(false).await;
    Ok(json!({ "draft": draft }))
}

fn ensure_draft_revision(draft: &SettlementDraft, expected: u64) -> Result<(), ServiceError> {
    if draft.revision != expected {
        return Err(ServiceError::Conflict(json!({
            "error": "stale_draft_revision",
            "revision": draft.revision,
            "draft": draft,
        })));
    }
    Ok(())
}

pub async fn restore_persistent_lock(state: &ApiState) -> Result<(), ServiceError> {
    let round_id = state.cfg.get().await.round.round_id;
    state
        .pipeline
        .set_locked(read_draft(&round_id)?.locked)
        .await;
    Ok(())
}

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
    if is_locked(state).await? {
        return Err(ServiceError::Locked(
            "轮次已锁定，不能修改结算规则".to_string(),
        ));
    }
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
    let source_table = body.order_table.clone();
    // 手工订单表中的显式价格是本次规划的权威价格；无订单表时才回退商品目录。
    let prices = if let Some(table) = &source_table {
        prices_from_order_table(table)?
    } else if body.prices.is_empty() {
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
    validate_prices_for_allocation(&allocation, &prices)?;
    let request = PlanRequest {
        config,
        allocation,
        strategy,
        limits: body.limits.unwrap_or_default(),
        prices,
    };
    let result = planner::plan(&request);
    if let Some(source) = &source_table {
        ensure_plan_invariants(source, &result.best)?;
    }
    Ok(json!({
        "strategy": strategy_name,
        "best": result.best,
        "best_result": result.best_result,
        "best_score": result.best_score,
        "candidates": result.candidates,
        "stats": result.stats,
        "planner": "t5",
        "invariants_ok": true,
    }))
}

fn prices_from_order_table(table: &OrderTable) -> Result<Vec<UnitPrice>, ServiceError> {
    let mut prices: BTreeMap<(String, Option<String>), i64> = BTreeMap::new();
    for package in &table.packages {
        for line in &package.lines {
            if line.qty == 0 || line.is_gift {
                continue;
            }
            if line.unit_price_cents <= 0 {
                return Err(ServiceError::Unprocessable(format!(
                    "非赠品 {}{} 缺少有效标价",
                    line.item_id,
                    line.variant_id
                        .as_deref()
                        .map(|v| format!("/{v}"))
                        .unwrap_or_default()
                )));
            }
            let key = (line.item_id.clone(), line.variant_id.clone());
            if let Some(existing) = prices.insert(key.clone(), line.unit_price_cents) {
                if existing != line.unit_price_cents {
                    return Err(ServiceError::Unprocessable(format!(
                        "同一商品/变体价格冲突：{}",
                        key.0
                    )));
                }
            }
        }
    }
    Ok(prices
        .into_iter()
        .map(|((item_id, variant_id), unit_price_cents)| UnitPrice {
            item_id,
            variant_id,
            unit_price_cents,
        })
        .collect())
}

fn validate_prices_for_allocation(
    allocation: &AllocationSnapshot,
    prices: &[UnitPrice],
) -> Result<(), ServiceError> {
    for item in &allocation.item_allocations {
        if item.kind == "gift" {
            continue;
        }
        let price = item
            .variant_id
            .as_ref()
            .and_then(|variant| {
                prices.iter().find(|price| {
                    price.item_id == item.item_id.0
                        && price.variant_id.as_deref() == Some(variant.as_str())
                })
            })
            .or_else(|| {
                prices
                    .iter()
                    .find(|price| price.item_id == item.item_id.0 && price.variant_id.is_none())
            });
        if price.map(|price| price.unit_price_cents > 0) != Some(true) {
            return Err(ServiceError::Unprocessable(format!(
                "商品 {} 缺少非零标价，已停止规划",
                item.item_id.0
            )));
        }
    }
    Ok(())
}

fn ensure_plan_invariants(source: &OrderTable, planned: &OrderTable) -> Result<(), ServiceError> {
    fn signature(table: &OrderTable) -> BTreeMap<(String, Option<String>, bool), (u32, i64)> {
        let mut out = BTreeMap::new();
        for package in &table.packages {
            for line in &package.lines {
                let entry = out
                    .entry((line.item_id.clone(), line.variant_id.clone(), line.is_gift))
                    .or_insert((0u32, 0i64));
                entry.0 = entry.0.saturating_add(line.qty);
                if !line.is_gift {
                    entry.1 = entry.1.saturating_add(line.total_cents());
                }
            }
        }
        out
    }
    if signature(source) != signature(planned) {
        return Err(ServiceError::Unprocessable(
            "规划前后商品数量或折前总价不守恒，已禁止应用".to_string(),
        ));
    }
    if planned
        .packages
        .iter()
        .flat_map(|package| &package.lines)
        .any(|line| !line.is_gift && line.qty > 0 && line.unit_price_cents <= 0)
    {
        return Err(ServiceError::Unprocessable(
            "规划结果包含零价非赠品，已禁止应用".to_string(),
        ));
    }
    Ok(())
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
            category: crate::round::RoundCategory::Normal,
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

    #[test]
    fn order_table_prices_reject_zero_and_conflicts() {
        let zero = OrderTable::new(vec![Package::new("p1", vec![Line::new("a", 1, 0)])]);
        assert!(matches!(
            prices_from_order_table(&zero),
            Err(ServiceError::Unprocessable(_))
        ));

        let conflict = OrderTable::new(vec![
            Package::new("p1", vec![Line::new("a", 1, 100)]),
            Package::new("p2", vec![Line::new("a", 1, 200)]),
        ]);
        assert!(matches!(
            prices_from_order_table(&conflict),
            Err(ServiceError::Unprocessable(_))
        ));
    }

    #[test]
    fn plan_invariant_accepts_repack_and_rejects_price_loss() {
        let source = OrderTable::new(vec![
            Package::new("p1", vec![Line::new("a", 1, 100)]),
            Package::new("p2", vec![Line::new("a", 2, 100)]),
        ]);
        let repacked = OrderTable::new(vec![Package::new("p3", vec![Line::new("a", 3, 100)])]);
        assert!(ensure_plan_invariants(&source, &repacked).is_ok());

        let zeroed = OrderTable::new(vec![Package::new("p3", vec![Line::new("a", 3, 0)])]);
        assert!(matches!(
            ensure_plan_invariants(&source, &zeroed),
            Err(ServiceError::Unprocessable(_))
        ));
    }
}
