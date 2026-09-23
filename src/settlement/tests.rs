use super::*;

use crate::domain::allocation::{
    BoxAllocation, ItemAllocation, SingleAllocation, SlotAllocation, SlotStatus,
};
use crate::domain::claim::SlotPolicy;
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;

fn line(item: &str, qty: u32, price: i64) -> Line {
    Line::new(item, qty, price)
}

fn pkg(id: &str, lines: Vec<Line>) -> Package {
    Package::new(id, lines)
}

fn table(packages: Vec<Package>) -> OrderTable {
    OrderTable::new(packages)
}

fn pricing(item: &str, variant: Option<&str>, mode: PricingMode, value: i64) -> PricingEntry {
    PricingEntry {
        item_id: item.to_string(),
        variant_id: variant.map(|v| v.to_string()),
        mode,
        value,
    }
}

fn discount(
    kind: DiscountKind,
    amount: i64,
    threshold: Option<i64>,
    ratio_ppm: Option<i64>,
    shares: i64,
) -> DiscountEntry {
    DiscountEntry {
        rule_id: "rule".to_string(),
        kind,
        amount,
        threshold,
        ratio_ppm,
        shares,
    }
}

fn tier_claimed(id: &str, price: i64, claimed: u32) -> GiftTier {
    GiftTier {
        tier_id: id.to_string(),
        threshold: 0,
        gift_name: format!("特典-{id}"),
        unit_price: price,
        claimed,
    }
}

#[test]
fn pricing_adjust_by_only_changes_adjusted_column() {
    let mut cfg = SettlementConfig::default();
    cfg.pricing
        .push(pricing("a", None, PricingMode::AdjustBy, 100));
    let t = table(vec![pkg("p1", vec![line("a", 2, 1000)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].gross_cents, 2000);
    assert_eq!(r.packages[0].adjusted_cents, 2200);
    assert_eq!(r.packages[0].payable_cents, 2000);
    assert_eq!(r.grand_total, 2000);
}

#[test]
fn pricing_set_final_overrides_base() {
    let mut cfg = SettlementConfig::default();
    cfg.pricing
        .push(pricing("a", None, PricingMode::SetFinal, 800));
    let t = table(vec![pkg("p1", vec![line("a", 3, 1000)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].gross_cents, 3000);
    assert_eq!(r.packages[0].adjusted_cents, 2400);
}

#[test]
fn pricing_matches_specific_variant_only() {
    let mut cfg = SettlementConfig::default();
    cfg.pricing
        .push(pricing("a", Some("v1"), PricingMode::SetFinal, 500));
    let t = table(vec![pkg(
        "p1",
        vec![
            line("a", 1, 1000).variant("v1"),
            line("a", 1, 1000).variant("v2"),
        ],
    )]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].adjusted_cents, 1500);
    assert_eq!(r.packages[0].gross_cents, 2000);
}

#[test]
fn discount_shares_selects_first_n_after_sorting() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts
        .push(discount(DiscountKind::WholeOrder, 100, None, None, 1));
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 100)]),
        pkg("p2", vec![line("a", 1, 300)]),
        pkg("p3", vec![line("a", 1, 200)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.discount_total, 100);
    assert_eq!(r.packages[0].discount_cents, 0);
    assert_eq!(r.packages[1].discount_cents, 100);
    assert_eq!(r.packages[2].discount_cents, 0);
}

#[test]
fn discount_shares_minus_one_applies_to_all() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts
        .push(discount(DiscountKind::WholeOrder, 100, None, None, -1));
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 100)]),
        pkg("p2", vec![line("a", 1, 300)]),
        pkg("p3", vec![line("a", 1, 200)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.discount_total, 100);
    let sum: i64 = r.packages.iter().map(|p| p.discount_cents).sum();
    assert_eq!(sum, 100);
    assert!(r.packages.iter().all(|p| p.discount_cents > 0));
}

#[test]
fn discount_scope_include_vs_exclude_gift() {
    let t = table(vec![pkg(
        "p1",
        vec![line("a", 1, 1000), line("g", 1, 500).gift()],
    )]);

    let mut include = SettlementConfig::default();
    include.scope_mode = ScopeMode::IncludeGift;
    include
        .discounts
        .push(discount(DiscountKind::Threshold, 500, Some(1200), None, -1));
    let ri = evaluate(&include, &t);
    assert_eq!(ri.discount_total, 500);

    let mut exclude = SettlementConfig::default();
    exclude.scope_mode = ScopeMode::ExcludeGift;
    exclude
        .discounts
        .push(discount(DiscountKind::Threshold, 500, Some(1200), None, -1));
    let re = evaluate(&exclude, &t);
    assert_eq!(re.discount_total, 0);
}

