use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct IngressSource {
    pub id: String,
    pub part: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IngressBinding {
    pub round_id: String,
    pub group_id: String,
    pub category: crate::round::RoundCategory,
}

impl IngressBinding {
    pub fn from_config(cfg: &crate::settings::AppConfig) -> Self {
        Self {
            round_id: cfg.round.round_id.clone(),
            group_id: cfg.round.group_id.clone(),
            category: cfg.round.category,
        }
    }
    pub fn matches(&self, cfg: &crate::settings::AppConfig) -> bool {
        self == &Self::from_config(cfg)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Committed,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DelegationKind {
    Member,
    Guest { label: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Delegation {
    pub actor_user_id: String,
    pub actor_nickname: String,
    pub source_message_id: String,
    pub source_text: String,
    pub target: DelegationKind,
}

impl Delegation {
    pub fn guest_id(&self, group_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let input = serde_json::json!([group_id, self.actor_user_id, self.target]);
        format!("proxy:{:x}", Sha256::digest(input.to_string().as_bytes()))
    }
}

/// 归一化后的入站群消息（Gateway 产出，Pipeline 消费）。
#[derive(Debug, Clone)]
pub struct IncomingEvent {
    pub delegation: Option<Delegation>,
    pub ingress: Option<IngressSource>,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub text: String,
    pub timestamp_ms: i64,
    pub is_admin: bool,
    /// 匿名代排（`X(代)`）的**真实发送者**身份（权限/白名单按发送者判定）；普通消息为 `None`。
    pub sender_user_id: Option<String>,
    pub sender_nickname: Option<String>,
    /// 原始 OneBot 事件 JSON（C-3：保留所有成员原始事件，供事件溯源）。
    pub raw: Option<serde_json::Value>,
}

/// 事件消费端。Gateway 只依赖本 trait，不依赖具体 Pipeline。
#[async_trait]
pub trait EventSink: Send + Sync {
    async fn handle(&self, ev: IncomingEvent);

    async fn deliver(
        &self,
        _binding: &IngressBinding,
        ev: IncomingEvent,
    ) -> anyhow::Result<Delivery> {
        self.handle(ev).await;
        Ok(Delivery::Committed)
    }

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
