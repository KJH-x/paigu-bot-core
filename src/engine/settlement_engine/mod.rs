use std::collections::{BTreeMap, HashMap};

use crate::domain::discount::DiscountRule;
use crate::domain::ids::{ItemId, UserId};
use crate::domain::item::Item;
use crate::domain::money::MoneyCents;
use crate::domain::settlement::{
    ItemTotal, PaymentStatus, SettlementSnapshot, SettlementWarning, UserBill, UserBillLine,
};
use crate::domain::snapshot::AllocationSnapshot;
use crate::settlement::{evaluate, Line, OrderTable, Package, SettlementConfig};

pub struct SettlementInput {
    pub allocation: AllocationSnapshot,
    pub items: Vec<Item>,
    pub discount_rules: Vec<DiscountRule>,
}

pub struct SettlementEngine {}

impl Default for SettlementEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SettlementEngine {
    pub fn new() -> Self {
        Self {}
    }

    /// D-01 适配器：结算金额统一由 `settlement::evaluate`（单一计算真源）计算，
    /// 再映射回现有 `SettlementSnapshot` DTO（对外 JSON 结构不变）。
    ///
    /// - 旧 `DiscountRule` 已无生产者（无 `DiscountRulesSet` 事件）；此处不再参与计算，
    ///   若调用方仍传入则记录一条告警，避免静默丢失。
    /// - 分配结果按用户归组为下单表（`package_id = user_id`），行价为商品标价，
    ///   与旧 `build_user_bills` 口径一致；随后把 `SettlementResult.lines` 折回用户账单。
    pub fn settle(&self, input: &SettlementInput) -> SettlementSnapshot {
        let item_map: HashMap<ItemId, &Item> =
            input.items.iter().map(|i| (i.item_id.clone(), i)).collect();

        // §U8：结算阶段执行「包尾强制成盒 + 自动滑入」（幂等；分配阶段已解析时结果一致）。
        let resolved = crate::domain::allocation::resolve_tail_boxes(&input.allocation);
        let forced_tail_boxes = resolved
            .warnings
            .iter()
            .filter(|w| w.message.starts_with("包尾强制成盒"))
            .count();

        let table = allocation_to_order_table(&resolved, &item_map);
        let config = SettlementConfig::default();
        let result = evaluate(&config, &table);

        let mut bills_map: BTreeMap<String, UserBill> = BTreeMap::new();
        for line in &result.lines {
            let bill = bills_map
                .entry(line.package_id.clone())
                .or_insert_with(|| UserBill {
                    user_id: UserId(line.package_id.clone()),
                    display_name: String::new(),
                    lines: vec![],
                    gross_total: MoneyCents::zero(),
                    discount_share: MoneyCents::zero(),
                    gift_value_share: MoneyCents::zero(),
                    shipping_fee: MoneyCents::zero(),
                    final_total: MoneyCents::zero(),
                    payment_status: PaymentStatus::Unpaid,
                });

            let item = item_map.get(&ItemId(line.item_id.clone()));
            let name = item.map(|i| i.name.clone()).unwrap_or_default();
            let kind = item
                .map(|i| i.kind.as_str().to_string())
                .unwrap_or_default();

            if let Some(existing) = bill.lines.iter_mut().find(|l| l.item_id.0 == line.item_id) {
                existing.quantity = existing.quantity.saturating_add(line.qty);
                existing.gross = existing
                    .gross
                    .checked_add(MoneyCents(line.total_cents))
                    .unwrap_or(existing.gross);
            } else {
                bill.lines.push(UserBillLine {
                    item_id: ItemId(line.item_id.clone()),
                    item_name: name,
                    kind,
                    quantity: line.qty,
                    unit_price: MoneyCents(line.unit_price_cents),
                    gross: MoneyCents(line.total_cents),
                });
            }

            bill.gross_total = bill
                .gross_total
                .checked_add(MoneyCents(line.total_cents))
                .unwrap_or(bill.gross_total);
            bill.discount_share = bill
                .discount_share
                .checked_add(MoneyCents(line.reduce_cents))
                .unwrap_or(bill.discount_share);
        }

        for bill in bills_map.values_mut() {
            bill.final_total = bill
                .gross_total
                .checked_sub(bill.discount_share)
                .and_then(|v| v.checked_sub(bill.gift_value_share))
                .and_then(|v| v.checked_add(bill.shipping_fee))
                .unwrap_or(MoneyCents::zero());
            if bill.final_total.0 < 0 {
                bill.final_total = MoneyCents::zero();
            }
        }

        let bills: Vec<UserBill> = bills_map.into_values().collect();
        let gross_total = MoneyCents(result.list_total_cents);
        let discount_total = MoneyCents(result.reduce_average_total);
        let final_total = bills
            .iter()
            .map(|b| b.final_total)
            .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));

        let mut seen: Vec<ItemId> = Vec::new();
        let mut item_totals: Vec<ItemTotal> = Vec::new();
        for item in &input.items {
            if seen.contains(&item.item_id) {
                continue;
            }
            seen.push(item.item_id.clone());
            let total_qty: u32 = bills
                .iter()
                .flat_map(|b| &b.lines)
                .filter(|l| l.item_id == item.item_id)
                .map(|l| l.quantity)
                .sum();
            item_totals.push(ItemTotal {
                item_id: item.item_id.clone(),
                item_name: item.name.clone(),
                kind: item.kind.as_str().to_string(),
                total_quantity: total_qty,
                unit_price: item.unit_price,
                gross_total: item
                    .unit_price
                    .checked_mul_i64(total_qty as i64)
                    .unwrap_or(MoneyCents::zero()),
                box_count: 0,
                incomplete_box_count: 0,
                gift_quantity: 0,
                notes: None,
            });
        }

        let mut warnings: Vec<SettlementWarning> = Vec::new();
        if !input.discount_rules.is_empty() {
            warnings.push(SettlementWarning {
                user_id: None,
                message: "旧 DiscountRule 已停止参与结算；金额统一由 settlement::evaluate 计算"
                    .to_string(),
                severity: "warning".to_string(),
            });
        }
        warnings.extend(result.warnings.iter().map(|m| SettlementWarning {
            user_id: None,
            message: m.clone(),
            severity: "warning".to_string(),
        }));
        if forced_tail_boxes > 0 {
            warnings.push(SettlementWarning {
                user_id: None,
                message: format!("包尾强制成盒：{forced_tail_boxes} 个"),
                severity: "info".to_string(),
            });
        }

        SettlementSnapshot {
            round_id: input.allocation.round_id.clone(),
            version: input.allocation.version,
            generated_at: chrono::Utc::now(),
            user_bills: bills,
            item_totals,
            discount_applications: vec![],
            gross_total,
            discount_total,
            final_total,
            warnings,
        }
    }
}

