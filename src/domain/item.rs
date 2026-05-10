use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::domain::ids::{ItemId, RoundId, AliasId};
use crate::domain::money::MoneyCents;
use crate::domain::claim::ClaimType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemKind {
    Split,
    Single,
    Gift,
    Shipping,
    Adjustment,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Split => "split",
            ItemKind::Single => "single",
            ItemKind::Gift => "gift",
            ItemKind::Shipping => "shipping",
            ItemKind::Adjustment => "adjustment",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "split" => Some(ItemKind::Split),
            "single" => Some(ItemKind::Single),
            "gift" => Some(ItemKind::Gift),
            "shipping" => Some(ItemKind::Shipping),
            "adjustment" => Some(ItemKind::Adjustment),
            _ => None,
        }
    }

    pub fn compatible_with(&self, claim_type: &ClaimType) -> bool {
        match (self, claim_type) {
            (ItemKind::Split, ClaimType::Split) => true,
            (ItemKind::Single, ClaimType::Single) => true,
            (ItemKind::Gift, ClaimType::GiftClaim) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub item_id: ItemId,
    pub round_id: RoundId,
    pub name: String,
    pub kind: ItemKind,
    pub unit_price: MoneyCents,
    pub box_size: Option<u32>,
    pub max_quantity: Option<u32>,
    pub is_blind: bool,
    pub is_proxy_card: bool,
    pub aliases: Vec<String>,
    pub sort_order: i32,
    pub metadata: JsonValue,
}

impl Item {
    pub fn matches_name_or_alias(&self, name: &str) -> bool {
        if self.item_id.0 == name {
            return true;
        }
        if self.name.contains(name) {
            return true;
        }
        self.aliases.iter().any(|a| a.contains(name))
    }

    pub fn exact_matches_item_id(&self, id: &str) -> bool {
        self.item_id.0 == id
    }

    pub fn exact_matches_name(&self, name: &str) -> bool {
        self.name == name || self.aliases.iter().any(|a| a == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAlias {
    pub alias_id: AliasId,
    pub round_id: RoundId,
    pub item_id: ItemId,
    pub alias: String,
    pub weight: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundContext {
    pub round_id: RoundId,
    pub title: String,
    pub items: Vec<Item>,
}
