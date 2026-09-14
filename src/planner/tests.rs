use super::*;

use crate::domain::allocation::{
    BoxAllocation, ItemAllocation, SingleAllocation, SlotAllocation, SlotStatus,
};
use crate::domain::claim::SlotPolicy;
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;
use crate::settlement::{DiscountEntry, DiscountKind, GiftTier};

fn single_item(item_id: &str, user: &str, claim: &str, price: i64) -> ItemAllocation {
    ItemAllocation {
        item_id: ItemId(item_id.to_string()),
        item_name: item_id.to_string(),
        kind: "single".to_string(),
        variant_id: None,
        boxes: vec![],
        singles: vec![SingleAllocation {
            user_id: UserId(user.to_string()),
            claim_id: ClaimId(claim.to_string()),
            item_id: ItemId(item_id.to_string()),
            quantity: 1,
            unit_price: MoneyCents(price),
        }],
        waiting: vec![],
    }
}

fn snapshot(items: Vec<ItemAllocation>) -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: RoundId("r1".to_string()),
        version: 1,
        generated_at: chrono::Utc::now(),
        item_allocations: items,
        user_summaries: vec![],
        warnings: vec![],
    }
}

fn tier(id: &str, threshold: i64, price: i64) -> GiftTier {
    GiftTier {
        tier_id: id.to_string(),
        threshold,
        gift_name: format!("gift-{id}"),
        unit_price: price,
    }
}

fn threshold_discount(amount: i64, threshold: i64, shares: i64) -> DiscountEntry {
    DiscountEntry {
        rule_id: "d".to_string(),
        kind: DiscountKind::Threshold,
        amount,
        threshold: Some(threshold),
        ratio_ppm: None,
        shares,
    }
}

fn limits(max_packages: u32, max_iters: u64) -> PlanLimits {
    PlanLimits {
        max_packages,
        max_iters,
        time_budget_ms: 5_000,
    }
}

#[test]
fn gift_max_merges_to_cross_three_tiers() {
    let mut config = SettlementConfig::default();
    config.gift_tiers = vec![
        tier("t0", 0, 10),
        tier("t300", 30_000, 30),
        tier("t500", 50_000, 50),
    ];
    let req = PlanRequest {
        config,
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 25_000),
            single_item("b", "u2", "c2", 25_100),
        ]),
        strategy: Strategy::GiftMax,
        limits: limits(3, 100_000),
        prices: vec![],
    };

    let result = plan(&req);
    assert_eq!(result.best.packages.len(), 1);
    assert_eq!(result.best_result.gift_list.len(), 3);
    assert_eq!(result.best_result.gift_valuation_total, 90);
    assert_eq!(result.best_score.highest_tier_hits, 1);
    assert_eq!(result.best_score.gift_count, 3);
}

#[test]
fn discount_max_splits_into_threshold_packages() {
    let mut config = SettlementConfig::default();
    config.discounts = vec![threshold_discount(1_000, 10_000, -1)];
    let req = PlanRequest {
        config,
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 6_000),
            single_item("b", "u2", "c2", 6_000),
            single_item("c", "u3", "c3", 6_000),
            single_item("d", "u4", "c4", 6_000),
        ]),
        strategy: Strategy::DiscountMax,
        limits: limits(4, 200_000),
        prices: vec![],
    };

    let result = plan(&req);
    assert_eq!(result.best_score.discount_total, 2_000);
    assert_eq!(result.best.packages.len(), 2);
}

#[test]
fn best_table_packages_sorted_by_amount_desc() {
    let mut config = SettlementConfig::default();
    config.discounts = vec![threshold_discount(100, 1_000, -1)];
    let req = PlanRequest {
        config,
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 1_000),
            single_item("b", "u2", "c2", 3_000),
            single_item("c", "u3", "c3", 2_000),
        ]),
        strategy: Strategy::DiscountMax,
        limits: limits(3, 100_000),
        prices: vec![],
    };

    let result = plan(&req);
    assert_eq!(result.best_score.discount_total, 300);
    assert_eq!(result.best.packages.len(), 3);
    let amounts: Vec<i64> = result
        .best
        .packages
        .iter()
        .map(|p| p.lines.iter().map(|l| l.total_cents()).sum())
        .collect();
    assert_eq!(amounts, vec![3_000, 2_000, 1_000]);
}

#[test]
fn no_discount_config_prunes_after_first_leaf() {
    let req = PlanRequest {
        config: SettlementConfig::default(),
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 100),
            single_item("b", "u2", "c2", 200),
            single_item("c", "u3", "c3", 300),
            single_item("d", "u4", "c4", 400),
            single_item("e", "u5", "c5", 500),
        ]),
        strategy: Strategy::DiscountMax,
        limits: limits(5, 1_000_000),
        prices: vec![],
    };

    let result = plan(&req);
    assert_eq!(result.stats.evaluated, 1);
    assert!(result.stats.pruned > 0);
    assert!(!result.stats.truncated);
}

