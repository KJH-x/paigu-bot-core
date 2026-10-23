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

fn draft_path(round_id: &str) -> Result<PathBuf, ServiceError> {
    crate::settings::rounds::validate_round_id(round_id)
        .map_err(|_| ServiceError::BadRequest("轮次标识无效，未访问结算文件".into()))?;
    Ok(draft_dir().join(format!("{round_id}.json")))
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
    pub calculated_source: Option<String>,
    #[serde(default)]
    pub reviewed_source: Option<String>,
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
            calculated_source: None,
            reviewed_source: None,
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
    let path = draft_path(round_id)?;
    if !path.exists() {
        return Ok(SettlementDraft::empty(round_id));
    }
    let raw =
        std::fs::read_to_string(path).map_err(|error| ServiceError::Internal(error.into()))?;
    decode_draft(&raw, round_id)
}

fn decode_draft(raw: &str, round_id: &str) -> Result<SettlementDraft, ServiceError> {
    let draft: SettlementDraft =
        serde_json::from_str(raw).map_err(|error| ServiceError::Internal(error.into()))?;
    if draft.schema_version != 1 || draft.round_id != round_id {
        return Err(ServiceError::Unprocessable(
            "结算草稿版本或轮次不匹配，未读取为有效草稿".into(),
        ));
    }
    Ok(draft)
}

pub fn round_locked(round_id: &str) -> Result<bool, ServiceError> {
    Ok(read_draft(round_id)?.locked)
}

pub(crate) fn checkpoint(round_id: &str) -> Result<Value, ServiceError> {
    serde_json::to_value(read_draft(round_id)?).map_err(|e| ServiceError::Internal(e.into()))
}

pub async fn is_locked(state: &ApiState) -> Result<bool, ServiceError> {
    let round_id = state.cfg.get().await.round.round_id;
    Ok(round_locked(&round_id)? || state.pipeline.is_locked().await)
}

fn write_draft_unlocked(draft: &SettlementDraft) -> Result<(), ServiceError> {
    let path = draft_path(&draft.round_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ServiceError::Internal(error.into()))?;
    }
    let raw =
        serde_json::to_vec_pretty(draft).map_err(|error| ServiceError::Internal(error.into()))?;
    crate::persistence::atomic_write(&path, &raw)
        .map_err(|error| ServiceError::Internal(error.into()))?;
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
    draft_view(state, true).await
}

pub async fn get_status(state: &ApiState) -> Result<Value, ServiceError> {
    draft_view(state, false).await
}

async fn draft_view(state: &ApiState, include_draft: bool) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    let draft = read_draft(&cfg.round.round_id)?;
    review::response(
        &draft,
        &cfg,
        &current_allocation(state).await?,
        include_draft,
    )
}

#[derive(Deserialize)]
pub struct DraftBody {
    pub round_id: String,
    pub revision: u64,
    pub source_token: String,
    #[serde(default)]
    pub ack_review: bool,
    pub order_table: OrderTable,
    #[serde(default)]
    pub selected_strategy: Option<String>,
    #[serde(default)]
    pub order_numbers: BTreeMap<String, String>,
}

pub async fn put_draft(state: &ApiState, body: DraftBody) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(
            json!({"error":"round_changed","message":"轮次已切换，未保存到另一轮"}),
        ));
    }
    let mut draft = read_draft(&cfg.round.round_id)?;
    if draft.locked || state.pipeline.is_locked().await {
        return Err(ServiceError::Locked("结算草稿已锁定".to_string()));
    }
    ensure_draft_revision(&draft, body.revision)?;
    let allocation = current_allocation(state).await?;
    let token = review::source_token(&cfg, &allocation)?;
    review::ensure_source(&body.source_token, &token)?;
    if body.ack_review
        || (draft.revision == 0
            && draft.order_table.packages.is_empty()
            && draft.last_result.is_none())
    {
        draft.reviewed_source = Some(token.clone());
    }
    let result = evaluate(
        &merge_catalog_pricing(cfg.settlement.clone(), &cfg.round),
        &body.order_table,
    );
    draft.order_table = body.order_table;
    draft.last_result = Some(result);
    draft.calculated_source = Some(token);
    draft.selected_strategy = body.selected_strategy;
    draft.order_numbers = body.order_numbers;
    draft.updated_at_ms = crate::services::messages::now_ms();
    write_draft_checked(&mut draft, body.revision)?;
    review::response(&draft, &cfg, &allocation, true)
}

