use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::snapshot::AllocationSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OrderTable {
    #[serde(default)]
    pub packages: Vec<Package>,
}

impl OrderTable {
    pub fn new(packages: Vec<Package>) -> Self {
        Self { packages }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub package_id: String,
    #[serde(default)]
    pub lines: Vec<Line>,
}

impl Package {
    pub fn new(package_id: impl Into<String>, lines: Vec<Line>) -> Self {
        Self {
            package_id: package_id.into(),
            lines,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub item_id: String,
    #[serde(default)]
    pub variant_id: Option<String>,
    pub qty: u32,
    pub unit_price_cents: i64,
    #[serde(default)]
    pub is_gift: bool,
}

impl Line {
    pub fn new(item_id: impl Into<String>, qty: u32, unit_price_cents: i64) -> Self {
        Self {
            item_id: item_id.into(),
            variant_id: None,
            qty,
            unit_price_cents,
            is_gift: false,
        }
    }

    pub fn variant(mut self, variant_id: impl Into<String>) -> Self {
        self.variant_id = Some(variant_id.into());
        self
    }

    pub fn gift(mut self) -> Self {
        self.is_gift = true;
        self
    }

    pub fn total_cents(&self) -> i64 {
        self.unit_price_cents.saturating_mul(self.qty as i64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitPrice {
    pub item_id: String,
    #[serde(default)]
    pub variant_id: Option<String>,
    pub unit_price_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SettlementResult {
    pub packages: Vec<PackageSettlement>,
    pub gift_list: Vec<PackageGift>,
    pub gift_valuation_total: i64,
    pub reduce_average_total: i64,
    pub discount_total: i64,
    pub grand_total: i64,
    pub warnings: Vec<String>,
    /// `C` = 无折扣商品总价（各实购商品标价合计，不含特典）。
    #[serde(default)]
    pub list_total_cents: i64,
    /// `B` = Σ 各下单包实付价（折后，不含特典）。
    #[serde(default)]
    pub paid_total_cents: i64,
    /// 减均后逐行明细（含 `final_*`）。
    #[serde(default)]
    pub lines: Vec<LineSettlement>,
}

/// 单行（商品/变体）的减均明细。金额均为「分」，即 2 位小数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineSettlement {
    pub package_id: String,
    pub item_id: String,
    #[serde(default)]
    pub variant_id: Option<String>,
    pub qty: u32,
    /// 标价单价（原价）。
    pub unit_price_cents: i64,
    /// 标价合计 = `unit_price_cents × qty`。
    pub total_cents: i64,
    /// 减均分摊额（该行合计）。
    pub reduce_cents: i64,
    /// 减均后合计 = `total_cents − reduce_cents`。
    pub final_total_cents: i64,
    /// 减均后单价（按行合计均分，四舍五入；逐件精确值以 `final_total_cents` 为准）。
    pub final_unit_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSettlement {
    pub package_id: String,
    pub gross_cents: i64,
    pub adjusted_cents: i64,
    pub discount_cents: i64,
    pub reduce_average_cents: i64,
    pub payable_cents: i64,
    pub gift_count: u32,
    pub gift_value_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageGift {
    pub package_id: String,
    pub tier_id: String,
    pub gift_name: String,
    pub quantity: u32,
    pub unit_price_cents: i64,
}

pub fn order_table_from_allocation(snapshot: &AllocationSnapshot, prices: &[UnitPrice]) -> OrderTable {
    type LineKey = (String, Option<String>, bool);
    let mut by_package: BTreeMap<String, BTreeMap<LineKey, (u32, i64)>> = BTreeMap::new();

    for alloc in &snapshot.item_allocations {
        let is_gift = alloc.kind == "gift";
        let variant = alloc.variant_id.clone();
        let base_price = lookup_unit_price(prices, &alloc.item_id.0, variant.as_deref());

        for mbox in &alloc.boxes {
            for slot in &mbox.slots {
                let user_id = match &slot.user_id {
                    Some(u) => u,
                    None => continue,
                };
                let claim = slot
                    .claim_id
                    .as_ref()
                    .map(|c| c.0.clone())
                    .unwrap_or_default();
                let entry = by_package
                    .entry(package_key(&user_id.0, &claim))
                    .or_default();
                let line = entry
                    .entry((alloc.item_id.0.clone(), variant.clone(), is_gift))
                    .or_insert((0, base_price));
                line.0 += 1;
            }
        }

        for single in &alloc.singles {
            let entry = by_package
                .entry(package_key(&single.user_id.0, &single.claim_id.0))
                .or_default();
            let price = if single.unit_price.0 != 0 {
                single.unit_price.0
            } else {
                base_price
            };
            let line = entry
                .entry((alloc.item_id.0.clone(), variant.clone(), is_gift))
                .or_insert((0, price));
            line.0 += single.quantity;
        }
    }

    let packages = by_package
        .into_iter()
        .map(|(package_id, lines)| Package {
            package_id,
            lines: lines
                .into_iter()
                .map(
                    |((item_id, variant_id, is_gift), (qty, unit_price_cents))| Line {
                        item_id,
                        variant_id,
                        qty,
                        unit_price_cents,
                        is_gift,
                    },
                )
                .collect(),
        })
        .collect();

    OrderTable { packages }
}

fn lookup_unit_price(prices: &[UnitPrice], item_id: &str, variant_id: Option<&str>) -> i64 {
    if let Some(variant) = variant_id {
        if let Some(found) = prices
            .iter()
            .find(|p| p.item_id == item_id && p.variant_id.as_deref() == Some(variant))
        {
            return found.unit_price_cents;
        }
    }
    prices
        .iter()
        .find(|p| p.item_id == item_id && p.variant_id.is_none())
        .map(|p| p.unit_price_cents)
        .unwrap_or(0)
}

fn package_key(user_id: &str, claim_id: &str) -> String {
    if claim_id.is_empty() {
        user_id.to_string()
    } else {
        format!("{user_id}#{claim_id}")
    }
}

/// 商品数量键：`(item_id, variant_id, is_gift)`。
pub type QuantityKey = (String, Option<String>, bool);

/// 某类商品的「数量对不上」明细（排包未完成 / 数量超出）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuantityGap {
    pub item_id: String,
    #[serde(default)]
    pub variant_id: Option<String>,
    pub is_gift: bool,
    pub expected: u32,
    pub actual: u32,
}

/// 排包完成度报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompletenessReport {
    pub complete: bool,
    pub missing: Vec<QuantityGap>,
    pub extra: Vec<QuantityGap>,
    pub messages: Vec<String>,
}

/// 排谷结果（分配快照）中各商品应交数量。
pub fn expected_quantities(snapshot: &AllocationSnapshot) -> BTreeMap<QuantityKey, u32> {
    let mut out: BTreeMap<QuantityKey, u32> = BTreeMap::new();
    for alloc in &snapshot.item_allocations {
        let key = (
            alloc.item_id.0.clone(),
            alloc.variant_id.clone(),
            alloc.kind == "gift",
        );
        let mut qty = 0u32;
        for b in &alloc.boxes {
            for s in &b.slots {
                if s.user_id.is_some() {
                    qty = qty.saturating_add(1);
                }
            }
        }
        for s in &alloc.singles {
            qty = qty.saturating_add(s.quantity);
        }
        *out.entry(key).or_insert(0) += qty;
    }
    out
}

/// 下单表中各商品数量。
pub fn table_quantities(table: &OrderTable) -> BTreeMap<QuantityKey, u32> {
    let mut out: BTreeMap<QuantityKey, u32> = BTreeMap::new();
    for p in &table.packages {
        for l in &p.lines {
            let key = (l.item_id.clone(), l.variant_id.clone(), l.is_gift);
            *out.entry(key).or_insert(0) += l.qty;
        }
    }
    out
}

/// 校验下单表是否已「排包完成」：每类商品数量须与排谷结果一致。
pub fn check_completeness(table: &OrderTable, snapshot: &AllocationSnapshot) -> CompletenessReport {
    let expected = expected_quantities(snapshot);
    let actual = table_quantities(table);

    let mut missing = Vec::new();
    for (key, exp) in &expected {
        let act = actual.get(key).copied().unwrap_or(0);
        if act < *exp {
            missing.push(QuantityGap {
                item_id: key.0.clone(),
                variant_id: key.1.clone(),
                is_gift: key.2,
                expected: *exp,
                actual: act,
            });
        }
    }
    let mut extra = Vec::new();
    for (key, act) in &actual {
        let exp = expected.get(key).copied().unwrap_or(0);
        if *act > exp {
            extra.push(QuantityGap {
                item_id: key.0.clone(),
                variant_id: key.1.clone(),
                is_gift: key.2,
                expected: exp,
                actual: *act,
            });
        }
    }

    let complete = missing.is_empty() && extra.is_empty();
    let mut messages = Vec::new();
    if !missing.is_empty() {
        messages.push(format!("排包未完成：{} 类商品尚未排入下单表", missing.len()));
    }
    if !extra.is_empty() {
        messages.push(format!("数量超出排谷结果：{} 类商品", extra.len()));
    }
    CompletenessReport {
        complete,
        missing,
        extra,
        messages,
    }
}
