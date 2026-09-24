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

fn variant_item(item_id: &str, variants: &[&str]) -> Item {
    let mut item = split_item(item_id, "拼团盒", 1);
    item.variants = variants
        .iter()
        .map(|v| ItemVariant {
            variant_id: v.to_string(),
            name: v.to_string(),
            unit_price: MoneyCents(100),
            capacity: None,
            aliases: vec![],
        })
        .collect();
    item
}

fn filled_user(
    snap: &AllocationSnapshot,
    item: &str,
    variant: &str,
    col: u32,
) -> Option<String> {
    snap.item_allocations
        .iter()
        .find(|i| i.item_id.0 == item && i.variant_id.as_deref() == Some(variant))
        .and_then(|e| e.boxes.iter().find(|b| b.box_index == col))
        .and_then(|b| {
            b.slots
                .iter()
                .find_map(|s| s.user_id.as_ref().map(|u| u.0.clone()))
        })
}

fn slot_at(snap: &AllocationSnapshot, item: &str, variant: &str, col: u32) -> Option<SlotAllocation> {
    snap.item_allocations
        .iter()
        .find(|i| i.item_id.0 == item && i.variant_id.as_deref() == Some(variant))
        .and_then(|e| e.boxes.iter().find(|b| b.box_index == col))
        .and_then(|b| b.slots.first().cloned())
}

/// §U8 用户原例：A/B/C 各排 1/3/2 个；包尾请求「B + 包尾」⇒ 锁定盒列 4；
/// 结算时列 2 的 C 认购人滑入列 4 的 C 槽；A 无未成团槽故列 4 空置；B 冲突不滑入。
#[test]
fn tail_locks_column_after_max_variant_claim_and_slides_non_conflicting() {
    let item = variant_item("box", &["A", "B", "C"]);
    let lines = vec![
        line("a1", "box", Some("A"), 1, SlotPolicy::Normal, 1),
        line("b1", "box", Some("B"), 1, SlotPolicy::Normal, 2),
        line("b2", "box", Some("B"), 1, SlotPolicy::Normal, 3),
        line("b3", "box", Some("B"), 1, SlotPolicy::Normal, 4),
        line("c1", "box", Some("C"), 1, SlotPolicy::Normal, 5),
        line("c2", "box", Some("C"), 1, SlotPolicy::Normal, 6),
        line("tail", "box", Some("B"), 1, SlotPolicy::TailLocked, 7),
    ];

    let snap = AllocationEngine::new()
        .allocate(&[item], &lines, &[])
        .unwrap();

    // 包尾人占锁定列 4 的申报变体 B。
    assert_eq!(filled_user(&snap, "box", "B", 4).as_deref(), Some("tail"));
    assert_eq!(
        slot_at(&snap, "box", "B", 4).map(|s| s.slot_policy),
        Some(SlotPolicy::TailLocked)
    );
    // 列 2 的 C 认购人滑入列 4 的 C 槽（不冲突）。
    assert_eq!(filled_user(&snap, "box", "C", 4).as_deref(), Some("c2"));
    assert!(filled_user(&snap, "box", "C", 2).is_none());
    // A 没有未成团槽 → 列 4 空置（LockedEmpty），不分配给包尾人。
    assert!(filled_user(&snap, "box", "A", 4).is_none());
    assert_eq!(
        slot_at(&snap, "box", "A", 4).map(|s| s.status),
        Some(SlotStatus::LockedEmpty)
    );
    // B 属于申报变体集（冲突）→ 列 2/3 的 B 不滑入。
    assert_eq!(filled_user(&snap, "box", "B", 2).as_deref(), Some("b2"));
    assert_eq!(filled_user(&snap, "box", "B", 3).as_deref(), Some("b3"));
    // 列 1 已成盒（A/B/C），A 不被移动。
    assert_eq!(filled_user(&snap, "box", "A", 1).as_deref(), Some("a1"));

    // 幂等：同输入重复分配结果一致。
    let again = AllocationEngine::new()
        .allocate(&[variant_item("box", &["A", "B", "C"])], &lines, &[])
        .unwrap();
    assert_eq!(
        serde_json::to_value(&snap.item_allocations).unwrap(),
        serde_json::to_value(&again.item_allocations).unwrap()
    );
}

/// ＄U8 冲突用例：未成盒列中的认购变体属于包尾申报集 ⇒ 不滑入。
#[test]
fn tail_conflicting_variant_does_not_slide_in() {
    let item = variant_item("box2", &["A", "B"]);
    let lines = vec![
        line("a1", "box2", Some("A"), 1, SlotPolicy::Normal, 1),
        line("b1", "box2", Some("B"), 1, SlotPolicy::Normal, 2),
        line("b2", "box2", Some("B"), 1, SlotPolicy::Normal, 3),
        line("tail", "box2", Some("B"), 1, SlotPolicy::TailLocked, 4),
    ];

    let snap = AllocationEngine::new()
        .allocate(&[item], &lines, &[])
        .unwrap();

    // B 普通认购最大列 = 2 ⇒ 锁定列 3。
    assert_eq!(filled_user(&snap, "box2", "B", 3).as_deref(), Some("tail"));
    // 列 2 未成盒（缺 A），但 B ∈ 申报集（冲突）⇒ 不滑入。
    assert_eq!(filled_user(&snap, "box2", "B", 2).as_deref(), Some("b2"));
    assert!(filled_user(&snap, "box2", "A", 3).is_none());
}

/// ＄U8 整盒：独立种类，进入单领队列，不参与拼团成盒。
#[test]
fn whole_box_item_enters_single_queue() {
    let mut item = split_item("wb", "整盒通行证", 6);
    item.kind = ItemKind::WholeBox;
    let claim = line("u1", "wb", None, 1, SlotPolicy::FullBox, 1);

    let snap = AllocationEngine::new()
        .allocate(&[item], &[claim], &[])
        .unwrap();
    let wb = snap
        .item_allocations
        .iter()
        .find(|i| i.item_id.0 == "wb")
        .unwrap();
    assert!(wb.boxes.is_empty());
    assert_eq!(wb.singles.len(), 1);
    assert_eq!(wb.singles[0].quantity, 1);
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
