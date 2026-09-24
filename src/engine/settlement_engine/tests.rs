use super::*;
use crate::domain::allocation::{
    BoxAllocation, ItemAllocation, SingleAllocation, SlotAllocation, SlotStatus,
};
use crate::domain::claim::SlotPolicy;
use crate::domain::ids::{ClaimId, RoundId};
use crate::domain::item::ItemKind;

fn item(item_id: &str, kind: ItemKind, price: i64) -> Item {
    Item {
        item_id: ItemId(item_id.to_string()),
        round_id: RoundId("r1".to_string()),
        name: item_id.to_string(),
        kind,
        unit_price: MoneyCents(price),
        box_size: None,
        max_quantity: None,
        is_blind: false,
        is_proxy_card: false,
        aliases: vec![],
        sort_order: 0,
        metadata: serde_json::Value::Null,
        variants: vec![],
    }
}

fn slot(user_id: &str) -> SlotAllocation {
    SlotAllocation {
        slot_index: 0,
        user_id: Some(UserId(user_id.to_string())),
        claim_id: Some(ClaimId("c1".to_string())),
        claim_line_index: Some(0),
        status: SlotStatus::Filled,
        slot_policy: SlotPolicy::Normal,
        segment_id: None,
        lock_reason: None,
    }
}

fn allocation() -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: RoundId("r1".to_string()),
        version: 3,
        generated_at: chrono::Utc::now(),
        item_allocations: vec![
            ItemAllocation {
                item_id: ItemId("a".to_string()),
                item_name: "a".to_string(),
                kind: "split".to_string(),
                variant_id: None,
                boxes: vec![BoxAllocation {
                    box_index: 0,
                    slots: vec![slot("u1"), slot("u1")],
                }],
                singles: vec![],
                waiting: vec![],
            },
            ItemAllocation {
                item_id: ItemId("b".to_string()),
                item_name: "b".to_string(),
                kind: "single".to_string(),
                variant_id: None,
                boxes: vec![],
                singles: vec![SingleAllocation {
                    user_id: UserId("u1".to_string()),
                    claim_id: ClaimId("c1".to_string()),
                    item_id: ItemId("b".to_string()),
                    quantity: 1,
                    unit_price: MoneyCents(500),
                }],
                waiting: vec![],
            },
        ],
        user_summaries: vec![],
        warnings: vec![],
    }
}

fn engine() -> SettlementEngine {
    SettlementEngine::new()
}

#[test]
fn settle_delegates_to_evaluate_and_maps_bills() {
    let input = SettlementInput {
        allocation: allocation(),
        items: vec![
            item("a", ItemKind::Split, 1000),
            item("b", ItemKind::Single, 500),
        ],
        discount_rules: vec![],
    };

    let snap = engine().settle(&input);
    assert_eq!(snap.user_bills.len(), 1);
    let bill = &snap.user_bills[0];
    assert_eq!(bill.user_id.0, "u1");
    assert_eq!(bill.gross_total.0, 2500);
    assert_eq!(bill.discount_share.0, 0);
    assert_eq!(bill.final_total.0, 2500);
    assert_eq!(snap.gross_total.0, 2500);
    assert_eq!(snap.final_total.0, 2500);

    let a = snap
        .item_totals
        .iter()
        .find(|t| t.item_id.0 == "a")
        .unwrap();
    assert_eq!(a.total_quantity, 2);
    assert_eq!(a.gross_total.0, 2000);
    let b = snap
        .item_totals
        .iter()
        .find(|t| t.item_id.0 == "b")
        .unwrap();
    assert_eq!(b.total_quantity, 1);
    assert_eq!(b.gross_total.0, 500);
}

#[test]
fn settle_resolves_and_reports_forced_tail_boxes() {
    use crate::domain::claim::{ClaimType, EffectiveClaimLine, SlotPolicy};
    use crate::domain::ids::ClaimId;
    use crate::domain::item::ItemVariant;
    use crate::engine::allocation_engine::AllocationEngine;

    let mut it = item("box", ItemKind::Split, 1000);
    it.variants = vec![
        ItemVariant {
            variant_id: "A".to_string(),
            name: "A".to_string(),
            unit_price: MoneyCents(1000),
            capacity: None,
            aliases: vec![],
        },
        ItemVariant {
            variant_id: "B".to_string(),
            name: "B".to_string(),
            unit_price: MoneyCents(1000),
            capacity: None,
            aliases: vec![],
        },
    ];
    let mk = |user: &str, variant: &str, policy: SlotPolicy, seq: i64| EffectiveClaimLine {
        claim_id: ClaimId(format!("c-{user}-{seq}")),
        line_index: 0,
        user_id: UserId(user.to_string()),
        item_id: ItemId("box".to_string()),
        variant_id: Some(variant.to_string()),
        quantity: 1,
        claim_type: ClaimType::Split,
        slot_policy: policy,
        effective_at: chrono::DateTime::from_timestamp_millis(seq).unwrap(),
        sequence: seq,
        priority_level: 0,
    };
    let lines = vec![
        mk("a1", "A", SlotPolicy::Normal, 1),
        mk("b1", "B", SlotPolicy::Normal, 2),
        mk("tail", "B", SlotPolicy::TailLocked, 3),
    ];

    let allocation = AllocationEngine::new()
        .allocate(&[it.clone()], &lines, &[])
        .unwrap();
    let snap = engine().settle(&SettlementInput {
        allocation,
        items: vec![it],
        discount_rules: vec![],
    });
    assert!(
        snap.warnings
            .iter()
            .any(|w| w.message.contains("包尾强制成盒")),
        "warnings={:?}",
        snap.warnings
    );
}

#[test]
fn settle_warns_when_legacy_discount_rules_present() {
    let input = SettlementInput {
        allocation: allocation(),
        items: vec![
            item("a", ItemKind::Split, 1000),
            item("b", ItemKind::Single, 500),
        ],
        discount_rules: vec![DiscountRule::ShoppingFund {
            rule_id: "fund".to_string(),
            amount: MoneyCents(100),
            allocation_policy:
                crate::domain::discount::DiscountAllocationPolicy::ByGrossAmountRatio,
        }],
    };

    let snap = engine().settle(&input);
    assert!(snap
        .warnings
        .iter()
        .any(|w| w.message.contains("DiscountRule")));
}
