use async_trait::async_trait;
use std::sync::Arc;

/// 归一化后的入站群消息（Gateway 产出，Pipeline 消费）。
#[derive(Debug, Clone)]
pub struct IncomingEvent {
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    /// 原始 OneBot 事件 JSON（C-3：保留所有成员原始事件，供事件溯源）。
    pub raw: Option<serde_json::Value>,
}

/// 事件消费端。Gateway 只依赖本 trait，不依赖具体 Pipeline。
#[async_trait]
pub trait EventSink: Send + Sync {
    async fn handle(&self, ev: IncomingEvent);

    /// 共享消息日志句柄（T-05：Gateway 的 Drop 记录 / 原始事件落盘复用同一实例）。
    fn messages(&self) -> Arc<crate::messages::MessageLog>;
}

/// 流水线单条消息的处理结果（HTTP/展示共用）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineOutcome {
    pub status: String,
    pub detail: String,
    pub reply: Option<String>,
    pub version: i64,
    pub snapshot: Option<serde_json::Value>,
}
