use crate::settlement::model::{
    Line, OrderTable, PackageGift, PackageSettlement, SettlementResult,
};
use crate::settlement::{DiscountKind, PricingMode, ScopeMode, SettlementConfig};

pub fn evaluate(config: &SettlementConfig, table: &OrderTable) -> SettlementResult {
    let mut warnings: Vec<String> = Vec::new();
    let count = table.packages.len();

    let mut gross = vec![0i64; count];
    let mut adjusted = vec![0i64; count];
    let mut gift_line_value = vec![0i64; count];
    let mut line_count = vec![0usize; count];

    for (i, pkg) in table.packages.iter().enumerate() {
        line_count[i] = pkg.lines.len();
        for line in &pkg.lines {
            let total = line.total_cents();
            if line.is_gift {
                gift_line_value[i] = gift_line_value[i].saturating_add(total);
            } else {
                gross[i] = gross[i].saturating_add(total);
                adjusted[i] = adjusted[i].saturating_add(
                    adjusted_unit_price(config, line).saturating_mul(line.qty as i64),
                );
            }
        }
    }

    let scope_amount: Vec<i64> = (0..count)
        .map(|i| match config.scope_mode {
            ScopeMode::IncludeGift => gross[i].saturating_add(gift_line_value[i]),
            ScopeMode::ExcludeGift => gross[i],
        })
        .collect();

    let mut gift_hits: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut gift_value = vec![0i64; count];
    for i in 0..count {
        for (tier_index, tier) in config.gift_tiers.iter().enumerate() {
            if gross[i] >= tier.threshold {
                gift_hits[i].push(tier_index);
                gift_value[i] = gift_value[i].saturating_add(tier.unit_price);
            }
        }
    }
    let gift_valuation_total = gift_value.iter().copied().fold(0i64, i64::saturating_add);

    let mut gift_list: Vec<PackageGift> = Vec::new();
    for i in 0..count {
        for &tier_index in &gift_hits[i] {
            let tier = &config.gift_tiers[tier_index];
            gift_list.push(PackageGift {
                package_id: table.packages[i].package_id.clone(),
                tier_id: tier.tier_id.clone(),
                gift_name: tier.gift_name.clone(),
                quantity: 1,
                unit_price_cents: tier.unit_price,
            });
        }
    }

    let mut discount = vec![0i64; count];
    for entry in &config.discounts {
        let selected = select_packages(entry.shares, &scope_amount, &line_count, table);
        match entry.kind {
            DiscountKind::Threshold => {
                let threshold = entry.threshold.unwrap_or(0);
                if entry.threshold.is_none() {
                    warnings.push(format!(
                        "折扣 {} 缺少满减门槛 threshold，按 0 处理",
                        entry.rule_id
                    ));
                }
                for &i in &selected {
                    if scope_amount[i] >= threshold {
                        discount[i] = discount[i].saturating_add(entry.amount);
                    }
                }
            }
            DiscountKind::WholeOrder => {
                let scoped_total = selected
                    .iter()
                    .map(|&i| scope_amount[i])
                    .fold(0i64, i64::saturating_add);
                let total = match entry.ratio_ppm {
                    Some(ppm) => ((scoped_total as i128) * (ppm as i128) / 1_000_000) as i64,
                    None => entry.amount,
                };
                if total <= 0 {
                    continue;
                }
                if scoped_total <= 0 {
                    warnings.push(format!(
                        "折扣 {} 作用范围内金额为 0，无法回摊",
                        entry.rule_id
                    ));
                    continue;
                }
                let weights: Vec<(usize, i64)> =
                    selected.iter().map(|&i| (i, scope_amount[i])).collect();
                for (i, share) in largest_remainder(total, &weights) {
                    discount[i] = discount[i].saturating_add(share);
                }
            }
        }
    }
    let discount_total = discount.iter().copied().fold(0i64, i64::saturating_add);

    let weights: Vec<(usize, i64)> = (0..count)
        .map(|i| {
            let w = if config.reduce_average.include_gift_price {
                gross[i].saturating_add(gift_value[i])
            } else {
                gross[i]
            };
            (i, w)
        })
        .collect();
    let basis = weights.iter().map(|(_, w)| *w).fold(0i64, i64::saturating_add);
    let reduce_shares = if gift_valuation_total > 0 && basis > 0 {
        largest_remainder(gift_valuation_total, &weights)
    } else {
        if gift_valuation_total > 0 && basis == 0 {
            warnings.push("减均基数为 0，特典折价无法分摊".to_string());
        }
        weights.iter().map(|(i, _)| (*i, 0)).collect()
    };
    let mut reduce = vec![0i64; count];
    for (i, share) in reduce_shares {
        reduce[i] = reduce[i].saturating_add(share);
    }
    let reduce_average_total = reduce.iter().copied().fold(0i64, i64::saturating_add);

    let mut packages = Vec::with_capacity(count);
    for i in 0..count {
        let payable = gross[i].saturating_sub(discount[i]).saturating_sub(reduce[i]);
        if payable < 0 {
            warnings.push(format!(
                "下单包 {} 折扣/减均超过折前金额，应付按 0 计",
                table.packages[i].package_id
            ));
        }
        packages.push(PackageSettlement {
            package_id: table.packages[i].package_id.clone(),
            gross_cents: gross[i],
            adjusted_cents: adjusted[i],
            discount_cents: discount[i],
            reduce_average_cents: reduce[i],
            payable_cents: payable.max(0),
            gift_count: gift_hits[i].len() as u32,
            gift_value_cents: gift_value[i],
        });
    }
    let grand_total = packages
        .iter()
        .map(|p| p.payable_cents)
        .fold(0i64, i64::saturating_add);

    SettlementResult {
        packages,
        gift_list,
        gift_valuation_total,
        reduce_average_total,
        discount_total,
        grand_total,
        warnings,
    }
}

