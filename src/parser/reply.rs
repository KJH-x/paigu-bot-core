use serde::{Deserialize, Serialize};

/// 校验层对外回复（原 `inbound::command_router::BotReply` 的迁移版本）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotReply {
    Silent,
    Text(String),
    NeedConfirm { text: String, confirm_token: String },
    AdminOnly(String),
}

impl BotReply {
    pub fn silent() -> Self {
        BotReply::Silent
    }

    pub fn text(s: impl Into<String>) -> Self {
        BotReply::Text(s.into())
    }

    pub fn text_content(&self) -> Option<&str> {
        match self {
            BotReply::Text(t) => Some(t.as_str()),
            BotReply::NeedConfirm { text, .. } => Some(text.as_str()),
            BotReply::AdminOnly(t) => Some(t.as_str()),
            BotReply::Silent => None,
        }
    }
}