#[derive(Deserialize)]
pub struct DraftRevisionBody {
    pub round_id: String,
    pub revision: u64,
    #[serde(default)]
    pub source_token: Option<String>,
}

pub async fn lock_draft(state: &ApiState, body: DraftRevisionBody) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    if cfg.round.round_id != body.round_id {
        return Err(ServiceError::Conflict(json!({"error":"round_changed"})));
    }
    let mut draft = read_draft(&cfg.round.round_id)?;
    ensure_draft_revision(&draft, body.revision)?;
    let allocation = current_allocation(state).await?;
    let token = review::source_token(&cfg, &allocation)?;
    let expected = body
        .source_token
        .as_deref()
        .ok_or_else(|| ServiceError::BadRequest("锁定需要当前来源 source_token".into()))?;
    review::ensure_source(expected, &token)?;
    if draft.locked {
        return review::response(&draft, &cfg, &allocation, true);
    }
    if draft.reviewed_source.as_deref() != Some(&token) {
        return Err(ServiceError::Conflict(
            json!({"error":"settlement_review_required","message":"上游数据已变化，请保留当前订单并重新核对后锁定","needs_review":true,"source_token":token}),
        ));
    }
    let issues = review::issues(
        &draft.order_table,
        &draft.order_numbers,
        &allocation,
        &cfg.round,
    );
    if !issues.is_empty() {
        return Err(ServiceError::Validation(
            json!({"error":"settlement_invalid","message":"订单尚有待处理问题，请按清单定位修正","issues":issues}),
        ));
    }
    let prices = prices_from_order_table(&draft.order_table)?;
    validate_prices_for_allocation(&allocation, &prices)?;
    draft.last_result = Some(evaluate(
        &merge_catalog_pricing(cfg.settlement.clone(), &cfg.round),
        &draft.order_table,
    ));
    draft.calculated_source = Some(token);
    draft.locked = true;
    let now = crate::services::messages::now_ms();
    draft.locked_at_ms = Some(now);
    draft.updated_at_ms = now;
    write_draft_checked(&mut draft, body.revision)?;
    state.pipeline.restore_settlement_lock(true).await;
    state.pipeline.set_locked_locked(&cfg, true).await?;
    review::response(&draft, &cfg, &allocation, true)
}

pub async fn unlock_draft(
    state: &ApiState,
    body: DraftRevisionBody,
) -> Result<Value, ServiceError> {
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    let round_id = cfg.round.round_id.clone();
    if round_id != body.round_id {
        return Err(ServiceError::Conflict(json!({"error":"round_changed"})));
    }
    let mut draft = read_draft(&round_id)?;
    ensure_draft_revision(&draft, body.revision)?;
    draft.locked = false;
    draft.locked_at_ms = None;
    draft.updated_at_ms = crate::services::messages::now_ms();
    write_draft_checked(&mut draft, body.revision)?;
    state.pipeline.set_locked_locked(&cfg, false).await?;
    review::response(&draft, &cfg, &current_allocation(state).await?, true)
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
        .restore_settlement_lock(read_draft(&round_id)?.locked)
        .await;
    Ok(())
}

