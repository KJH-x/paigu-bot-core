use crate::domain::ids::{RoundId, ItemId, UserId};
use crate::domain::round::Round;
use crate::domain::item::Item;
use crate::domain::item::ItemKind;
use crate::domain::money::MoneyCents;
use crate::domain::claim::{Eligibility, EligibilityScope};

pub fn fixture_round() -> Round {
    Round {
        round_id: RoundId("test_round_1".to_string()),
        group_id: "test_group".to_string(),
        title: "测试团".to_string(),
        status: crate::domain::round::RoundStatus::Active,
        start_at: None,
        end_at: None,
        allow_cancel: true,
        allow_modify: true,
        default_timezone: "Asia/Shanghai".to_string(),
        created_by: "admin".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

pub fn fixture_split_item(item_id: &str, name: &str, price_cents: i64, box_size: u32) -> Item {
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
    }
}

pub fn fixture_single_item(item_id: &str, name: &str, price_cents: i64, max_quantity: u32) -> Item {
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
    }
}

pub fn fixture_eligibility(user_id: &str, priority_level: i32, item_ids: Vec<&str>) -> Eligibility {
    Eligibility {
        eligibility_id: crate::domain::ids::EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: RoundId("test_round_1".to_string()),
        user_id: UserId(user_id.to_string()),
        priority_type: "test_priority".to_string(),
        priority_level,
        scope: EligibilityScope {
            item_ids: Some(item_ids.into_iter().map(|s| ItemId(s.to_string())).collect()),
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: None,
        valid_until: None,
        note: None,
    }
}
