use super::*;
use async_trait::async_trait;
use serde_json::json;

use crate::bus::IncomingEvent;

struct NullSink;

#[async_trait]
impl EventSink for NullSink {
    async fn handle(&self, _ev: IncomingEvent) {}

    fn messages(&self) -> Arc<MessageLog> {
        let dir = std::env::temp_dir().join("paigu-null-sink");
        MessageLog::new(dir.clone(), dir.join("events"))
    }
}

fn test_store() -> Arc<ConfigStore> {
    let path =
        std::env::temp_dir().join(format!("paigu-gateway-test-{}.json", uuid::Uuid::new_v4()));
    Arc::new(ConfigStore::load(path).expect("load test config"))
}

fn test_gateway() -> Arc<Gateway> {
    Gateway::new(test_store(), Arc::new(NullSink))
}

fn test_store_with(f: impl FnOnce(&mut crate::settings::AppConfig)) -> Arc<ConfigStore> {
    let mut cfg = crate::settings::default_config();
    f(&mut cfg);
    let path =
        std::env::temp_dir().join(format!("paigu-gateway-test-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
    Arc::new(ConfigStore::load(path).expect("load test config"))
}

#[tokio::test]
async fn send_action_rejects_send_actions() {
    let gw = test_gateway();
    let err = gw
        .send_action("send_group_msg", json!({ "group_id": "123456789" }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("forbidden"));
    assert!(gw.send_action("send_msg", json!({})).await.is_err());
}

#[tokio::test]
async fn send_action_rejects_send_even_if_whitelisted_when_reply_disabled() {
    let store = test_store_with(|cfg| {
        cfg.gateway.reply_enabled = false;
        cfg.gateway.allowed_actions = vec!["send_group_msg".to_string()];
    });
    let gw = Gateway::new(store, Arc::new(NullSink));
    let err = gw
        .send_action("send_group_msg", json!({ "group_id": "123456789" }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("forbidden"));
}

#[tokio::test]
async fn send_action_rejects_send_when_enabled_but_not_whitelisted() {
    let store = test_store_with(|cfg| {
        cfg.gateway.reply_enabled = true;
        cfg.gateway.allowed_actions = vec!["get_group_list".to_string()];
    });
    let gw = Gateway::new(store, Arc::new(NullSink));
    assert!(gw
        .send_action("send_group_msg", json!({ "group_id": "123456789" }))
        .await
        .is_err());
}

#[tokio::test]
async fn send_action_allows_send_when_enabled_and_whitelisted() {
    let store = test_store_with(|cfg| {
        cfg.gateway.reply_enabled = true;
        cfg.gateway.allowed_actions = vec!["send_group_msg".to_string()];
    });
    let gw = Gateway::new(store, Arc::new(NullSink));

    let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();
    gw.clients.lock().unwrap().insert(1, tx);

    let responder = gw.clone();
    let task = tokio::spawn(async move {
        if let Some(WsMessage::Text(text)) = rx.recv().await {
            let frame: Value = serde_json::from_str(text.as_str()).unwrap();
            let echo = frame["echo"].as_str().unwrap().to_string();
            responder.dispatch_response(&json!({
                "status": "ok",
                "retcode": 0,
                "data": { "message_id": 1 },
                "echo": echo
            }));
        }
    });

    let result = gw
        .send_action(
            "send_group_msg",
            json!({ "group_id": "123456789", "message": "x" }),
        )
        .await;
    assert!(result.is_ok(), "expected send allowed: {result:?}");
    assert_eq!(result.unwrap()["retcode"], 0);
    task.await.unwrap();
}

#[tokio::test]
async fn send_action_rejects_disallowed_action() {
    let gw = test_gateway();
    assert!(gw.send_action("set_group_ban", json!({})).await.is_err());
}

#[tokio::test]
async fn send_action_without_client_errors() {
    let gw = test_gateway();
    assert!(gw
        .send_action("get_group_member_list", json!({ "group_id": "123456789" }))
        .await
        .is_err());
}

#[tokio::test]
async fn reacts_with_emoji_on_routed_message() {
    let store = test_store_with(|cfg| {
        cfg.gateway.whitelist_groups = vec!["123456789".to_string()];
        cfg.gateway.react_emoji_id = Some(76);
    });
    let gw = Gateway::new(store, Arc::new(NullSink));
    let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();
    gw.clients.lock().unwrap().insert(1, tx);

    let event = json!({
        "post_type": "message",
        "message_type": "group",
        "group_id": 123456789,
        "user_id": 10000,
        "message_id": 555,
        "message": [{ "type": "text", "data": { "text": "排 通行证 1" } }],
        "sender": { "user_id": 10000, "nickname": "甲", "role": "member" },
        "time": 1000
    });
    gw.on_text(&event.to_string()).await;

    let mut frame = None;
    while let Ok(Some(WsMessage::Text(text))) =
        tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
    {
        let value: Value = serde_json::from_str(text.as_str()).unwrap();
        if value["action"] == "set_msg_emoji_like" {
            frame = Some(value);
            break;
        }
    }
    let frame = frame.expect("应发出 set_msg_emoji_like");
    assert_eq!(frame["params"]["emoji_id"], 76);
    assert_eq!(frame["params"]["message_id"], 555);
}

#[tokio::test]
async fn no_emoji_reaction_when_disabled() {
    let store = test_store_with(|cfg| {
        cfg.gateway.whitelist_groups = vec!["123456789".to_string()];
        cfg.gateway.react_emoji_id = None;
    });
    let gw = Gateway::new(store, Arc::new(NullSink));
    let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();
    gw.clients.lock().unwrap().insert(1, tx);

    let event = json!({
        "post_type": "message",
        "message_type": "group",
        "group_id": 123456789,
        "user_id": 10000,
        "message_id": 556,
        "message": [{ "type": "text", "data": { "text": "排 通行证 1" } }],
        "sender": { "user_id": 10000, "nickname": "甲", "role": "member" },
        "time": 1000
    });
    gw.on_text(&event.to_string()).await;

    let sent = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(sent.is_err(), "关闭时不应发出任何动作");
}

#[tokio::test]
async fn dispatch_response_round_trips_echo() {
    let gw = test_gateway();
    let (tx, rx) = oneshot::channel();
    gw.pending.lock().unwrap().insert("echo-1".to_string(), tx);

    let response = json!({
        "status": "ok",
        "retcode": 0,
        "data": { "user_id": 10001 },
        "echo": "echo-1"
    });
    assert!(gw.dispatch_response(&response));
    let received = rx.await.expect("response delivered");
    assert_eq!(received["retcode"], 0);
    assert_eq!(received["data"]["user_id"], 10001);

    assert!(!gw.dispatch_response(&response));
    assert!(!gw.dispatch_response(&json!({ "post_type": "message" })));
}

#[tokio::test]
async fn status_reports_shape() {
    let gw = test_gateway();
    let status = gw.status().await;
    assert_eq!(status["listening"], false);
    assert_eq!(status["clients"], 0);
    assert!(status.get("bound_addr").is_some());
    assert!(status.get("last_error").is_some());
}
