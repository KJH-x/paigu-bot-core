use std::collections::HashMap;

use crate::domain::ids::{ItemId, UserId};
use crate::domain::item::Item;
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::settlement::{SettlementSnapshot, UserBill, UserBillLine, ItemTotal, DiscountApplication};
use crate::domain::discount::{DiscountRule, DiscountScope, DiscountAllocationPolicy};
use crate::domain::money::MoneyCents;
use crate::error::SettlementError;

pub struct SettlementInput {
    pub allocation: AllocationSnapshot,
    pub items: Vec<Item>,
    pub discount_rules: Vec<DiscountRule>,
    pub gift_valuations: Vec<crate::domain::gift::GiftAllocation>,
}

pub struct SettlementEngine {}

impl SettlementEngine {
    pub fn new() -> Self {
        Self {}
    }

    pub fn settle(&self, input: &SettlementInput) -> Result<SettlementSnapshot, SettlementError> {
        let item_map: HashMap<ItemId, &Item> = input.items.iter().map(|i| (i.item_id.clone(), i)).collect();
        let mut bills = self.build_user_bills(&input.allocation, &item_map);

        let gross_total: MoneyCents = bills.iter()
            .map(|b| b.gross_total)
            .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));

        let mut discount_applications = Vec::new();

        for rule in &input.discount_rules {
            self.apply_discount_rule(rule, &mut bills, &input.allocation, &item_map, &mut discount_applications);
        }

        let discount_total: MoneyCents = discount_applications.iter()
            .map(|d| d.amount)
            .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));

        for bill in &mut bills {
            bill.final_total = bill.gross_total
                .checked_sub(bill.discount_share)
                .unwrap_or(MoneyCents::zero())
                .checked_sub(bill.gift_value_share)
                .unwrap_or(MoneyCents::zero())
                .checked_add(bill.shipping_fee)
                .unwrap_or(MoneyCents::zero());

            if bill.final_total.0 < 0 {
                bill.final_total = MoneyCents::zero();
            }
        }

        let final_total: MoneyCents = bills.iter()
            .map(|b| b.final_total)
            .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));

        let item_totals: Vec<ItemTotal> = item_map.iter().map(|(id, item)| {
            let total_qty: u32 = bills.iter()
                .flat_map(|b| &b.lines)
                .filter(|l| &l.item_id == id)
                .map(|l| l.quantity)
                .sum();

            ItemTotal {
                item_id: id.clone(),
                item_name: item.name.clone(),
                kind: item.kind.as_str().to_string(),
                total_quantity: total_qty,
                unit_price: item.unit_price,
                gross_total: item.unit_price.checked_mul_u32(total_qty).unwrap_or(MoneyCents::zero()),
                box_count: 0,
                incomplete_box_count: 0,
                gift_quantity: 0,
                notes: None,
            }
        }).collect();

        Ok(SettlementSnapshot {
            round_id: input.allocation.round_id.clone(),
            version: input.allocation.version,
            generated_at: chrono::Utc::now(),
            user_bills: bills,
            item_totals,
            discount_applications,
            gross_total,
            discount_total,
            final_total,
            warnings: vec![],
        })
    }

    fn build_user_bills(
        &self,
        allocation: &AllocationSnapshot,
        item_map: &HashMap<ItemId, &Item>,
    ) -> Vec<UserBill> {
        let mut bills_map: HashMap<UserId, UserBill> = HashMap::new();

        for ia in &allocation.item_allocations {
            let item = item_map.get(&ia.item_id);
            let price = item.map(|i| i.unit_price).unwrap_or(MoneyCents::zero());
            let kind = item.map(|i| i.kind.as_str().to_string()).unwrap_or_default();
            let name = item.map(|i| i.name.clone()).unwrap_or_default();

            for mbox in &ia.boxes {
                for slot in &mbox.slots {
                    if let Some(ref user_id) = slot.user_id {
                        let bill = bills_map.entry(user_id.clone()).or_insert_with(|| -> UserBill {
                            UserBill {
                            user_id: user_id.clone(),
                            display_name: String::new(),
                            lines: vec![],
                            gross_total: MoneyCents::zero(),
                            discount_share: MoneyCents::zero(),
                            gift_value_share: MoneyCents::zero(),
                            shipping_fee: MoneyCents::zero(),
                            final_total: MoneyCents::zero(),
                            payment_status: crate::domain::settlement::PaymentStatus::Unpaid,
                            }
                        });

                        if let Some(existing) = bill.lines.iter_mut().find(|l| l.item_id == ia.item_id) {
                            existing.quantity += 1;
                            existing.gross = existing.gross.checked_add(price).unwrap_or(existing.gross);
                        } else {
                            bill.lines.push(UserBillLine {
                                item_id: ia.item_id.clone(),
                                item_name: name.clone(),
                                kind: kind.clone(),
                                quantity: 1,
                                unit_price: price,
                                gross: price,
                            });
                        }
                    }
                }
            }

            for sa in &ia.singles {
                let bill = bills_map.entry(sa.user_id.clone()).or_insert_with(|| -> UserBill {
                    UserBill {
                    user_id: sa.user_id.clone(),
                    display_name: String::new(),
                    lines: vec![],
                    gross_total: MoneyCents::zero(),
                    discount_share: MoneyCents::zero(),
                    gift_value_share: MoneyCents::zero(),
                    shipping_fee: MoneyCents::zero(),
                    final_total: MoneyCents::zero(),
                    payment_status: crate::domain::settlement::PaymentStatus::Unpaid,
                    }
                });

                bill.lines.push(UserBillLine {
                    item_id: ia.item_id.clone(),
                    item_name: name.clone(),
                    kind: kind.clone(),
                    quantity: sa.quantity,
                    unit_price: price,
                    gross: price.checked_mul_u32(sa.quantity).unwrap_or(MoneyCents::zero()),
                });
            }
        }

        for bill in bills_map.values_mut() {
            bill.gross_total = bill.lines.iter()
                .map(|l| l.gross)
                .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));
        }

        bills_map.into_values().collect()
    }

    fn apply_discount_rule(
        &self,
        rule: &DiscountRule,
        bills: &mut Vec<UserBill>,
        _allocation: &AllocationSnapshot,
        _item_map: &HashMap<ItemId, &Item>,
        discount_applications: &mut Vec<DiscountApplication>,
    ) {
        match rule {
            DiscountRule::ThresholdDiscount { rule_id, threshold, discount, repeatable, scope, stackable: _ } => {
                let basis = self.compute_user_basis(bills, scope);
                let scoped_total = basis.iter().map(|(_, m)| m.as_cents()).sum::<i64>();
                let times = if *repeatable && threshold.as_cents() > 0 {
                    scoped_total / threshold.as_cents()
                } else if scoped_total >= threshold.as_cents() {
                    1
                } else {
                    0
                };

                if times > 0 {
                    let total_discount = discount.checked_mul_i64(times).unwrap_or(*discount);
                    let shares = allocate_discount_by_ratio(total_discount, &basis);
                    self.apply_discount_shares(bills, &shares);
                    discount_applications.push(DiscountApplication {
                        rule_id: rule_id.clone(),
                        rule_type: "threshold_discount".to_string(),
                        amount: total_discount,
                        shares,
                    });
                }
            }
            DiscountRule::FixedActualDiscount { rule_id, amount, scope, allocation_policy } => {
                let basis = self.compute_user_basis(bills, scope);
                let shares = match allocation_policy {
                    DiscountAllocationPolicy::ByGrossAmountRatio => {
                        allocate_discount_by_ratio(*amount, &basis)
                    }
                    DiscountAllocationPolicy::ByQuantityRatio => {
                        allocate_discount_by_quantity(*amount, bills, scope)
                    }
                    DiscountAllocationPolicy::EqualByUser => {
                        allocate_discount_equal(*amount, &basis)
                    }
                    DiscountAllocationPolicy::Manual(shares) => {
                        shares.iter().map(|s| crate::domain::settlement::DiscountShare {
                            user_id: s.user_id.clone(),
                            amount: s.amount,
                        }).collect()
                    }
                };
                self.apply_discount_shares(bills, &shares);
                discount_applications.push(DiscountApplication {
                    rule_id: rule_id.clone(),
                    rule_type: "fixed_discount".to_string(),
                    amount: *amount,
                    shares,
                });
            }
            DiscountRule::ShoppingFund { rule_id, amount, allocation_policy } => {
                let basis = self.compute_user_basis(bills, &DiscountScope::AllPaidItems);
                let shares = match allocation_policy {
                    DiscountAllocationPolicy::ByGrossAmountRatio => {
                        allocate_discount_by_ratio(*amount, &basis)
                    }
                    _ => allocate_discount_by_ratio(*amount, &basis),
                };
                self.apply_discount_shares(bills, &shares);
                discount_applications.push(DiscountApplication {
                    rule_id: rule_id.clone(),
                    rule_type: "shopping_fund".to_string(),
                    amount: *amount,
                    shares,
                });
            }
            DiscountRule::GiftByThreshold { rule_id: _, threshold, gift_item_id: _, gift_quantity_per_threshold, gift_valuation, allocation_policy: _, value_offset_policy: _ } => {
                let basis = self.compute_user_basis(bills, &DiscountScope::AllPaidItems);
                let scoped_total = basis.iter().map(|(_, m)| m.as_cents()).sum::<i64>();
                let gift_count = if threshold.as_cents() > 0 {
                    (scoped_total / threshold.as_cents()) as u32 * gift_quantity_per_threshold
                } else {
                    0
                };
                let total_gift_value = gift_valuation.checked_mul_u32(gift_count).unwrap_or(MoneyCents::zero());
                let shares = allocate_discount_by_ratio(total_gift_value, &basis);
                for bill in bills.iter_mut() {
                    if let Some(share) = shares.iter().find(|s| &s.user_id == &bill.user_id) {
                        bill.gift_value_share = bill.gift_value_share.checked_add(share.amount).unwrap_or(bill.gift_value_share);
                    }
                }
            }
        }
    }

    fn compute_user_basis(&self, bills: &[UserBill], scope: &DiscountScope) -> Vec<(UserId, MoneyCents)> {
        match scope {
            DiscountScope::AllPaidItems => {
                bills.iter().map(|b| (b.user_id.clone(), b.gross_total)).collect()
            }
            DiscountScope::ItemIds(item_ids) => {
                bills.iter().map(|b| {
                    let relevant: MoneyCents = b.lines.iter()
                        .filter(|l| item_ids.contains(&l.item_id))
                        .map(|l| l.gross)
                        .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));
                    (b.user_id.clone(), relevant)
                }).collect()
            }
            DiscountScope::ItemKinds(kinds) => {
                bills.iter().map(|b| {
                    let relevant: MoneyCents = b.lines.iter()
                        .filter(|l| kinds.contains(&l.kind))
                        .map(|l| l.gross)
                        .fold(MoneyCents::zero(), |a, b| a.checked_add(b).unwrap_or(a));
                    (b.user_id.clone(), relevant)
                }).collect()
            }
        }
    }

    fn apply_discount_shares(&self, bills: &mut Vec<UserBill>, shares: &[crate::domain::settlement::DiscountShare]) {
        for share in shares {
            if let Some(bill) = bills.iter_mut().find(|b| b.user_id == share.user_id) {
                bill.discount_share = bill.discount_share.checked_add(share.amount).unwrap_or(bill.discount_share);
            }
        }
    }
}

