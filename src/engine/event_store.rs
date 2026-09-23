use async_trait::async_trait;

use crate::domain::event::EventEnvelope;
use crate::domain::ids::RoundId;

// 保留：测试夹具（src/tests/replay_helpers.rs）与后续重放接线使用；本轮不删除。
#[allow(dead_code)]
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn append(&self, event: &EventEnvelope) -> anyhow::Result<EventEnvelope>;
    async fn read_all(&self, round_id: &RoundId) -> anyhow::Result<Vec<EventEnvelope>>;
}

#[allow(dead_code)] // 保留：内存事件存储夹具，暂无生产消费者（重放接线备用）。
pub struct InMemoryEventStore {
    #[allow(dead_code)] // 仅由保留的 `read_all` 读取（见上）。
    events: tokio::sync::RwLock<Vec<EventEnvelope>>,
}

impl InMemoryEventStore {
    #[allow(dead_code)] // 保留：与 `InMemoryEventStore` 配套的构造函数。
    pub fn new() -> Self {
        Self {
            events: tokio::sync::RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn append(&self, event: &EventEnvelope) -> anyhow::Result<EventEnvelope> {
        let mut events = self.events.write().await;
        events.push(event.clone());
        Ok(event.clone())
    }

    async fn read_all(&self, round_id: &RoundId) -> anyhow::Result<Vec<EventEnvelope>> {
        let events = self.events.read().await;
        Ok(events
            .iter()
            .filter(|e| &e.round_id == round_id)
            .cloned()
            .collect())
    }
}
