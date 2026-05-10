use crate::domain::ids::{UserId, RoundId, ItemId};
use crate::domain::event::{EventEnvelope, DomainEvent, ClaimCancelled};
use crate::engine::event_store::EventStore;
use crate::error::AppResult;
use std::sync::Arc;

pub struct CancelService {
    pub event_store: Arc<dyn EventStore>,
}

impl CancelService {
    pub fn new(event_store: Arc<dyn EventStore>) -> Self {
        Self { event_store }
    }

    pub async fn cancel_claim(
        &self,
        round_id: &RoundId,
        user_id: &UserId,
        target_claim_id: Option<crate::domain::ids::ClaimId>,
        target_item_id: Option<ItemId>,
        quantity: Option<u32>,
    ) -> AppResult<EventEnvelope> {
        if let Some(ref claim_id) = target_claim_id {
            let events = self.event_store.read_all(round_id).await?;
            let owns_claim = events.iter().any(|ev| {
                if let DomainEvent::ClaimCreated(ref c) = ev.payload {
                    &c.claim_id == claim_id && &c.user_id == user_id
                } else {
                    false
                }
            });
            if !owns_claim {
                return Err(crate::error::AppError::Unauthorized("不能撤销他人的排谷记录。".to_string()));
            }
        }

        let event = EventEnvelope {
            event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
            round_id: round_id.clone(),
            group_id: String::new(),
            user_id: user_id.clone(),
            raw_message_id: None,
            event_type: "claim_cancelled".to_string(),
            effective_at: chrono::Utc::now(),
            sequence: 0,
            payload: DomainEvent::ClaimCancelled(ClaimCancelled {
                target_claim_id,
                target_item_id,
                quantity,
                reason: None,
                parse_trace: None,
                validation_trace: vec![],
            }),
            status: crate::domain::event::EventStatus::Active,
        };
        self.event_store.append(&event).await
    }
}
