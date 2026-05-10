use std::sync::Arc;

use crate::domain::ids::{RoundId, UserId};
use crate::domain::claim::Eligibility;
use crate::domain::item::Item;
use crate::domain::event::{EventEnvelope, DomainEvent, DiscountRulesSet};
use crate::domain::discount::DiscountRule;
use crate::repo::round_repo::{EligibilityRepo, RoundRepo, ItemRepo};
use crate::engine::event_store::EventStore;
use crate::error::AppResult;

pub struct AdminService {
    pub eligibility_repo: Arc<dyn EligibilityRepo>,
    pub round_repo: Arc<dyn RoundRepo>,
    pub item_repo: Arc<dyn ItemRepo>,
    pub event_store: Arc<dyn EventStore>,
}

impl AdminService {
    pub fn new(
        eligibility_repo: Arc<dyn EligibilityRepo>,
        round_repo: Arc<dyn RoundRepo>,
        item_repo: Arc<dyn ItemRepo>,
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        Self { eligibility_repo, round_repo, item_repo, event_store }
    }

    pub async fn add_eligibility(
        &self,
        round_id: RoundId,
        user_id: UserId,
        priority_type: String,
        priority_level: i32,
        scope: crate::domain::claim::EligibilityScope,
    ) -> AppResult<Eligibility> {
        let eligibility = Eligibility {
            eligibility_id: crate::domain::ids::EligibilityId(uuid::Uuid::new_v4().to_string()),
            round_id,
            user_id,
            priority_type,
            priority_level,
            scope,
            max_uses: None,
            used_count: 0,
            valid_from: None,
            valid_until: None,
            note: None,
        };
        self.eligibility_repo.insert(&eligibility).await
    }

    pub async fn add_item(&self, round_id: RoundId, name: String, kind: String, unit_price_cents: i64, box_size: Option<u32>, max_quantity: Option<u32>, aliases: Vec<String>) -> AppResult<Item> {
        let item = Item {
            item_id: crate::domain::ids::ItemId(uuid::Uuid::new_v4().to_string()),
            round_id: round_id.clone(),
            name,
            kind: crate::domain::item::ItemKind::from_str(&kind).unwrap_or(crate::domain::item::ItemKind::Split),
            unit_price: crate::domain::money::MoneyCents(unit_price_cents),
            box_size,
            max_quantity,
            is_blind: false,
            is_proxy_card: false,
            aliases,
            sort_order: 0,
            metadata: serde_json::Value::Null,
        };
        self.item_repo.insert(&item).await
    }

    pub async fn set_discount_rules(&self, round_id: RoundId, rules: Vec<DiscountRule>, source_text: String, user_id: crate::domain::ids::UserId) -> AppResult<EventEnvelope> {
        let event = EventEnvelope {
            event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
            round_id,
            group_id: String::new(),
            user_id,
            raw_message_id: None,
            event_type: "discount_rules_set".to_string(),
            effective_at: chrono::Utc::now(),
            sequence: 0,
            payload: DomainEvent::DiscountRulesSet(DiscountRulesSet { rules, source_text }),
            status: crate::domain::event::EventStatus::Active,
        };
        self.event_store.append(&event).await
    }

    pub async fn close_round(&self, round_id: &RoundId, _user_id: crate::domain::ids::UserId) -> AppResult<()> {
        self.round_repo.update_status(round_id, crate::domain::round::RoundStatus::Closed).await
    }

    pub async fn export_round(&self, _round_id: &RoundId) -> AppResult<String> {
        Ok("导出功能开发中。".to_string())
    }
}
