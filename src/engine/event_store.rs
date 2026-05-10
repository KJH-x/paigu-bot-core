use async_trait::async_trait;

use crate::domain::ids::RoundId;
use crate::domain::event::EventEnvelope;
use crate::error::AppResult;

#[async_trait]
pub trait EventStore: Send + Sync {
    async fn append(&self, event: &EventEnvelope) -> AppResult<EventEnvelope>;
    async fn read_all(&self, round_id: &RoundId) -> AppResult<Vec<EventEnvelope>>;
    async fn read_after_sequence(&self, round_id: &RoundId, after_sequence: i64) -> AppResult<Vec<EventEnvelope>>;
}

pub struct InMemoryEventStore {
    events: tokio::sync::RwLock<Vec<EventEnvelope>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            events: tokio::sync::RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn append(&self, event: &EventEnvelope) -> AppResult<EventEnvelope> {
        let mut events = self.events.write().await;
        events.push(event.clone());
        Ok(event.clone())
    }

    async fn read_all(&self, round_id: &RoundId) -> AppResult<Vec<EventEnvelope>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| &e.round_id == round_id)
            .cloned()
            .collect())
    }

    async fn read_after_sequence(&self, round_id: &RoundId, after_sequence: i64) -> AppResult<Vec<EventEnvelope>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| &e.round_id == round_id && e.sequence > after_sequence)
            .cloned()
            .collect())
    }
}
