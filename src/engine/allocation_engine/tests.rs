use super::*;
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::item::{Item, ItemKind, ItemVariant};

fn split_item(item_id: &str, name: &str, box_size: u32) -> Item {
    Item {
        item_id: ItemId(item_id.to_string()),
        round_id: RoundId("r1".to_string()),
        name: name.to_string(),
        kind: ItemKind::Split,
        unit_price: MoneyCents(100),
        box_size: Some(box_size),
        max_quantity: None,
        is_blind: false,
        is_proxy_card: false,
        aliases: vec![],
        sort_order: 0,
        metadata: serde_json::json!({}),
        variants: vec![],
    }
}

fn line(
    user: &str,
    item: &str,
    variant: Option<&str>,
    quantity: u32,
    policy: SlotPolicy,
    sequence: i64,
) -> EffectiveClaimLine {
    EffectiveClaimLine {
        claim_id: ClaimId(format!("c-{user}-{sequence}")),
        line_index: 0,
        user_id: UserId(user.to_string()),
        item_id: ItemId(item.to_string()),
        variant_id: variant.map(|v| v.to_string()),
        quantity,
        claim_type: ClaimType::Split,
        slot_policy: policy,
        effective_at: chrono::DateTime::from_timestamp_millis(sequence)
            .unwrap_or_else(chrono::Utc::now),
        sequence,
        priority_level: 0,
    }
}

#[test]
fn variants_are_allocated_independently() {
    let mut item = split_item("badge", "徽章", 2);
    item.variants = vec![
        ItemVariant {
            variant_id: "v_a".to_string(),
            name: "变体甲".to_string(),
            unit_price: MoneyCents(100),
            capacity: Some(2),
            aliases: vec![],
        },
        ItemVariant {
            variant_id: "v_b".to_string(),
            name: "变体乙".to_string(),
            unit_price: MoneyCents(100),
            capacity: Some(2),
            aliases: vec![],
        },
    ];
    let lines = vec![
        line("u1", "badge", Some("v_a"), 2, SlotPolicy::Normal, 1),
        line("u2", "badge", Some("v_b"), 2, SlotPolicy::Normal, 2),
    ];

    let snapshot = AllocationEngine::new()
        .allocate(&[item], &lines, &[])
        .unwrap();
    let rows: Vec<_> = snapshot
        .item_allocations
        .iter()
        .filter(|i| i.item_id.0 == "badge")
        .collect();
    assert_eq!(rows.len(), 2);
    let a = rows
        .iter()
        .find(|r| r.variant_id.as_deref() == Some("v_a"))
        .unwrap();
    let b = rows
        .iter()
        .find(|r| r.variant_id.as_deref() == Some("v_b"))
        .unwrap();
    assert_eq!(a.boxes.len(), 1);
    assert_eq!(b.boxes.len(), 1);
    assert_eq!(a.boxes[0].slots[0].user_id_str(), Some("u1"));
    assert_eq!(a.boxes[0].slots[1].user_id_str(), Some("u1"));
    assert_eq!(b.boxes[0].slots[0].user_id_str(), Some("u2"));
    assert_eq!(b.boxes[0].slots[1].user_id_str(), Some("u2"));
}

#[test]
fn single_claims_go_to_singles() {
    let mut item = split_item("gift", "特典", 2);
    item.kind = ItemKind::Single;
    item.max_quantity = Some(5);
    let mut claim = line("u1", "gift", None, 2, SlotPolicy::Normal, 1);
    claim.claim_type = ClaimType::Single;

    let snapshot = AllocationEngine::new()
        .allocate(&[item], &[claim], &[])
        .unwrap();
    let gift = snapshot
        .item_allocations
        .iter()
        .find(|i| i.item_id.0 == "gift")
        .unwrap();
    assert_eq!(gift.singles.len(), 1);
    assert_eq!(gift.singles[0].quantity, 2);
    assert!(gift.boxes.is_empty());
}

#[test]
fn tail_claim_is_clamped_to_box_size() {
    let item = split_item("bonus", "特典卡", 3);
    let claim = line("u1", "bonus", None, 10, SlotPolicy::TailLocked, 1);

    let snapshot = AllocationEngine::new()
        .allocate(&[item], &[claim], &[])
        .unwrap();
    let bonus = snapshot
        .item_allocations
        .iter()
        .find(|i| i.item_id.0 == "bonus")
        .unwrap();
    assert_eq!(bonus.boxes.len(), 1);
    assert_eq!(bonus.boxes[0].slots.len(), 3);
    assert!(bonus.boxes[0]
        .slots
        .iter()
        .all(|s| s.user_id_str() == Some("u1")));
}

#[test]
fn snapshot_order_is_deterministic() {
    let items = vec![split_item("b", "商品乙", 2), split_item("a", "商品甲", 2)];
    let lines = vec![
        line("u2", "a", None, 1, SlotPolicy::Normal, 2),
        line("u1", "b", None, 1, SlotPolicy::Normal, 1),
    ];
    let engine = AllocationEngine::new();
    let first = engine.allocate(&items, &lines, &[]).unwrap();
    let second = engine.allocate(&items, &lines, &[]).unwrap();
    let reversed: Vec<_> = lines.iter().rev().cloned().collect();
    let third = engine.allocate(&items, &reversed, &[]).unwrap();

    let rows = |s: &AllocationSnapshot| serde_json::to_value(&s.item_allocations).unwrap();
    let users = |s: &AllocationSnapshot| serde_json::to_value(&s.user_summaries).unwrap();
    assert_eq!(rows(&first), rows(&second));
    assert_eq!(rows(&first), rows(&third));
    assert_eq!(users(&first), users(&second));
    assert_eq!(users(&first), users(&third));
}
