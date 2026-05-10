use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::item::RoundContext;
use crate::error::LlmError;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn parse_message(&self, req: LlmParseRequest) -> Result<LlmParseResponse, LlmError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParseRequest {
    pub system_prompt: String,
    pub user_payload: serde_json::Value,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParseResponse {
    pub raw_text: String,
    pub parsed: super::parsed_event::ParsedMessage,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseRequestContext {
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message: String,
    pub active_rounds: Vec<RoundContext>,
}