pub fn allocate_discount_by_ratio(
    total_discount: MoneyCents,
    user_basis: &[(UserId, MoneyCents)],
) -> Vec<crate::domain::settlement::DiscountShare> {
    let basis_sum: i64 = user_basis.iter().map(|(_, m)| m.0).sum();
    if basis_sum <= 0 || total_discount.0 <= 0 {
        return user_basis.iter().map(|(u, _)| crate::domain::settlement::DiscountShare {
            user_id: u.clone(),
            amount: MoneyCents::zero(),
        }).collect();
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

    shares.into_iter().map(|(u, cents, _)| crate::domain::settlement::DiscountShare {
        user_id: u,
        amount: MoneyCents(cents),
    }).collect()
}

pub fn allocate_discount_by_quantity(
    total_discount: MoneyCents,
    bills: &[UserBill],
    scope: &DiscountScope,
) -> Vec<crate::domain::settlement::DiscountShare> {
    let quantities: Vec<(UserId, u32)> = bills.iter().map(|b| {
        let qty = match scope {
            DiscountScope::AllPaidItems => b.lines.iter().map(|l| l.quantity).sum(),
            DiscountScope::ItemIds(ids) => b.lines.iter().filter(|l| ids.contains(&l.item_id)).map(|l| l.quantity).sum(),
            DiscountScope::ItemKinds(kinds) => b.lines.iter().filter(|l| kinds.contains(&l.kind)).map(|l| l.quantity).sum(),
        };
        (b.user_id.clone(), qty)
    }).collect();

    let total_qty: u32 = quantities.iter().map(|(_, q)| q).sum();
    if total_qty == 0 {
        return quantities.iter().map(|(u, _)| crate::domain::settlement::DiscountShare {
            user_id: u.clone(),
            amount: MoneyCents::zero(),
        }).collect();
    }

    let basis: Vec<(UserId, MoneyCents)> = quantities.iter()
        .map(|(u, q)| (u.clone(), MoneyCents(*q as i64 * 100)))
        .collect();

    allocate_discount_by_ratio(total_discount, &basis)
}

pub fn allocate_discount_equal(
    total_discount: MoneyCents,
    user_basis: &[(UserId, MoneyCents)],
) -> Vec<crate::domain::settlement::DiscountShare> {
    if user_basis.is_empty() {
        return vec![];
    }
    let per_user = total_discount.0 / user_basis.len() as i64;
    let remainder = total_discount.0 % user_basis.len() as i64;
    user_basis.iter().enumerate().map(|(i, (u, _))| {
        let amount = if i == 0 { per_user + remainder } else { per_user };
        crate::domain::settlement::DiscountShare {
            user_id: u.clone(),
            amount: MoneyCents(amount),
        }
    }).collect()
}
