use serde::{Deserialize, Serialize};

use crate::inbound::qq_message::IncomingQqMessage;

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

    pub fn need_confirm(text: impl Into<String>, token: impl Into<String>) -> Self {
        BotReply::NeedConfirm {
            text: text.into(),
            confirm_token: token.into(),
        }
    }

    pub fn is_silent(&self) -> bool {
        matches!(self, BotReply::Silent)
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

pub enum CommandIntent {
    MemberClaim,
    MemberCancel,
    AdminCreateRound,
    AdminAddItem,
    AdminAddEligibility,
    AdminSetDiscount,
    AdminLockSlot,
    AdminFixUser,
    AdminCloseRound,
    AdminExport,
    Unknown,
}

pub fn classify_message(msg: &IncomingQqMessage) -> CommandIntent {
    let text = msg.text.trim();

    if msg.is_admin && text.starts_with('/') {
        if text.starts_with("/开团") {
            return CommandIntent::AdminCreateRound;
        }
        if text.starts_with("/加商品") || text.starts_with("/add_item") {
            return CommandIntent::AdminAddItem;
        }
        if text.starts_with("/加优先") || text.starts_with("/add_eligibility") {
            return CommandIntent::AdminAddEligibility;
        }
        if text.starts_with("/设置优惠") || text.starts_with("/set_discount") {
            return CommandIntent::AdminSetDiscount;
        }
        if text.starts_with("/锁位") || text.starts_with("/lock") {
            return CommandIntent::AdminLockSlot;
        }
        if text.starts_with("/修正") || text.starts_with("/fix") {
            return CommandIntent::AdminFixUser;
        }
        if text.starts_with("/结团") || text.starts_with("/close") {
            return CommandIntent::AdminCloseRound;
        }
        if text.starts_with("/导出") || text.starts_with("/export") {
            return CommandIntent::AdminExport;
        }
        return CommandIntent::Unknown;
    }

    if text.starts_with('/') && !msg.is_admin {
        return CommandIntent::Unknown;
    }

    let lower = text.to_lowercase();
    if lower.contains("撤") || lower.contains("取消") || lower.contains("cancel") {
        return CommandIntent::MemberCancel;
    }
    if lower.contains("排") || lower.contains("单领") || lower.contains("claim")
        || lower.contains("包尾") || lower.contains("端盒") {
        return CommandIntent::MemberClaim;
    }

    CommandIntent::Unknown
}
