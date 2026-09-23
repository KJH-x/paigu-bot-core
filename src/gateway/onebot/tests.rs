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
