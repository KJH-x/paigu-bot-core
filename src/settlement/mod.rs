use serde::{Deserialize, Serialize};

#[allow(dead_code)]
mod engine;
#[allow(dead_code)]
mod model;

#[allow(unused_imports)]
pub use engine::evaluate;
#[allow(unused_imports)]
pub use model::{
    order_table_from_allocation, Line, LineSettlement, OrderTable, Package, PackageGift,
    PackageSettlement, SettlementResult, UnitPrice,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettlementConfig {
    #[serde(default)]
    pub pricing: Vec<PricingEntry>,
    #[serde(default)]
    pub discounts: Vec<DiscountEntry>,
    #[serde(default)]
    pub scope_mode: ScopeMode,
    #[serde(default)]
    pub gift_tiers: Vec<GiftTier>,
    #[serde(default)]
    pub reduce_average: ReduceAverageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingEntry {
    pub item_id: String,
    pub variant_id: Option<String>,
    pub mode: PricingMode,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PricingMode {
    AdjustBy,
    SetFinal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountEntry {
    pub rule_id: String,
    pub kind: DiscountKind,
    pub amount: i64,
    pub threshold: Option<i64>,
    pub ratio_ppm: Option<i64>,
    pub shares: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiscountKind {
    Threshold,
    WholeOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScopeMode {
    IncludeGift,
    ExcludeGift,
}

impl Default for ScopeMode {
    fn default() -> Self {
        ScopeMode::ExcludeGift
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiftTier {
    pub tier_id: String,
    /// 展示用门槛（新口径下不参与判定，保留兼容）。
    #[serde(default)]
    pub threshold: i64,
    pub gift_name: String,
    /// 每份特典的指定价（用于折价 `G`）。
    pub unit_price: i64,
    /// 排谷阶段的认购数；实际授予 = `min(claimed, 下单包数 P)`。
    #[serde(default)]
    pub claimed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReduceAverageConfig {
    pub include_gift_price: bool,
}

#[cfg(test)]
mod tests;