fn adjusted_unit_price(config: &SettlementConfig, line: &Line) -> i64 {
    let mut price = line.unit_price_cents;
    for entry in &config.pricing {
        if entry.item_id != line.item_id {
            continue;
        }
        if let Some(variant) = &entry.variant_id {
            if line.variant_id.as_deref() != Some(variant.as_str()) {
                continue;
            }
        }
        price = match entry.mode {
            PricingMode::AdjustBy => line.unit_price_cents.saturating_add(entry.value),
            PricingMode::SetFinal => entry.value,
        };
    }
    price
}

fn select_packages(
    shares: i64,
    scope_amount: &[i64],
    line_count: &[usize],
    table: &OrderTable,
) -> Vec<usize> {
    if shares < 0 {
        return (0..table.packages.len()).collect();
    }
    if shares == 0 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..table.packages.len()).collect();
    idx.sort_by(|&a, &b| {
        scope_amount[b]
            .cmp(&scope_amount[a])
            .then(line_count[b].cmp(&line_count[a]))
            .then(table.packages[a].package_id.cmp(&table.packages[b].package_id))
    });
    idx.truncate(shares as usize);
    idx
}

pub(crate) fn largest_remainder(total: i64, weights: &[(usize, i64)]) -> Vec<(usize, i64)> {
    if weights.is_empty() || total <= 0 {
        return weights.iter().map(|(i, _)| (*i, 0)).collect();
    }
    let weight_sum: i128 = weights.iter().map(|(_, w)| *w as i128).sum();
    if weight_sum <= 0 {
        return weights.iter().map(|(i, _)| (*i, 0)).collect();
    }
    let total_i = total as i128;
    let mut shares: Vec<(usize, i64, i128)> = weights
        .iter()
        .map(|(i, w)| {
            let numerator = total_i * (*w as i128);
            (*i, (numerator / weight_sum) as i64, numerator % weight_sum)
        })
        .collect();
    let allocated: i128 = shares.iter().map(|(_, q, _)| *q as i128).sum();
    let mut leftover = total_i - allocated;
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| shares[b].2.cmp(&shares[a].2).then(shares[a].0.cmp(&shares[b].0)));
    for &k in &order {
        if leftover <= 0 {
            break;
        }
        shares[k].1 += 1;
        leftover -= 1;
    }
    shares.into_iter().map(|(i, q, _)| (i, q)).collect()
}
