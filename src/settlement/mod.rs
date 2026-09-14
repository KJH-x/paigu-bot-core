use serde::{Deserialize, Serialize};

#[allow(dead_code)]
mod engine;
#[allow(dead_code)]
mod model;

#[allow(unused_imports)]
pub use engine::evaluate;
#[allow(unused_imports)]
pub use model::{
    order_table_from_allocation, Line, OrderTable, Package, PackageGift, PackageSettlement,
    SettlementResult, UnitPrice,
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
    pub threshold: i64,
    pub gift_name: String,
    pub unit_price: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReduceAverageConfig {
    pub include_gift_price: bool,
}

#[cfg(test)]
mod tests;