#[test]
fn gift_granting_is_min_of_claims_and_packages() {
    // A 档认购 3、B 档认购 2；P=2 → G=2A+2B；P=3 → G=3A+2B（成几开几）
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("A", 100, 3), tier_claimed("B", 200, 2)];

    let t2 = table(vec![
        pkg("p1", vec![line("a", 1, 1000)]),
        pkg("p2", vec![line("a", 1, 1000)]),
    ]);
    let r2 = evaluate(&cfg, &t2);
    assert_eq!(r2.gift_valuation_total, 2 * 100 + 2 * 200);
    assert!(r2.warnings.iter().any(|w| w.contains("掉落")));

    let t3 = table(vec![
        pkg("p1", vec![line("a", 1, 1000)]),
        pkg("p2", vec![line("a", 1, 1000)]),
        pkg("p3", vec![line("a", 1, 1000)]),
    ]);
    let r3 = evaluate(&cfg, &t3);
    assert_eq!(r3.gift_valuation_total, 3 * 100 + 2 * 200);
}

#[test]
fn moonlit_spec_g_is_twelve_times_twelve() {
    // 月行水上-更新：特典 12 份 × ¥12 = 144，P=12 → G=144
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("gift_card", 1200, 12)];
    let t = table(
        (1..=12)
            .map(|i| pkg(&format!("p{i}"), vec![line("a", i, 1000)]))
            .collect(),
    );
    let r = evaluate(&cfg, &t);
    assert_eq!(r.gift_valuation_total, 14_400);
    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(final_sum + r.gift_valuation_total, r.paid_total_cents);
}

#[test]
fn reduce_average_total_is_list_minus_paid_plus_gift() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("t", 5000, 1)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 30_000)]),
        pkg("p2", vec![line("a", 1, 10_000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.list_total_cents, 40_000);
    assert_eq!(r.paid_total_cents, 40_000);
    assert_eq!(r.gift_valuation_total, 5000);
    assert_eq!(r.reduce_average_total, 5000);
    // 权重 30000:10000 → 3750:1250
    assert_eq!(r.lines[0].reduce_cents, 3750);
    assert_eq!(r.lines[1].reduce_cents, 1250);
    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(final_sum + r.gift_valuation_total, r.paid_total_cents);
}

#[test]
fn reduce_average_accounts_for_discount() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts
        .push(discount(DiscountKind::WholeOrder, 10_000, None, None, -1));
    cfg.gift_tiers = vec![tier_claimed("t", 5000, 1)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 50_000)]),
        pkg("p2", vec![line("a", 1, 50_000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.list_total_cents, 100_000);
    assert_eq!(r.paid_total_cents, 90_000);
    assert_eq!(r.grand_total, 90_000);
    // D = C − B + G = 100000 − 90000 + 5000
    assert_eq!(r.reduce_average_total, 15_000);
    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(final_sum + r.gift_valuation_total, r.paid_total_cents);
}

#[test]
fn reduce_average_largest_remainder_conserves_total() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("t", 100, 1)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 2000)]),
        pkg("p2", vec![line("a", 1, 1000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.reduce_average_total, 100);
    assert_eq!(r.lines[0].reduce_cents, 67);
    assert_eq!(r.lines[1].reduce_cents, 33);
    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(final_sum + r.gift_valuation_total, r.paid_total_cents);
}

#[test]
fn reduce_average_is_per_piece_and_keeps_integer_cents() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("t", 1, 1)];
    let t = table(vec![pkg("p1", vec![line("a", 2, 2500)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.list_total_cents, 5000);
    assert_eq!(r.reduce_average_total, 1);
    assert_eq!(r.lines[0].total_cents, 5000);
    assert_eq!(r.lines[0].reduce_cents, 1);
    assert_eq!(r.lines[0].final_total_cents, 4999);
    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(final_sum + r.gift_valuation_total, r.paid_total_cents);
}

#[test]
fn reduce_average_zero_basis_warns_and_checks_out() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier_claimed("t", 500, 1)];
    let t = table(vec![pkg("p1", vec![line("a", 1, 0)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.paid_total_cents, 0);
    assert_eq!(r.reduce_average_total, 0);
    assert!(r.warnings.iter().any(|w| w.contains("减均基数")));
    // 基数为 0 时校验式无法成立，必须显式告警
    assert!(r.warnings.iter().any(|w| w.contains("减均校验失败")));
}

#[test]
fn threshold_discount_applies_per_package() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts
        .push(discount(DiscountKind::Threshold, 100, Some(1000), None, -1));
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 1000)]),
        pkg("p2", vec![line("a", 1, 2000)]),
        pkg("p3", vec![line("a", 1, 500)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].discount_cents, 100);
    assert_eq!(r.packages[1].discount_cents, 100);
    assert_eq!(r.packages[2].discount_cents, 0);
    assert_eq!(r.discount_total, 200);
}

