use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::IncomingEvent;
use crate::parser::normalize::clean_nickname;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sender {
    #[serde(default)]
    pub user_id: Option<Value>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub card: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteMessageEvent {
    #[serde(default)]
    pub post_type: String,
    #[serde(default)]
    pub message_type: Option<String>,
    #[serde(default)]
    pub self_id: Option<Value>,
    #[serde(default)]
    pub user_id: Option<Value>,
    #[serde(default)]
    pub group_id: Option<Value>,
    #[serde(default)]
    pub message_id: Option<Value>,
    #[serde(default)]
    pub raw_message: Option<String>,
    #[serde(default)]
    pub message: Option<Value>,
    #[serde(default)]
    pub sender: Option<Sender>,
    #[serde(default)]
    pub time: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutePolicy {
    #[serde(default)]
    pub whitelist_groups: Vec<String>,
    #[serde(default)]
    pub whitelist_members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Drop,
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    Message,
    Drop(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub user_id: String,
    pub identity: String,
    pub display: String,
    pub is_admin: bool,
}

pub fn value_to_string(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn group_id_string(ev: &RouteMessageEvent) -> Option<String> {
    value_to_string(ev.group_id.as_ref())
}

fn unescape_cq(s: &str) -> String {
    s.replace("&#91;", "[")
        .replace("&#93;", "]")
        .replace("&#44;", ",")
        .replace("&amp;", "&")
}

pub fn strip_cq_codes(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '['
            && i + 3 < chars.len()
            && chars[i + 1] == 'C'
            && chars[i + 2] == 'Q'
            && chars[i + 3] == ':'
        {
            if let Some(rel) = chars[i..].iter().position(|&c| c == ']') {
                i += rel + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    unescape_cq(&out)
}

pub fn normalize_message(ev: &RouteMessageEvent) -> String {
    let mut text = match &ev.message {
        Some(Value::Array(segments)) => {
            let mut buf = String::new();
            for seg in segments {
                if seg.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = seg
                        .get("data")
                        .and_then(|d| d.get("text"))
                        .and_then(Value::as_str)
                    {
                        buf.push_str(t);
                    }
                }
            }
            buf
        }
        Some(Value::String(s)) => strip_cq_codes(s),
        _ => String::new(),
    };

    if text.trim().is_empty() {
        if let Some(raw) = &ev.raw_message {
            let alt = strip_cq_codes(raw);
            if !alt.trim().is_empty() {
                text = alt;
            }
        }
    }

    text.trim().to_string()
}

pub fn sanitize(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let total = flat.chars().count();
    let mut s: String = flat.chars().take(80).collect();
    if total > 80 {
        s.push('…');
    }
    s
}

pub fn parse_identity(ev: &RouteMessageEvent) -> Identity {
    let sender = ev.sender.clone().unwrap_or_default();
    let user_id = value_to_string(ev.user_id.as_ref())
        .or_else(|| value_to_string(sender.user_id.as_ref()))
        .unwrap_or_default();
    let raw_nick = sender
        .card
        .clone()
        .filter(|c| !c.trim().is_empty())
        .or_else(|| sender.nickname.clone())
        .unwrap_or_default();
    let (identity, display) = clean_nickname(&raw_nick);
    let is_admin = matches!(sender.role.as_deref(), Some("owner") | Some("admin"));
    Identity {
        user_id,
        identity,
        display,
        is_admin,
    }
}

pub fn decide_route_with_reason(ev: &RouteMessageEvent, policy: &RoutePolicy) -> RouteDecision {
    if ev.post_type != "message" {
        return RouteDecision::Drop("not_message".to_string());
    }
    if ev.message_type.as_deref() != Some("group") {
        return RouteDecision::Drop("not_group_message".to_string());
    }
    let Some(group_id) = group_id_string(ev) else {
        return RouteDecision::Drop("missing_group_id".to_string());
    };
    if !crate::parser::policy::group_allowed(&policy.whitelist_groups, &group_id) {
        return RouteDecision::Drop("not_whitelisted_group".to_string());
    }
    if !policy.whitelist_members.is_empty() {
        let id = parse_identity(ev);
        let candidates = [
            id.user_id.as_str(),
            id.identity.as_str(),
            id.display.as_str(),
        ];
        if !crate::parser::policy::member_allowed(&policy.whitelist_members, &candidates) {
            return RouteDecision::Drop("not_whitelisted_member".to_string());
        }
    }
    if normalize_message(ev).is_empty() {
        return RouteDecision::Drop("empty_message".to_string());
    }
    RouteDecision::Message
}

pub fn decide_route(ev: &RouteMessageEvent, policy: &RoutePolicy) -> RouteKind {
    match decide_route_with_reason(ev, policy) {
        RouteDecision::Message => RouteKind::Message,
        RouteDecision::Drop(_) => RouteKind::Drop,
    }
}

/// 把入站事件归一化为持久化日志记录（Drop 与已处理消息共用）。
pub fn to_message_record(
    ev: &RouteMessageEvent,
    routed: &str,
    status: &str,
    detail: &str,
) -> crate::messages::MessageRecord {
    let id = parse_identity(ev);
    crate::messages::MessageRecord {
        seq: crate::messages::next_seq(),
        group_id: group_id_string(ev).unwrap_or_default(),
        user_id: id.user_id,
        nickname: id.display,
        message_id: value_to_string(ev.message_id.as_ref()).unwrap_or_default(),
        text: normalize_message(ev),
        timestamp_ms: ev
            .time
            .map(|t| t.saturating_mul(1000))
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        is_admin: id.is_admin,
        routed: routed.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
    }
}

pub fn to_incoming_event(ev: &RouteMessageEvent) -> IncomingEvent {
    let id = parse_identity(ev);
    let group_id = group_id_string(ev).unwrap_or_default();
    let message_id = value_to_string(ev.message_id.as_ref()).unwrap_or_default();
    let text = normalize_message(ev);
    let timestamp_ms = ev
        .time
        .map(|t| t.saturating_mul(1000))
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    IncomingEvent {
        group_id,
        user_id: id.user_id,
        nickname: id.display,
        message_id,
        text,
        timestamp_ms,
        is_admin: id.is_admin,
        raw: serde_json::to_value(ev).ok(),
    }
}

#[cfg(test)]
mod tests;
