use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::IncomingEvent;

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

pub fn normalize_fullwidth(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0xFF01..=0xFF5E).contains(&u) {
                char::from_u32(u - 0xFEE0).unwrap_or(c)
            } else if u == 0x3000 {
                ' '
            } else {
                c
            }
        })
        .collect()
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

fn split_proxy(s: &str) -> Option<(String, String)> {
    if !s.ends_with(')') {
        return None;
    }
    let open = s.rfind('(')?;
    let inner = &s[open + 1..s.len() - 1];
    let rest = inner.strip_prefix('代')?;
    let a = s[..open].trim();
    let b = rest.trim();
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a.to_string(), b.to_string()))
}

fn strip_remark(s: &str) -> String {
    match s.char_indices().find(|(_, c)| *c == '(' || *c == '（') {
        Some((i, _)) => s[..i].trim().to_string(),
        None => s.trim().to_string(),
    }
}

pub fn clean_nickname(raw: &str) -> (String, String) {
    let normalized = normalize_fullwidth(raw);
    let t = normalized.trim();
    if let Some((a, b)) = split_proxy(t) {
        return (a.clone(), format!("{a}({b})"));
    }
    let identity = strip_remark(t);
    (identity.clone(), identity)
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
    if !policy.whitelist_groups.iter().any(|g| g == &group_id) {
        return RouteDecision::Drop("not_whitelisted_group".to_string());
    }
    if !policy.whitelist_members.is_empty() {
        let id = parse_identity(ev);
        let candidates = [
            id.user_id.as_str(),
            id.identity.as_str(),
            id.display.as_str(),
        ];
        let allowed = policy.whitelist_members.iter().any(|m| {
            let m = m.trim();
            !m.is_empty() && candidates.contains(&m)
        });
        if !allowed {
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
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> RoutePolicy {
        RoutePolicy {
            whitelist_groups: vec!["123456789".to_string()],
            whitelist_members: Vec::new(),
        }
    }

    fn group_event(group_id: &str, text: &str) -> RouteMessageEvent {
        RouteMessageEvent {
            post_type: "message".to_string(),
            message_type: Some("group".to_string()),
            group_id: Some(json!(group_id)),
            user_id: Some(json!("10001")),
            message_id: Some(json!(42)),
            message: Some(json!([{ "type": "text", "data": { "text": text } }])),
            raw_message: Some(text.to_string()),
            time: Some(1_788_782_400),
            ..Default::default()
        }
    }

    #[test]
    fn decide_route_passes_whitelisted_group() {
        assert_eq!(
            decide_route(&group_event("123456789", "结城理 通行证"), &policy()),
            RouteKind::Message
        );
    }

    #[test]
    fn decide_route_drops_non_whitelisted_group() {
        assert_eq!(
            decide_route(&group_event("123456", "结城理 通行证"), &policy()),
            RouteKind::Drop
        );
    }

    #[test]
    fn decide_route_drops_non_message() {
        let mut ev = group_event("123456789", "结城理");
        ev.post_type = "notice".to_string();
        assert_eq!(decide_route(&ev, &policy()), RouteKind::Drop);

        let mut private = group_event("123456789", "结城理");
        private.message_type = Some("private".to_string());
        assert_eq!(decide_route(&private, &policy()), RouteKind::Drop);
    }

    #[test]
    fn decide_route_member_whitelist() {
        let mut p = policy();
        p.whitelist_members = vec!["10001".to_string()];
        assert_eq!(
            decide_route(&group_event("123456789", "结城理"), &p),
            RouteKind::Message
        );

        p.whitelist_members = vec!["99999".to_string()];
        assert_eq!(
            decide_route(&group_event("123456789", "结城理"), &p),
            RouteKind::Drop
        );
        assert_eq!(
            decide_route_with_reason(&group_event("123456789", "结城理"), &p),
            RouteDecision::Drop("not_whitelisted_member".to_string())
        );
    }

    #[test]
    fn decide_route_member_whitelist_matches_identity_and_display() {
        let mut p = policy();
        p.whitelist_members = vec!["甲".to_string()];
        let mut ev = group_event("123456789", "结城理");
        ev.sender = Some(Sender {
            user_id: Some(json!("10001")),
            nickname: Some("nick".to_string()),
            card: Some("甲（备注）".to_string()),
            role: None,
        });
        assert_eq!(decide_route(&ev, &p), RouteKind::Message);
    }

    #[test]
    fn decide_route_empty_member_whitelist_allows_all() {
        let mut ev = group_event("123456789", "结城理");
        ev.sender = Some(Sender {
            user_id: Some(json!("10001")),
            ..Default::default()
        });
        assert_eq!(decide_route(&ev, &policy()), RouteKind::Message);
    }

    #[test]
    fn to_message_record_marks_drop_reason() {
        let mut ev = group_event("123456789", "结城理 通行证");
        ev.sender = Some(Sender {
            user_id: Some(json!("10001")),
            nickname: Some("成员01".to_string()),
            role: Some("admin".to_string()),
            ..Default::default()
        });
        let rec = to_message_record(
            &ev,
            "drop:not_whitelisted_member",
            "Dropped",
            "not_whitelisted_member",
        );
        assert_eq!(rec.routed, "drop:not_whitelisted_member");
        assert_eq!(rec.status, "Dropped");
        assert_eq!(rec.detail, "not_whitelisted_member");
        assert_eq!(rec.group_id, "123456789");
        assert_eq!(rec.user_id, "10001");
        assert_eq!(rec.nickname, "成员01");
        assert_eq!(rec.message_id, "42");
        assert_eq!(rec.text, "结城理 通行证");
        assert_eq!(rec.timestamp_ms, 1_788_782_400_000);
        assert!(rec.is_admin);
    }

    #[test]
    fn decide_route_drops_empty_message() {
        let mut ev = group_event("123456789", "   ");
        ev.raw_message = Some("[CQ:image,file=a.jpg]".to_string());
        assert_eq!(decide_route(&ev, &policy()), RouteKind::Drop);

        let mut missing = group_event("123456789", "x");
        missing.message = None;
        missing.raw_message = None;
        assert_eq!(decide_route(&missing, &policy()), RouteKind::Drop);
    }

    #[test]
    fn normalize_message_from_segments() {
        let mut ev = group_event("123456789", "");
        ev.message = Some(json!([
            { "type": "text", "data": { "text": "结城理" } },
            { "type": "image", "data": { "file": "a.jpg" } },
            { "type": "text", "data": { "text": "通行证 x1" } }
        ]));
        assert_eq!(normalize_message(&ev), "结城理通行证 x1");
    }

    #[test]
    fn normalize_message_from_cq_codes() {
        let mut ev = group_event("123456789", "");
        ev.message = Some(json!(
            "[CQ:at,qq=123] 结城理 [CQ:image,file=a.jpg]通行证 [CQ:face,id=1]"
        ));
        assert_eq!(normalize_message(&ev), "结城理 通行证");
    }

    #[test]
    fn normalize_message_unescapes_entities() {
        let mut ev = group_event("123456789", "");
        ev.message = None;
        ev.raw_message = Some("[CQ:at,qq=1]A&#91;x&#93;&#44;B&amp;C".to_string());
        assert_eq!(normalize_message(&ev), "A[x],B&C");
    }

    #[test]
    fn clean_nickname_removes_remark() {
        assert_eq!(clean_nickname("甲（备注甲）").1, "甲");
        assert_eq!(clean_nickname("乙.（备注乙）").1, "乙.");
        assert_eq!(clean_nickname("丙/丁（备注丙）").1, "丙/丁");
        assert_eq!(clean_nickname("戊（备注戊👀）").1, "戊");
    }

    #[test]
    fn clean_nickname_proxy_notation() {
        let (identity, display) = clean_nickname("A（代B）");
        assert_eq!(identity, "A");
        assert_eq!(display, "A(B)");
        assert_eq!(clean_nickname("甲(代 乙)").0, "甲");
    }

    #[test]
    fn clean_nickname_normalizes_fullwidth() {
        assert_eq!(clean_nickname("名：015").1, "名:015");
    }

    #[test]
    fn parse_identity_uses_card_and_role() {
        let mut ev = group_event("123456789", "hi");
        ev.sender = Some(Sender {
            user_id: Some(json!("10001")),
            nickname: Some("nick".to_string()),
            card: Some("甲（备注甲）".to_string()),
            role: Some("admin".to_string()),
        });
        let id = parse_identity(&ev);
        assert_eq!(id.user_id, "10001");
        assert_eq!(id.identity, "甲");
        assert_eq!(id.display, "甲");
        assert!(id.is_admin);
    }

    #[test]
    fn to_incoming_event_maps_fields() {
        let ev = group_event("123456789", "结城理 通行证");
        let incoming = to_incoming_event(&ev);
        assert_eq!(incoming.group_id, "123456789");
        assert_eq!(incoming.user_id, "10001");
        assert_eq!(incoming.message_id, "42");
        assert_eq!(incoming.text, "结城理 通行证");
        assert_eq!(incoming.timestamp_ms, 1_788_782_400_000);
    }
}