#[test]
fn whole_order_ratio_allocates_proportionally() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts.push(discount(
        DiscountKind::WholeOrder,
        0,
        None,
        Some(100_000),
        -1,
    ));
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 1000)]),
        pkg("p2", vec![line("a", 1, 3000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.discount_total, 400);
    assert_eq!(r.packages[0].discount_cents, 100);
    assert_eq!(r.packages[1].discount_cents, 300);
}

#[test]
fn discount_uses_base_price_not_adjusted_price() {
    let mut cfg = SettlementConfig::default();
    cfg.pricing
        .push(pricing("a", None, PricingMode::SetFinal, 100));
    cfg.discounts
        .push(discount(DiscountKind::WholeOrder, 50, None, None, -1));
    let t = table(vec![pkg("p1", vec![line("a", 1, 1000)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].adjusted_cents, 100);
    assert_eq!(r.packages[0].gross_cents, 1000);
    assert_eq!(r.discount_total, 50);
    assert_eq!(r.packages[0].payable_cents, 950);
}

#[test]
fn payable_is_clamped_and_warned_when_discount_exceeds_gross() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts
        .push(discount(DiscountKind::WholeOrder, 200, None, None, -1));
    let t = table(vec![pkg("p1", vec![line("a", 1, 100)])]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].payable_cents, 0);
    assert_eq!(r.grand_total, 0);
    assert!(!r.warnings.is_empty());
}

#[test]
fn order_table_from_allocation_aggregates_by_user_and_claim() {
    let snapshot = AllocationSnapshot {
        round_id: RoundId("r1".to_string()),
        version: 1,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![
            ItemAllocation {
                item_id: ItemId("a".to_string()),
                item_name: "A".to_string(),
                kind: "split".to_string(),
                variant_id: Some("v1".to_string()),
                boxes: vec![BoxAllocation {
                    box_index: 0,
                    slots: vec![
                        SlotAllocation {
                            slot_index: 0,
                            user_id: Some(UserId("u1".to_string())),
                            claim_id: Some(ClaimId("c1".to_string())),
                            claim_line_index: Some(0),
                            status: SlotStatus::Filled,
                            slot_policy: SlotPolicy::Normal,
                            segment_id: None,
                            lock_reason: None,
                        },
                        SlotAllocation {
                            slot_index: 1,
                            user_id: Some(UserId("u1".to_string())),
                            claim_id: Some(ClaimId("c1".to_string())),
                            claim_line_index: Some(0),
                            status: SlotStatus::Filled,
                            slot_policy: SlotPolicy::Normal,
                            segment_id: None,
                            lock_reason: None,
                        },
                    ],
                }],
                singles: vec![],
                waiting: vec![],
            },
            ItemAllocation {
                item_id: ItemId("g".to_string()),
                item_name: "G".to_string(),
                kind: "gift".to_string(),
                variant_id: None,
                boxes: vec![],
                singles: vec![SingleAllocation {
                    user_id: UserId("u1".to_string()),
                    claim_id: ClaimId("c1".to_string()),
                    item_id: ItemId("g".to_string()),
                    quantity: 1,
                    unit_price: MoneyCents(0),
                }],
                waiting: vec![],
            },
        ],
        user_summaries: vec![],
        warnings: vec![],
    };
    let prices = vec![
        UnitPrice {
            item_id: "a".to_string(),
            variant_id: Some("v1".to_string()),
            unit_price_cents: 1000,
        },
        UnitPrice {
            item_id: "g".to_string(),
            variant_id: None,
            unit_price_cents: 500,
        },
    ];

    let t = order_table_from_allocation(&snapshot, &prices);
    assert_eq!(t.packages.len(), 1);
    assert_eq!(t.packages[0].package_id, "u1#c1");
    let lines = &t.packages[0].lines;
    assert_eq!(lines.len(), 2);
    let a = lines.iter().find(|l| l.item_id == "a").unwrap();
    assert_eq!(a.qty, 2);
    assert_eq!(a.unit_price_cents, 1000);
    assert!(!a.is_gift);
    let g = lines.iter().find(|l| l.item_id == "g").unwrap();
    assert_eq!(g.qty, 1);
    assert_eq!(g.unit_price_cents, 500);
    assert!(g.is_gift);
}

#[test]
fn largest_remainder_conserves_with_zero_weight() {
    let shares = super::engine::largest_remainder(7, &[(0, 0), (1, 0)]);
    assert_eq!(shares, vec![(0, 0), (1, 0)]);
}

fn alloc_snapshot(item: &str, qty: u32) -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: RoundId("r1".to_string()),
        version: 1,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![ItemAllocation {
            item_id: ItemId(item.to_string()),
            item_name: item.to_string(),
            kind: "split".to_string(),
            variant_id: Some("v1".to_string()),
            boxes: vec![],
            singles: vec![SingleAllocation {
                user_id: UserId("u1".to_string()),
                claim_id: ClaimId("c1".to_string()),
                item_id: ItemId(item.to_string()),
                quantity: qty,
                unit_price: MoneyCents(0),
            }],
            waiting: vec![],
        }],
        user_summaries: vec![],
        warnings: vec![],
    }
}

