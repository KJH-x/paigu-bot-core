use serde::{Deserialize, Serialize};

use crate::domain::ids::{ItemId, RoundId, UserId};
use crate::domain::money::MoneyCents;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiftRule {
    pub round_id: RoundId,
    pub gift_item_id: ItemId,
    pub threshold: MoneyCents,
    pub quantity_per_threshold: u32,
    pub valuation: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiftAllocation {
    pub gift_item_id: ItemId,
    pub user_id: UserId,
    pub quantity: u32,
    pub valuation_subtotal: MoneyCents,
}

pub fn compute_gift_quantity(scoped_total: MoneyCents, threshold: MoneyCents) -> u32 {
    if threshold.as_cents() <= 0 {
        return 0;
    }
    (scoped_total.as_cents() / threshold.as_cents()) as u32
}
