use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::domain::claim::ClaimType;
use crate::domain::ids::{AliasId, ItemId, RoundId};
use crate::domain::money::MoneyCents;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemVariant {
    pub variant_id: String,
    pub name: String,
    pub unit_price: MoneyCents,
    pub capacity: Option<u32>,
    #[serde(default)]
    pub aliases: Vec<String>,
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
    #[serde(default)]
    pub variants: Vec<ItemVariant>,
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

    pub fn find_variant_by_name(&self, name: &str) -> Option<&ItemVariant> {
        self.variants
            .iter()
            .find(|v| v.name == name || v.aliases.iter().any(|a| a == name))
    }

    pub fn find_variant_by_id(&self, variant_id: &str) -> Option<&ItemVariant> {
        self.variants.iter().find(|v| v.variant_id == variant_id)
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