/// 把排谷结果按用户归组为下单表；行价为商品标价（与旧 `build_user_bills` 一致），
/// 所有行按 `is_gift = false` 处理（旧栈不含特典折价语义）。
fn allocation_to_order_table(
    allocation: &AllocationSnapshot,
    item_map: &HashMap<ItemId, &Item>,
) -> OrderTable {
    type LineKey = (String, Option<String>);
    let mut by_user: BTreeMap<String, BTreeMap<LineKey, (u32, i64)>> = BTreeMap::new();

    for ia in &allocation.item_allocations {
        let price = item_map
            .get(&ia.item_id)
            .map(|i| i.unit_price.as_cents())
            .unwrap_or(0);

        let mut push = |user_id: &UserId, qty: u32| {
            let entry = by_user.entry(user_id.0.clone()).or_default();
            let line = entry
                .entry((ia.item_id.0.clone(), ia.variant_id.clone()))
                .or_insert((0, price));
            line.0 = line.0.saturating_add(qty);
        };

        for mbox in &ia.boxes {
            for slot in &mbox.slots {
                if let Some(user_id) = &slot.user_id {
                    push(user_id, 1);
                }
            }
        }
        for sa in &ia.singles {
            push(&sa.user_id, sa.quantity);
        }
    }

    let packages = by_user
        .into_iter()
        .map(|(user, lines)| Package {
            package_id: user,
            lines: lines
                .into_iter()
                .map(|((item_id, variant_id), (qty, unit_price_cents))| Line {
                    item_id,
                    variant_id,
                    qty,
                    unit_price_cents,
                    is_gift: false,
                })
                .collect(),
        })
        .collect();

    OrderTable { packages }
}

/// 保留的旧折扣分摊工具（当前仅被 `src/tests/replay_helpers.rs` 引用）。
#[allow(dead_code)]
pub fn allocate_discount_by_ratio(
    total_discount: MoneyCents,
    user_basis: &[(UserId, MoneyCents)],
) -> Vec<crate::domain::settlement::DiscountShare> {
    let basis_sum: i64 = user_basis.iter().map(|(_, m)| m.0).sum();
    if basis_sum <= 0 || total_discount.0 <= 0 {
        return user_basis
            .iter()
            .map(|(u, _)| crate::domain::settlement::DiscountShare {
                user_id: u.clone(),
                amount: MoneyCents::zero(),
            })
            .collect();
    }

    let mut shares: Vec<(UserId, i64, i64)> = Vec::new();
    let mut allocated = 0i64;

    for (user_id, basis) in user_basis {
        let numerator = total_discount.0 * basis.0;
        let floor = numerator / basis_sum;
        let remainder = numerator % basis_sum;
        allocated += floor;
        shares.push((user_id.clone(), floor, remainder));
    }

    let mut leftover = total_discount.0 - allocated;
    shares.sort_by(|a, b| b.2.cmp(&a.2));

    for share in shares.iter_mut() {
        if leftover <= 0 {
            break;
        }
        share.1 += 1;
        leftover -= 1;
    }

    shares
        .into_iter()
        .map(|(u, cents, _)| crate::domain::settlement::DiscountShare {
            user_id: u,
            amount: MoneyCents(cents),
        })
        .collect()
}

#[cfg(test)]
mod tests;
