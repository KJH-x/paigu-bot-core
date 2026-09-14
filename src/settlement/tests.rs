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

fn discount(kind: DiscountKind, amount: i64, threshold: Option<i64>, ratio_ppm: Option<i64>, shares: i64) -> DiscountEntry {
    DiscountEntry {
        rule_id: "rule".to_string(),
        kind,
        amount,
        threshold,
        ratio_ppm,
        shares,
    }
}

fn tier(id: &str, threshold: i64, price: i64) -> GiftTier {
    GiftTier {
        tier_id: id.to_string(),
        threshold,
        gift_name: format!("特典-{id}"),
        unit_price: price,
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
        vec![line("a", 1, 1000).variant("v1"), line("a", 1, 1000).variant("v2")],
    )]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].adjusted_cents, 1500);
    assert_eq!(r.packages[0].gross_cents, 2000);
}

#[test]
fn discount_shares_selects_first_n_after_sorting() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts.push(discount(
        DiscountKind::WholeOrder,
        100,
        None,
        None,
        1,
    ));
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
    cfg.discounts.push(discount(
        DiscountKind::WholeOrder,
        100,
        None,
        None,
        -1,
    ));
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
    include.discounts.push(discount(
        DiscountKind::Threshold,
        500,
        Some(1200),
        None,
        -1,
    ));
    let ri = evaluate(&include, &t);
    assert_eq!(ri.discount_total, 500);

    let mut exclude = SettlementConfig::default();
    exclude.scope_mode = ScopeMode::ExcludeGift;
    exclude.discounts.push(discount(
        DiscountKind::Threshold,
        500,
        Some(1200),
        None,
        -1,
    ));
    let re = evaluate(&exclude, &t);
    assert_eq!(re.discount_total, 0);
}

#[test]
fn gift_tiers_stack_per_package_without_cross_package_accumulation() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![
        tier("t0", 0, 100),
        tier("t300", 30_000, 300),
        tier("t500", 50_000, 500),
    ];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 50_100)]),
        pkg("p2", vec![line("a", 1, 25_000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.packages[0].gift_count, 3);
    assert_eq!(r.packages[1].gift_count, 1);
    assert_eq!(r.gift_valuation_total, 100 + 300 + 500 + 100);
    assert_eq!(r.gift_list.len(), 4);
}

#[test]
fn gift_501_yuan_hits_three_tiers() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![
        tier("tier0", 0, 10),
        tier("tier300", 30_000, 30),
        tier("tier500", 50_000, 50),
    ];
    let t = table(vec![pkg("p501", vec![line("a", 1, 50_100)])]);

    let r = evaluate(&cfg, &t);
    let ids: Vec<&str> = r.gift_list.iter().map(|g| g.tier_id.as_str()).collect();
    assert_eq!(ids, vec!["tier0", "tier300", "tier500"]);
    assert_eq!(r.packages[0].gift_count, 3);
    assert_eq!(r.gift_valuation_total, 90);
    assert!(r.gift_list.iter().all(|g| g.quantity == 1));
}

#[test]
fn reduce_average_excludes_gift_price_by_default() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier("t0", 0, 1000), tier("t300", 30_000, 3000)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 30_000)]),
        pkg("p2", vec![line("a", 1, 10_000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.gift_valuation_total, 5000);
    assert_eq!(r.reduce_average_total, 5000);
    assert_eq!(r.packages[0].reduce_average_cents, 3750);
    assert_eq!(r.packages[1].reduce_average_cents, 1250);
}

#[test]
fn reduce_average_includes_gift_price_when_configured() {
    let mut cfg = SettlementConfig::default();
    cfg.reduce_average.include_gift_price = true;
    cfg.gift_tiers = vec![tier("t0", 0, 1000), tier("t300", 30_000, 3000)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 30_000)]),
        pkg("p2", vec![line("a", 1, 10_000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.gift_valuation_total, 5000);
    assert_eq!(r.reduce_average_total, 5000);
    assert_eq!(r.packages[0].reduce_average_cents, 3778);
    assert_eq!(r.packages[1].reduce_average_cents, 1222);
}

#[test]
fn reduce_average_largest_remainder_conserves_total() {
    let mut cfg = SettlementConfig::default();
    cfg.gift_tiers = vec![tier("t1500", 1500, 100)];
    let t = table(vec![
        pkg("p1", vec![line("a", 1, 2000)]),
        pkg("p2", vec![line("a", 1, 1000)]),
    ]);

    let r = evaluate(&cfg, &t);
    assert_eq!(r.gift_valuation_total, 100);
    assert_eq!(r.reduce_average_total, 100);
    assert_eq!(r.packages[0].reduce_average_cents, 67);
    assert_eq!(r.packages[1].reduce_average_cents, 33);
    let sum: i64 = r.packages.iter().map(|p| p.reduce_average_cents).sum();
    assert_eq!(sum, r.gift_valuation_total);
}

#[test]
fn threshold_discount_applies_per_package() {
    let mut cfg = SettlementConfig::default();
    cfg.discounts.push(discount(
        DiscountKind::Threshold,
        100,
        Some(1000),
        None,
        -1,
    ));
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
    cfg.discounts.push(discount(
        DiscountKind::WholeOrder,
        50,
        None,
        None,
        -1,
    ));
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
    cfg.discounts.push(discount(
        DiscountKind::WholeOrder,
        200,
        None,
        None,
        -1,
    ));
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
