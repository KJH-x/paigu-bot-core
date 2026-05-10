use serde::{Deserialize, Serialize};

use crate::domain::ids::{ItemId, UserId, RoundId};
use crate::domain::money::MoneyCents;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountRulesSet {
    pub round_id: RoundId,
    pub rules: Vec<DiscountRule>,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DiscountRule {
    ThresholdDiscount {
        rule_id: String,
        scope: DiscountScope,
        threshold: MoneyCents,
        discount: MoneyCents,
        repeatable: bool,
        stackable: bool,
    },
    FixedActualDiscount {
        rule_id: String,
        scope: DiscountScope,
        amount: MoneyCents,
        allocation_policy: DiscountAllocationPolicy,
    },
    ShoppingFund {
        rule_id: String,
        amount: MoneyCents,
        allocation_policy: DiscountAllocationPolicy,
    },
    GiftByThreshold {
        rule_id: String,
        threshold: MoneyCents,
        gift_item_id: ItemId,
        gift_quantity_per_threshold: u32,
        gift_valuation: MoneyCents,
        allocation_policy: GiftAllocationPolicy,
        value_offset_policy: DiscountAllocationPolicy,
    },
}

impl DiscountRule {
    pub fn rule_id(&self) -> &str {
        match self {
            DiscountRule::ThresholdDiscount { rule_id, .. } => rule_id,
            DiscountRule::FixedActualDiscount { rule_id, .. } => rule_id,
            DiscountRule::ShoppingFund { rule_id, .. } => rule_id,
            DiscountRule::GiftByThreshold { rule_id, .. } => rule_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscountScope {
    AllPaidItems,
    ItemIds(Vec<ItemId>),
    ItemKinds(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscountAllocationPolicy {
    ByGrossAmountRatio,
    ByQuantityRatio,
    EqualByUser,
    Manual(Vec<ManualDiscountShare>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualDiscountShare {
    pub user_id: UserId,
    pub amount: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GiftAllocationPolicy {
    TreatAsSplitItem,
    GiveToPriorityUsers,
    Manual,
}
