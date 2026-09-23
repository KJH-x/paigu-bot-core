use crate::settlement::model::{
    Line, LineSettlement, OrderTable, PackageGift, PackageSettlement, SettlementResult,
};
use crate::settlement::{DiscountKind, PricingMode, ScopeMode, SettlementConfig};

/// 结算评估（2026-09-17 定稿口径，见 `docs/DECISIONS.md` §E / `REQUIREMENTS.md` §4）。
///
/// - `C` = 无折扣商品总价（实购商品标价合计，不含特典）
/// - `B` = Σ 各下单包实付价（折后，不含特典）
/// - `G` = Σ 已授予特典价 = `Σ_X min(claimed_X, P) × price_X`（`P` = 下单包数）
/// - 总优惠额 `D = C − B + G`，按各商品自身标价**加权分摊到每个商品（以「件」为单位）**
/// - 校验式：`Σ 减均后商品总价 + G = B`
pub fn evaluate(config: &SettlementConfig, table: &OrderTable) -> SettlementResult {
    let mut warnings: Vec<String> = Vec::new();
    let count = table.packages.len();

    let mut gross = vec![0i64; count];
    let mut adjusted = vec![0i64; count];
    let mut gift_line_value = vec![0i64; count];
    let mut line_count = vec![0usize; count];

    // 扁平化：行索引 -> (包索引, 行)
    let mut flat: Vec<(usize, &Line)> = Vec::new();

    for (i, pkg) in table.packages.iter().enumerate() {
        line_count[i] = pkg.lines.len();
        for line in &pkg.lines {
            flat.push((i, line));
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

    // ---- 折扣（口径不变） ----
    let scope_amount: Vec<i64> = (0..count)
        .map(|i| match config.scope_mode {
            ScopeMode::IncludeGift => gross[i].saturating_add(gift_line_value[i]),
            ScopeMode::ExcludeGift => gross[i],
        })
        .collect();

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

    // ---- 特典授予（成几开几：granted = min(认购数, 下单包数 P)） ----
    let p = count as u32;
    let mut gift_value = vec![0i64; count];
    let mut gift_count = vec![0u32; count];
    let mut gift_list: Vec<PackageGift> = Vec::new();
    for tier in &config.gift_tiers {
        let granted = tier.claimed.min(p);
        if tier.claimed > p {
            warnings.push(format!(
                "特典 {} 认购 {} 份 > 下单包数 {}，超出部分掉落（不付款、不参与减均）",
                tier.tier_id, tier.claimed, p
            ));
        }
        for i in 0..granted as usize {
            gift_value[i] = gift_value[i].saturating_add(tier.unit_price);
            gift_count[i] = gift_count[i].saturating_add(1);
            gift_list.push(PackageGift {
                package_id: table.packages[i].package_id.clone(),
                tier_id: tier.tier_id.clone(),
                gift_name: tier.gift_name.clone(),
                quantity: 1,
                unit_price_cents: tier.unit_price,
            });
        }
    }
    let gift_valuation_total = gift_value.iter().copied().fold(0i64, i64::saturating_add);

    // ---- C / B / D ----
    let list_total: i64 = flat
        .iter()
        .filter(|(_, l)| !l.is_gift)
        .map(|(_, l)| l.total_cents())
        .fold(0i64, i64::saturating_add);
    let paid_total: i64 = (0..count)
        .map(|i| gross[i].saturating_sub(discount[i]).max(0))
        .fold(0i64, i64::saturating_add);
    let to_reduce = list_total
        .saturating_sub(paid_total)
        .saturating_add(gift_valuation_total);

    // ---- 减均：按「件 × 标价」加权把 D 分摊到每个商品 ----
    let mut unit_weights: Vec<(usize, i64)> = Vec::new();
    for (li, (_, line)) in flat.iter().enumerate() {
        if line.is_gift {
            continue;
        }
        let w = line.unit_price_cents.max(0);
        for _ in 0..line.qty {
            unit_weights.push((li, w));
        }
    }
    let mut line_reduce = vec![0i64; flat.len()];
    let weight_sum: i64 = unit_weights
        .iter()
        .map(|(_, w)| *w)
        .fold(0i64, i64::saturating_add);
    if to_reduce > 0 && weight_sum <= 0 {
        warnings.push("减均基数为 0 或无实购商品，无法分摊优惠额".to_string());
    }
    for (li, share) in largest_remainder(to_reduce, &unit_weights) {
        line_reduce[li] = line_reduce[li].saturating_add(share);
    }

    // ---- 汇总：逐行 + 逐包 ----
    let mut lines: Vec<LineSettlement> = Vec::with_capacity(flat.len());
    let mut reduce = vec![0i64; count];
    for (li, (pi, line)) in flat.iter().enumerate() {
        let total = line.total_cents();
        let red = if line.is_gift { 0 } else { line_reduce[li] };
        let final_total = total.saturating_sub(red);
        let final_unit = if line.qty > 0 {
            round_div(final_total, line.qty as i64)
        } else {
            0
        };
        reduce[*pi] = reduce[*pi].saturating_add(red);
        lines.push(LineSettlement {
            package_id: table.packages[*pi].package_id.clone(),
            item_id: line.item_id.clone(),
            variant_id: line.variant_id.clone(),
            qty: line.qty,
            unit_price_cents: line.unit_price_cents,
            total_cents: total,
            reduce_cents: red,
            final_total_cents: final_total,
            final_unit_cents: final_unit,
        });
    }
    let reduce_average_total = reduce.iter().copied().fold(0i64, i64::saturating_add);

    // ---- 校验：Σ 减均后商品总价 + G = B ----
    let final_total_sum: i64 = lines
        .iter()
        .map(|l| l.final_total_cents)
        .fold(0i64, i64::saturating_add);
    if final_total_sum.saturating_add(gift_valuation_total) != paid_total {
        warnings.push(format!(
            "减均校验失败：Σ减均后商品总价({}) + 特典价({}) ≠ 实付价({})",
            final_total_sum, gift_valuation_total, paid_total
        ));
    }

    let mut packages = Vec::with_capacity(count);
    for i in 0..count {
        let payable = gross[i].saturating_sub(discount[i]);
        if payable < 0 {
            warnings.push(format!(
                "下单包 {} 折扣超过折前金额，应付按 0 计",
                table.packages[i].package_id
            ));
        }
        if reduce[i] > gross[i] {
            warnings.push(format!(
                "下单包 {} 减均额超过标价合计",
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
            gift_count: gift_count[i],
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
        list_total_cents: list_total,
        paid_total_cents: paid_total,
        lines,
    }
}

fn round_div(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    let half = denominator / 2;
    if numerator >= 0 {
        (numerator + half) / denominator
    } else {
        (numerator - half) / denominator
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
            .then(
                table.packages[a]
                    .package_id
                    .cmp(&table.packages[b].package_id),
            )
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
    order.sort_by(|&a, &b| {
        shares[b]
            .2
            .cmp(&shares[a].2)
            .then(shares[a].0.cmp(&shares[b].0))
    });
    for &k in &order {
        if leftover <= 0 {
            break;
        }
        shares[k].1 += 1;
        leftover -= 1;
    }
    shares.into_iter().map(|(i, q, _)| (i, q)).collect()
}
