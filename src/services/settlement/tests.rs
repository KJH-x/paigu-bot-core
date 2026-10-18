use super::*;
use crate::settlement::{GiftTier, Line, Package};

#[test]
fn legacy_settlement_with_unknown_provenance_requires_review_and_bad_schema_fails_closed() {
    let mut cfg = crate::settings::default_config();
    cfg.round = round_with_adjust(0);
    let mut legacy = SettlementDraft::empty("r");
    legacy.revision = 1;
    legacy.order_table = OrderTable::new(vec![Package::new(
        "包甲",
        vec![Line::new("a", 1, 1000).subitem("v1")],
    )]);
    legacy
        .order_numbers
        .insert("包甲".into(), "ORDER-01".into());
    legacy.last_result = Some(evaluate(&cfg.settlement, &legacy.order_table));
    let mut raw = serde_json::to_value(&legacy).unwrap();
    raw.as_object_mut().unwrap().remove("reviewed_source");
    raw.as_object_mut().unwrap().remove("calculated_source");
    let decoded = decode_draft(&raw.to_string(), "r").unwrap();
    let view = review::response(&decoded, &cfg, &empty_allocation(), true).unwrap();
    assert_eq!(view["needs_review"], true);
    assert_eq!(view["result_stale"], true);
    assert_eq!(view["draft"]["order_numbers"], raw["order_numbers"]);
    raw["schema_version"] = json!(2);
    assert!(decode_draft(&raw.to_string(), "r").is_err());
    raw["schema_version"] = json!(1);
    assert!(decode_draft(&raw.to_string(), "another-round").is_err());
}

#[test]
fn parse_strategy_accepts_known_values() {
    assert!(matches!(
        parse_strategy("gift_max"),
        Ok(planner::Strategy::GiftMax)
    ));
    assert!(matches!(
        parse_strategy("discount_max"),
        Ok(planner::Strategy::DiscountMax)
    ));
    assert!(parse_strategy("nope").is_err());
}

#[test]
fn order_table_to_allocation_groups_lines_by_item() {
    let table = OrderTable::new(vec![
        Package::new("p1", vec![Line::new("a", 2, 100).subitem("v1")]),
        Package::new("p2", vec![Line::new("a", 1, 100).subitem("v1")]),
    ]);
    let allocation = order_table_to_allocation(&table);
    assert_eq!(allocation.item_allocations.len(), 1);
    let item = &allocation.item_allocations[0];
    assert_eq!(item.singles.len(), 2);
    let total: u32 = item.singles.iter().map(|s| s.quantity).sum();
    assert_eq!(total, 3);
}

#[test]
fn synthesized_allocation_feeds_planner() {
    let mut config = SettlementConfig::default();
    config.gift_tiers.push(GiftTier {
        tier_id: "t100".to_string(),
        threshold: 100,
        gift_name: "占位特典".to_string(),
        unit_price: 10,
        claimed: 10,
    });
    let table = OrderTable::new(vec![
        Package::new("p1", vec![Line::new("a", 1, 50)]),
        Package::new("p2", vec![Line::new("a", 1, 50)]),
    ]);
    let request = PlanRequest {
        config,
        allocation: order_table_to_allocation(&table),
        strategy: planner::Strategy::GiftMax,
        limits: PlanLimits::default(),
        prices: vec![],
    };
    let result = planner::plan(&request);
    let hits: u32 = result
        .best_result
        .packages
        .iter()
        .map(|p| p.gift_count)
        .sum();
    assert!(hits >= 1);
}

fn round_with_adjust(adjust_cents: i64) -> RoundSettings {
    use crate::settings::{ItemConfig, RoundSettings, SubitemConfig};
    RoundSettings {
        round_id: "r".to_string(),
        title: "t".to_string(),
        group_id: "g".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![ItemConfig {
            item_id: "a".to_string(),
            name: "A".to_string(),
            kind: "group".to_string(),
            class: None,
            aliases: vec![],
            unit_price_cents: 5000,
            max_quantity: None,
            box_item_id: None,
            subitems: vec![SubitemConfig {
                subitem_id: "v1".to_string(),
                name: "甲".to_string(),
                unit_price_cents: 5000,
                adjust_cents,
                capacity: None,
                aliases: vec![],
            }],
        }],
        category: crate::round::RoundCategory::Normal,
    }
}

#[test]
fn merge_catalog_adjust_sets_adjusted_price_only() {
    let merged = merge_catalog_pricing(SettlementConfig::default(), &round_with_adjust(-500));
    assert_eq!(merged.pricing.len(), 1);
    assert!(matches!(merged.pricing[0].mode, PricingMode::AdjustBy));
    assert_eq!(merged.pricing[0].value, -500);

    let table = OrderTable::new(vec![Package::new(
        "p1",
        vec![Line::new("a", 1, 5000).subitem("v1")],
    )]);
    let result = evaluate(&merged, &table);
    assert_eq!(result.lines[0].unit_price_cents, 5000, "标价 A 口径不变");
    assert_eq!(result.packages[0].gross_cents, 5000, "毛额以 A 计");
    assert_eq!(
        result.packages[0].adjusted_cents, 4500,
        "调后价 = C = A + B"
    );
}

#[test]
fn explicit_pricing_wins_over_catalog_adjust() {
    let mut base = SettlementConfig::default();
    base.pricing.push(PricingEntry {
        item_id: "a".to_string(),
        subitem_id: Some("v1".to_string()),
        mode: PricingMode::SetFinal,
        value: 6000,
    });
    let merged = merge_catalog_pricing(base, &round_with_adjust(-500));
    assert_eq!(merged.pricing.len(), 1, "已存在的显式定价保留、不追加");
    assert_eq!(merged.pricing[0].value, 6000);
}

#[test]
fn order_table_prices_reject_zero_and_conflicts() {
    let zero = OrderTable::new(vec![Package::new("p1", vec![Line::new("a", 1, 0)])]);
    assert!(matches!(
        prices_from_order_table(&zero),
        Err(ServiceError::Unprocessable(_))
    ));

    let conflict = OrderTable::new(vec![
        Package::new("p1", vec![Line::new("a", 1, 100)]),
        Package::new("p2", vec![Line::new("a", 1, 200)]),
    ]);
    assert!(matches!(
        prices_from_order_table(&conflict),
        Err(ServiceError::Unprocessable(_))
    ));
}

#[test]
fn plan_invariant_accepts_repack_and_rejects_price_loss() {
    let source = OrderTable::new(vec![
        Package::new("p1", vec![Line::new("a", 1, 100)]),
        Package::new("p2", vec![Line::new("a", 2, 100)]),
    ]);
    let repacked = OrderTable::new(vec![Package::new("p3", vec![Line::new("a", 3, 100)])]);
    assert!(ensure_plan_invariants(&source, &repacked).is_ok());

    let zeroed = OrderTable::new(vec![Package::new("p3", vec![Line::new("a", 3, 0)])]);
    assert!(matches!(
        ensure_plan_invariants(&source, &zeroed),
        Err(ServiceError::Unprocessable(_))
    ));
}