#[test]
fn max_iters_truncates_deterministically() {
    let req = PlanRequest {
        config: SettlementConfig::default(),
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 100),
            single_item("b", "u2", "c2", 200),
            single_item("c", "u3", "c3", 300),
            single_item("d", "u4", "c4", 400),
            single_item("e", "u5", "c5", 500),
            single_item("f", "u6", "c6", 600),
            single_item("g", "u7", "c7", 700),
        ]),
        strategy: Strategy::GiftMax,
        limits: limits(7, 3),
        prices: vec![],
    };

    let first = plan(&req);
    let second = plan(&req);

    assert!(first.stats.truncated);
    assert_eq!(first.stats.truncation_reason.as_deref(), Some("max_iters"));
    assert_eq!(first.stats.nodes, second.stats.nodes);
    assert_eq!(first.stats.evaluated, second.stats.evaluated);
    assert_eq!(
        serde_json::to_string(&first.best).unwrap(),
        serde_json::to_string(&second.best).unwrap()
    );
}

#[test]
fn both_strategies_are_reproducible() {
    for strategy in [Strategy::GiftMax, Strategy::DiscountMax] {
        let mut config = SettlementConfig::default();
        config.gift_tiers = vec![
            tier("t0", 0, 10),
            tier("t300", 30_000, 30),
            tier("t500", 50_000, 50),
        ];
        config.discounts = vec![threshold_discount(1_000, 10_000, -1)];
        let req = PlanRequest {
            config,
            allocation: snapshot(vec![
                single_item("a", "u1", "c1", 25_000),
                single_item("b", "u2", "c2", 25_100),
                single_item("c", "u3", "c3", 12_000),
            ]),
            strategy,
            limits: limits(3, 200_000),
            prices: vec![],
        };

        let first = plan(&req);
        let second = plan(&req);
        assert_eq!(
            serde_json::to_string(&first.best).unwrap(),
            serde_json::to_string(&second.best).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&first.best_result).unwrap(),
            serde_json::to_string(&second.best_result).unwrap()
        );
        assert_eq!(first.best_score, second.best_score);
    }
}

#[test]
fn manual_evaluate_matches_settlement() {
    let config = SettlementConfig::default();
    let table = OrderTable::new(vec![Package::new("p1", vec![Line::new("a", 2, 1_000)])]);
    assert_eq!(manual_evaluate(&config, &table), evaluate(&config, &table));
}

#[test]
fn box_slots_use_price_table() {
    let mut config = SettlementConfig::default();
    config.gift_tiers = vec![tier("t0", 0, 10), tier("t500", 50_000, 50)];
    let allocation = snapshot(vec![ItemAllocation {
        item_id: ItemId("box_item".to_string()),
        item_name: "盒货".to_string(),
        kind: "split".to_string(),
        variant_id: Some("v1".to_string()),
        boxes: vec![BoxAllocation {
            box_index: 0,
            slots: vec![SlotAllocation {
                slot_index: 0,
                user_id: Some(UserId("u1".to_string())),
                claim_id: Some(ClaimId("c1".to_string())),
                claim_line_index: Some(0),
                status: SlotStatus::Filled,
                slot_policy: SlotPolicy::Normal,
                segment_id: None,
                lock_reason: None,
            }],
        }],
        singles: vec![],
        waiting: vec![],
    }]);
    let req = PlanRequest {
        config,
        allocation,
        strategy: Strategy::GiftMax,
        limits: limits(2, 100_000),
        prices: vec![UnitPrice {
            item_id: "box_item".to_string(),
            variant_id: Some("v1".to_string()),
            unit_price_cents: 50_000,
        }],
    };

    let result = plan(&req);
    assert_eq!(result.best.packages.len(), 1);
    assert_eq!(result.best.packages[0].lines[0].unit_price_cents, 50_000);
    assert_eq!(result.best_result.gift_valuation_total, 60);
}

#[test]
fn max_packages_is_respected() {
    let mut config = SettlementConfig::default();
    config.gift_tiers = vec![tier("t0", 0, 10)];
    let req = PlanRequest {
        config,
        allocation: snapshot(vec![
            single_item("a", "u1", "c1", 1_000),
            single_item("b", "u2", "c2", 2_000),
            single_item("c", "u3", "c3", 3_000),
        ]),
        strategy: Strategy::DiscountMax,
        limits: limits(1, 100_000),
        prices: vec![],
    };

    let result = plan(&req);
    assert_eq!(result.best.packages.len(), 1);
    assert_eq!(result.stats.packages_used, 1);
}
