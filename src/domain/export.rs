use serde::{Deserialize, Serialize};

use crate::domain::money::MoneyCents;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemSummaryRow {
    pub item_id: String,
    pub name: String,
    pub kind: String,
    pub unit_price_yuan: String,
    pub total_quantity: u32,
    pub gross_total_yuan: String,
    pub completed_boxes: u32,
    pub incomplete_boxes: u32,
    pub gift_quantity: u32,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBillRow {
    pub user_id: String,
    pub display_name: String,
    pub item_details: String,
    pub gross_total_yuan: String,
    pub discount_share_yuan: String,
    pub gift_value_share_yuan: String,
    pub shipping_fee_yuan: String,
    pub final_total_yuan: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderHelperRow {
    pub item_name: String,
    pub quantity: u32,
    pub unit_price_yuan: String,
    pub total_yuan: String,
    pub discount_scope: String,
    pub notes: String,
}

pub fn format_money(m: MoneyCents) -> String {
    m.format_yuan()
}