/// §U6：把目录子项的 `adjust_cents`（B）合成为结算定价（`mode=AdjustBy`，精确匹配
/// `item_id`/`subitem_id`），使 `evaluate` 的「调后价」列反映 C = A + B。
///
/// **仅内存合并，不持久化**。已有同 `(item_id, subitem_id)` 的显式定价优先保留，
/// 以便配置/覆盖中的显式定价仍以显式为准；请求自带 `pricing` 时整体以请求为准（不调用本函数）。
pub fn merge_catalog_pricing(
    mut config: SettlementConfig,
    round: &RoundSettings,
) -> SettlementConfig {
    for item in &round.items {
        for subitem in &item.subitems {
            if subitem.adjust_cents == 0 {
                continue;
            }
            let exists = config.pricing.iter().any(|entry| {
                entry.item_id == item.item_id
                    && entry.subitem_id.as_deref() == Some(subitem.subitem_id.as_str())
            });
            if exists {
                continue;
            }
            config.pricing.push(PricingEntry {
                item_id: item.item_id.clone(),
                subitem_id: Some(subitem.subitem_id.clone()),
                mode: PricingMode::AdjustBy,
                value: subitem.adjust_cents,
            });
        }
    }
    config
}

pub async fn get_settlement_config(state: &ApiState) -> Value {
    let cfg = state.cfg.get().await;
    json!({ "config": cfg.settlement, "revision": cfg.revision,"round_id":cfg.round.round_id })
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
    let _serial = state.pipeline.message_write_guard().await;
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
                    line.subitem_id
                        .as_deref()
                        .map(|v| format!("/{v}"))
                        .unwrap_or_default()
                )));
            }
            let key = (line.item_id.clone(), line.subitem_id.clone());
            if let Some(existing) = prices.insert(key.clone(), line.unit_price_cents) {
                if existing != line.unit_price_cents {
                    return Err(ServiceError::Unprocessable(format!(
                        "同一商品/子项价格冲突：{}",
                        key.0
                    )));
                }
            }
        }
    }
    Ok(prices
        .into_iter()
        .map(|((item_id, subitem_id), unit_price_cents)| UnitPrice {
            item_id,
            subitem_id,
            unit_price_cents,
        })
        .collect())
}

fn validate_prices_for_allocation(
    allocation: &AllocationSnapshot,
    prices: &[UnitPrice],
) -> Result<(), ServiceError> {
    for item in &allocation.item_allocations {
        if item.kind == "gift"
            || (!item
                .boxes
                .iter()
                .any(|b| b.slots.iter().any(|s| s.user_id.is_some()))
                && !item.singles.iter().any(|s| s.quantity > 0))
        {
            continue;
        }
        let price = item
            .subitem_id
            .as_ref()
            .and_then(|subitem| {
                prices.iter().find(|price| {
                    price.item_id == item.item_id.0
                        && price.subitem_id.as_deref() == Some(subitem.as_str())
                })
            })
            .or_else(|| {
                prices
                    .iter()
                    .find(|price| price.item_id == item.item_id.0 && price.subitem_id.is_none())
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
                    .entry((line.item_id.clone(), line.subitem_id.clone(), line.is_gift))
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
    let _serial = state.pipeline.message_write_guard().await;
    let cfg = state.cfg.get().await;
    let allocation = current_allocation(state).await?;
    let token = review::source_token(&cfg, &allocation)?;
    let (version, board) = state.pipeline.board().await;
    if board.is_null() {
        return Ok(json!({
            "order_table": OrderTable::default(),
            "version": version,
            "empty": true,
            "round_id": cfg.round.round_id,
            "source_token": token,
        }));
    }
    let snapshot: AllocationSnapshot =
        serde_json::from_value(board).map_err(|e| ServiceError::Internal(e.into()))?;
    let prices = if body.prices.is_empty() {
        cfg.round.to_unit_prices()
    } else {
        body.prices
    };
    let order_table = order_table_from_allocation(&snapshot, &prices);
    let empty = order_table.packages.is_empty();
    Ok(json!({
        "order_table": order_table,
        "version": version,
        "empty": empty,
        "round_id": cfg.round.round_id,
        "source_token": token,
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
            let key = (line.item_id.clone(), line.subitem_id.clone(), line.is_gift);
            let entry = grouped.entry(key).or_insert_with(|| ItemAllocation {
                item_id: ItemId(line.item_id.clone()),
                item_name: line.item_id.clone(),
                kind: if line.is_gift { "gift" } else { "split" }.to_string(),
                subitem_id: line.subitem_id.clone(),
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

mod review;

#[cfg(test)]
mod tests;
