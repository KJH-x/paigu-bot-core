use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ids::{RoundId, ItemId, UserId};
use crate::domain::money::MoneyCents;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementSnapshot {
    pub round_id: RoundId,
    pub version: i64,
    pub generated_at: DateTime<Utc>,
    pub user_bills: Vec<UserBill>,
    pub item_totals: Vec<ItemTotal>,
    pub discount_applications: Vec<DiscountApplication>,
    pub gross_total: MoneyCents,
    pub discount_total: MoneyCents,
    pub final_total: MoneyCents,
    pub warnings: Vec<SettlementWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBill {
    pub user_id: UserId,
    pub display_name: String,
    pub lines: Vec<UserBillLine>,
    pub gross_total: MoneyCents,
    pub discount_share: MoneyCents,
    pub gift_value_share: MoneyCents,
    pub shipping_fee: MoneyCents,
    pub final_total: MoneyCents,
    pub payment_status: PaymentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentStatus {
    Unpaid,
    Paid,
    Partial,
    Refunded,
}

impl Default for PaymentStatus {
    fn default() -> Self {
        PaymentStatus::Unpaid
    }
}

impl UserBill {
    pub fn negative_discount_total(&self) -> MoneyCents {
        MoneyCents(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBillLine {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: String,
    pub quantity: u32,
    pub unit_price: MoneyCents,
    pub gross: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemTotal {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: String,
    pub total_quantity: u32,
    pub unit_price: MoneyCents,
    pub gross_total: MoneyCents,
    pub box_count: u32,
    pub incomplete_box_count: u32,
    pub gift_quantity: u32,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountApplication {
    pub rule_id: String,
    pub rule_type: String,
    pub amount: MoneyCents,
    pub shares: Vec<DiscountShare>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountShare {
    pub user_id: UserId,
    pub amount: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementWarning {
    pub user_id: Option<UserId>,
    pub message: String,
    pub severity: String,
}