#[test]
fn completeness_detects_missing_and_extra() {
    let snapshot = alloc_snapshot("a", 3);

    let ok = table(vec![pkg(
        "p1",
        vec![
            line("a", 2, 100).variant("v1"),
            line("a", 1, 100).variant("v1"),
        ],
    )]);
    let report = check_completeness(&ok, &snapshot);
    assert!(report.complete, "{report:?}");

    let short = table(vec![pkg("p1", vec![line("a", 1, 100).variant("v1")])]);
    let report = check_completeness(&short, &snapshot);
    assert!(!report.complete);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0].expected, 3);
    assert_eq!(report.missing[0].actual, 1);

    let over = table(vec![pkg("p1", vec![line("a", 5, 100).variant("v1")])]);
    let report = check_completeness(&over, &snapshot);
    assert!(!report.complete);
    assert_eq!(report.extra.len(), 1);
    assert_eq!(report.extra[0].expected, 3);
    assert_eq!(report.extra[0].actual, 5);
}

/// 月行水上-更新（用户 2026-09-17 定稿数据）：
/// 12 单、特典 12 份 × ¥12 = ¥144，标价合计 ¥3285（= Sheet2 参考 ¥3270 + 风尚速递SP 拼套 ¥15）。
#[test]
fn moonlit_spec_update_fixture_matches_reference() {
    let path = std::path::Path::new("simulation-corpus/real-xlsx/月行水上-更新/fixture.json");
    if !path.exists() {
        eprintln!("跳过：夹具不存在（xlsx 为 gitignored 输入，可运行 build_fixture.py 重建）");
        return;
    }
    let raw = std::fs::read_to_string(path).expect("read fixture");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse fixture");

    let table: OrderTable = serde_json::from_value(v["order_table"].clone()).expect("order_table");
    let config: SettlementConfig =
        serde_json::from_value(v["settlement_config"].clone()).expect("settlement_config");

    let r = evaluate(&config, &table);

    assert_eq!(table.packages.len(), 12, "P 应为 12 单");
    assert_eq!(r.gift_valuation_total, 14_400, "G = 12 × ¥12");
    assert_eq!(r.list_total_cents, 328_500, "C = ¥3285");
    assert_eq!(r.paid_total_cents, 328_500, "无折扣：B = C");
    assert_eq!(r.reduce_average_total, 14_400, "D = C − B + G = G");

    let final_sum: i64 = r.lines.iter().map(|l| l.final_total_cents).sum();
    assert_eq!(
        final_sum + r.gift_valuation_total,
        r.paid_total_cents,
        "校验式 Σfinal + G = B"
    );

    // 与 Sheet2「单领（报盒）」参考价逐单对照（第1单另含 风尚速递SP 拼套 +¥15）
    let per_order: Vec<i64> = v["reference"]["per_order_cents"]
        .as_array()
        .expect("per_order")
        .iter()
        .map(|x| x.as_i64().expect("cents"))
        .collect();
    assert_eq!(per_order.len(), 12);
    assert_eq!(v["reference"]["total_cents"].as_i64(), Some(327_000));
    for (i, expected) in per_order.iter().enumerate() {
        let delta = if i == 0 { 1_500 } else { 0 };
        assert_eq!(
            r.packages[i].gross_cents,
            expected + delta,
            "第{}单 标价合计",
            i + 1
        );
    }
}
