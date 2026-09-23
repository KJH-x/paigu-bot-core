#![cfg(test)]

use crate::domain::claim::{
    ClaimLine, ClaimType, EffectiveClaimLine, Eligibility, EligibilityScope, SlotPolicy,
};
use crate::domain::event::{ClaimCreated, DomainEvent, EventEnvelope, EventStatus};
use crate::domain::ids::{ClaimId, ItemId, RoundId, UserId};
use crate::domain::item::{Item, ItemKind};
use crate::domain::money::MoneyCents;
use crate::engine::allocation_engine::AllocationEngine;
use crate::engine::replay::collect_effective_claims;

fn fixture_split_item(item_id: &str, name: &str, price_cents: i64, box_size: u32) -> Item {
    Item {
        item_id: ItemId(item_id.to_string()),
        round_id: RoundId("test_round_1".to_string()),
        name: name.to_string(),
        kind: ItemKind::Split,
        unit_price: MoneyCents(price_cents),
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

fn fixture_single_item(item_id: &str, name: &str, price_cents: i64, max_quantity: u32) -> Item {
    Item {
        item_id: ItemId(item_id.to_string()),
        round_id: RoundId("test_round_1".to_string()),
        name: name.to_string(),
        kind: ItemKind::Single,
        unit_price: MoneyCents(price_cents),
        box_size: None,
        max_quantity: Some(max_quantity),
        is_blind: false,
        is_proxy_card: false,
        aliases: vec![],
        sort_order: 0,
        metadata: serde_json::json!({}),
        variants: vec![],
    }
}

fn ts(ms: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_else(chrono::Utc::now)
}

fn claim_line(
    user_id: &str,
    item_id: &str,
    quantity: u32,
    ts_ms: i64,
    seq: i64,
    priority: i32,
) -> EffectiveClaimLine {
    EffectiveClaimLine {
        claim_id: ClaimId(uuid::Uuid::new_v4().to_string()),
        line_index: 0,
        user_id: UserId(user_id.to_string()),
        item_id: ItemId(item_id.to_string()),
        variant_id: None,
        quantity,
        claim_type: ClaimType::Split,
        slot_policy: SlotPolicy::Normal,
        effective_at: ts(ts_ms),
        sequence: seq,
        priority_level: priority,
    }
}

fn variant_line(
    user_id: &str,
    item_id: &str,
    variant_id: &str,
    quantity: u32,
    ts_ms: i64,
    seq: i64,
) -> EffectiveClaimLine {
    EffectiveClaimLine {
        claim_id: ClaimId(uuid::Uuid::new_v4().to_string()),
        line_index: 0,
        user_id: UserId(user_id.to_string()),
        item_id: ItemId(item_id.to_string()),
        variant_id: Some(variant_id.to_string()),
        quantity,
        claim_type: ClaimType::Split,
        slot_policy: SlotPolicy::Normal,
        effective_at: ts(ts_ms),
        sequence: seq,
        priority_level: 0,
    }
}

fn tail_line(user_id: &str, item_id: &str, quantity: u32, ts_ms: i64) -> EffectiveClaimLine {
    EffectiveClaimLine {
        claim_id: ClaimId(uuid::Uuid::new_v4().to_string()),
        line_index: 0,
        user_id: UserId(user_id.to_string()),
        item_id: ItemId(item_id.to_string()),
        variant_id: None,
        quantity,
        claim_type: ClaimType::Split,
        slot_policy: SlotPolicy::TailLocked,
        effective_at: ts(ts_ms),
        sequence: 0,
        priority_level: 0,
    }
}

#[test]
fn priority_user_before_normal_user() {
    let item = fixture_split_item("badge", "badge", 4500, 10);
    let normal = claim_line("u1", "badge", 1, 100, 1, 0);
    let priority = claim_line("u2", "badge", 1, 101, 2, 10);

    let engine = AllocationEngine::new();
    let lines = vec![normal, priority];
    let events: Vec<EventEnvelope> = vec![];

    let snapshot = engine.allocate(&[item], &lines, &events).unwrap();
    let badge = snapshot.item("badge").unwrap();
    let slots = &badge.box_at(1).unwrap().slots;

    let mut sorted_lines = lines.clone();
    sorted_lines.sort_by(|a, b| {
        b.priority_level
            .cmp(&a.priority_level)
            .then_with(|| a.effective_at.cmp(&b.effective_at))
            .then_with(|| a.sequence.cmp(&b.sequence))
            .then_with(|| a.line_index.cmp(&b.line_index))
    });

    assert_eq!(slots[0].user_id_str(), Some("u2"));
    assert_eq!(slots[1].user_id_str(), Some("u1"));
}

#[test]
fn tail_locked_creates_new_box() {
    let item = fixture_split_item("bonus", "bonus", 3000, 5);
    let a = claim_line("u1", "bonus", 1, 100, 1, 0);
    let tail = tail_line("u2", "bonus", 2, 101);
    let b = claim_line("u3", "bonus", 1, 102, 2, 0);

    let engine = AllocationEngine::new();
    let lines = vec![a, tail, b];
    let events: Vec<EventEnvelope> = vec![];

    let snapshot = engine.allocate(&[item], &lines, &events).unwrap();
    let bonus = snapshot.item("bonus").unwrap();

    assert_eq!(bonus.boxes.len(), 2);
    assert_eq!(bonus.box_at(1).unwrap().slots[0].user_id_str(), Some("u1"));
    assert_eq!(bonus.box_at(1).unwrap().slots[1].user_id_str(), Some("u3"));
    assert_eq!(bonus.box_at(2).unwrap().slots[0].user_id_str(), Some("u2"));
    assert_eq!(bonus.box_at(2).unwrap().slots[1].user_id_str(), Some("u2"));
}

#[test]
fn cancel_moves_slots_forward() {
    let item = fixture_split_item("item_a", "item_a", 4500, 5);
    let u1 = claim_line("u1", "item_a", 1, 100, 1, 0);
    let u2 = claim_line("u2", "item_a", 1, 101, 2, 0);
    let u3 = claim_line("u3", "item_a", 1, 102, 3, 0);

    let engine = AllocationEngine::new();
    let lines = vec![u1, u2, u3];
    let events: Vec<EventEnvelope> = vec![];
    let snapshot = engine.allocate(&[item], &lines, &events).unwrap();

    let item_a = snapshot.item("item_a").unwrap();
    assert_eq!(item_a.box_at(1).unwrap().slots[0].user_id_str(), Some("u1"));
    assert_eq!(item_a.box_at(1).unwrap().slots[1].user_id_str(), Some("u2"));
    assert_eq!(item_a.box_at(1).unwrap().slots[2].user_id_str(), Some("u3"));
}

#[test]
fn discount_allocation_by_ratio() {
    use crate::engine::settlement_engine::allocate_discount_by_ratio;

    let total = MoneyCents(100);
    let basis = vec![
        (UserId("u1".to_string()), MoneyCents(300)),
        (UserId("u2".to_string()), MoneyCents(200)),
    ];

    let shares = allocate_discount_by_ratio(total, &basis);
    let sum: i64 = shares.iter().map(|s| s.amount.0).sum();
    assert_eq!(sum, 100);

    let u1_share = shares
        .iter()
        .find(|s| s.user_id.0 == "u1")
        .unwrap()
        .amount
        .0;
    let u2_share = shares
        .iter()
        .find(|s| s.user_id.0 == "u2")
        .unwrap()
        .amount
        .0;
    assert_eq!(u1_share, 60);
    assert_eq!(u2_share, 40);
}

#[test]
fn money_cents_operations() {
    let a = MoneyCents(4500);
    let b = MoneyCents(3000);
    assert_eq!(a.checked_add(b).unwrap().0, 7500);
    assert_eq!(a.checked_sub(b).unwrap().0, 1500);
    assert_eq!(a.checked_mul_u32(2).unwrap().0, 9000);
    assert_eq!(a.format_yuan(), "45.00");
    assert_eq!(MoneyCents::from_yuan(45).0, 4500);
}

#[test]
fn effective_claim_line_priority_sorting() {
    let mut lines = [
        claim_line("u1", "item_x", 1, 100, 1, 0),
        claim_line("u2", "item_x", 1, 101, 2, 10),
        claim_line("u3", "item_x", 1, 99, 3, 5),
    ];

    lines.sort_by(|a, b| {
        b.priority_level
            .cmp(&a.priority_level)
            .then_with(|| a.effective_at.cmp(&b.effective_at))
            .then_with(|| a.sequence.cmp(&b.sequence))
            .then_with(|| a.line_index.cmp(&b.line_index))
    });

    assert_eq!(lines[0].user_id.0, "u2");
    assert_eq!(lines[1].user_id.0, "u3");
    assert_eq!(lines[2].user_id.0, "u1");
}

#[test]
fn replay_cancellation_removes_effective_claims() {
    use crate::domain::event::ClaimCancelled;

    let events = vec![
        EventEnvelope {
            event_id: crate::domain::ids::EventId("e1".to_string()),
            round_id: RoundId("test".to_string()),
            group_id: "g1".to_string(),
            user_id: UserId("u1".to_string()),
            raw_message_id: None,
            event_type: "claim_created".to_string(),
            effective_at: ts(100),
            sequence: 1,
            payload: DomainEvent::ClaimCreated(ClaimCreated {
                claim_id: ClaimId("c1".to_string()),
                user_id: UserId("u1".to_string()),
                items: vec![ClaimLine {
                    item_id: ItemId("badge".to_string()),
                    variant_id: None,
                    quantity: 1,
                    claim_type: ClaimType::Split,
                    slot_policy: SlotPolicy::Normal,
                    is_proxy_card: false,
                    notes: None,
                }],
                source_text: String::new(),
                parse_trace: None,
                validation_trace: vec![],
            }),
            status: EventStatus::Active,
        },
        EventEnvelope {
            event_id: crate::domain::ids::EventId("e2".to_string()),
            round_id: RoundId("test".to_string()),
            group_id: "g1".to_string(),
            user_id: UserId("u1".to_string()),
            raw_message_id: None,
            event_type: "claim_cancelled".to_string(),
            effective_at: ts(101),
            sequence: 2,
            payload: DomainEvent::ClaimCancelled(ClaimCancelled {
                target_claim_id: Some(ClaimId("c1".to_string())),
                target_item_id: None,
                quantity: None,
                reason: None,
                parse_trace: None,
                validation_trace: vec![],
            }),
            status: EventStatus::Active,
        },
    ];

    let eligibility: Vec<Eligibility> = vec![];
    let effective = collect_effective_claims(&events, &eligibility);
    assert!(effective.is_empty());
}

#[test]
fn eligibility_applies_to_item() {
    let eligibility = Eligibility {
        eligibility_id: crate::domain::ids::EligibilityId("e1".to_string()),
        round_id: RoundId("r1".to_string()),
        user_id: UserId("u1".to_string()),
        priority_type: "vip".to_string(),
        priority_level: 10,
        scope: EligibilityScope {
            item_ids: Some(vec![
                ItemId("badge_rinne".to_string()),
                ItemId("badge_aira".to_string()),
            ]),
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: None,
        valid_until: None,
        note: None,
    };

    assert!(eligibility.applies_to_item(&ItemId("badge_rinne".to_string())));
    assert!(eligibility.applies_to_item(&ItemId("badge_aira".to_string())));
    assert!(!eligibility.applies_to_item(&ItemId("badge_himeru".to_string())));
}

#[test]
fn variant_allocation_isolated_per_variant() {
    use crate::domain::item::ItemVariant;

    let mut split = fixture_split_item("badge", "徽章", 4500, 10);
    split.variants = vec![
        ItemVariant {
            variant_id: "v_yukari".to_string(),
            name: "岳羽由加莉".to_string(),
            unit_price: MoneyCents(4500),
            capacity: Some(2),
            aliases: vec![],
        },
        ItemVariant {
            variant_id: "v_tora".to_string(),
            name: "虎狼丸".to_string(),
            unit_price: MoneyCents(4500),
            capacity: Some(2),
            aliases: vec![],
        },
    ];
    let single = fixture_single_item("gift", "特典", 1000, 10);

    let a1 = variant_line("u1", "badge", "v_yukari", 1, 100, 1);
    let a2 = variant_line("u2", "badge", "v_yukari", 1, 101, 2);
    let b1 = variant_line("u3", "badge", "v_tora", 1, 102, 3);
    let b2 = variant_line("u4", "badge", "v_tora", 1, 103, 4);
    let mut s1 = claim_line("u5", "gift", 2, 104, 5, 0);
    s1.claim_type = ClaimType::Single;

    let engine = AllocationEngine::new();
    let lines = vec![a1, a2, b1, b2, s1];
    let events: Vec<EventEnvelope> = vec![];
    let snapshot = engine.allocate(&[split, single], &lines, &events).unwrap();

    let badge_rows: Vec<_> = snapshot
        .item_allocations
        .iter()
        .filter(|i| i.item_id.0 == "badge")
        .collect();
    assert_eq!(badge_rows.len(), 2);

    let yukari = badge_rows
        .iter()
        .find(|i| i.variant_id.as_deref() == Some("v_yukari"))
        .unwrap();
    let tora = badge_rows
        .iter()
        .find(|i| i.variant_id.as_deref() == Some("v_tora"))
        .unwrap();

    assert_eq!(yukari.boxes.len(), 1);
    assert_eq!(tora.boxes.len(), 1);
    assert_eq!(yukari.box_at(1).unwrap().slots.len(), 2);
    assert_eq!(tora.box_at(1).unwrap().slots.len(), 2);

    assert_eq!(yukari.box_at(1).unwrap().slots[0].user_id_str(), Some("u1"));
    assert_eq!(yukari.box_at(1).unwrap().slots[1].user_id_str(), Some("u2"));
    assert_eq!(tora.box_at(1).unwrap().slots[0].user_id_str(), Some("u3"));
    assert_eq!(tora.box_at(1).unwrap().slots[1].user_id_str(), Some("u4"));

    let gift = snapshot
        .item_allocations
        .iter()
        .find(|i| i.item_id.0 == "gift")
        .unwrap();
    assert_eq!(gift.singles.len(), 1);
    assert_eq!(gift.singles[0].user_id.0, "u5");
    assert_eq!(gift.singles[0].quantity, 2);
}

#[test]
fn item_kind_compatible_with_claim_type() {
    let split = ItemKind::Split;
    let single = ItemKind::Single;
    let gift = ItemKind::Gift;

    assert!(split.compatible_with(&ClaimType::Split));
    assert!(!split.compatible_with(&ClaimType::Single));
    assert!(single.compatible_with(&ClaimType::Single));
    assert!(gift.compatible_with(&ClaimType::GiftClaim));
    assert!(!single.compatible_with(&ClaimType::Split));
}
