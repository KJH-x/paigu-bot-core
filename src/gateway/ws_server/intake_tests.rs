use super::*;
use crate::bus::IncomingEvent;
use async_trait::async_trait;
use tokio::sync::{Notify, Semaphore};

struct SlowSink {
    log: Arc<MessageLog>,
    entered: Notify,
    permits: Semaphore,
    received: Mutex<Vec<String>>,
}

#[async_trait]
impl EventSink for SlowSink {
    async fn handle(&self, event: IncomingEvent) {
        let _writer = self.log.write_guard().await;
        self.entered.notify_one();
        let Ok(permit) = self.permits.acquire().await else {
            return;
        };
        permit.forget();
        self.received.lock().unwrap().push(event.message_id);
    }
    fn messages(&self) -> Arc<MessageLog> {
        self.log.clone()
    }
}

fn message(id: i64) -> String {
    json!({"post_type":"message","message_type":"group","group_id":"123456789","user_id":"成员01","message_id":id,
        "message":[{"type":"text","data":{"text":"排 占位 1"}}],"sender":{"card":"成员01","role":"member"},"time":1000+id}).to_string()
}

struct Connection {
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
    directory: std::path::PathBuf,
}

#[tokio::test]
async fn safety_ws_oversize_message_is_rejected_before_durable_intake_or_delivery() {
    let (cfg, sink, owned) = slow_fixture();
    let gateway = Gateway::new(cfg.clone(), sink.clone());
    let oversized = "x".repeat(1024 * 1024 + 1);
    assert!(gateway.on_text(&oversized).await.is_err());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let gw = gateway.clone();
    let connection = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        gw.handle_connection(socket, 1).await
    });
    let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    // The transport may close while the client is still writing the oversized frame.
    let _ = client.send(WsMessage::Text(oversized.into())).await;
    tokio::time::timeout(Duration::from_secs(3), connection)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let round = cfg.get().await.round.round_id;
    assert!(!sink.log.has_pending_ingress(&round).unwrap());
    assert!(sink.log.read_only(&round).await.unwrap().is_empty());
    assert!(sink
        .log
        .read_events_strict(&round)
        .await
        .unwrap()
        .is_empty());
    assert!(sink.received.lock().unwrap().is_empty());
    assert_eq!(gateway.status().await["clients"], 0);
    drop(owned);
}

#[tokio::test]
async fn safety_dropped_gateway_frame_without_onebot_fields_cannot_become_a_domain_event() {
    use crate::domain::event::{DomainEvent, EventEnvelope, EventStatus, RoundPhaseSet};
    use crate::domain::ids::{EventId, RoundId, UserId};
    let (cfg, sink, owned) = slow_fixture();
    let gateway = Gateway::new(cfg.clone(), sink.clone());
    let round = cfg.get().await.round.round_id;
    let raw = serde_json::to_value(EventEnvelope {
        event_id: EventId("untrusted-frame".into()),
        round_id: RoundId(round.clone()),
        group_id: "123456789".into(),
        user_id: UserId("u1".into()),
        raw_message_id: None,
        event_type: "round_phase_set".into(),
        effective_at: chrono::Utc::now(),
        sequence: 1,
        payload: DomainEvent::RoundPhaseSet(RoundPhaseSet {
            phase: Some(crate::round::RoundPhase::Locked),
            set_by: UserId("u1".into()),
        }),
        status: EventStatus::Active,
    })
    .unwrap();
    gateway.on_text(&raw.to_string()).await.unwrap();
    wait_pending(&gateway, 0).await;
    assert!(sink.log.domain_events(&round).await.unwrap().is_empty());
    assert_eq!(
        sink.log.read_raw_events(&round, 0).await.unwrap(),
        vec![raw]
    );
    assert_eq!(
        sink.log.read_only(&round).await.unwrap()[0].status,
        "Dropped"
    );
    assert!(sink.received.lock().unwrap().is_empty());
    drop(owned);
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.server.abort();
        if self.directory.parent() == Some(std::env::temp_dir().as_path())
            && self
                .directory
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("paigu-intake-"))
        {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

#[tokio::test]
async fn slow_business_writer_cannot_block_same_connection_echo() {
    let directory = std::env::temp_dir().join(format!("paigu-intake-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let log = MessageLog::new(directory.join("messages"), directory.join("events"));
    let sink = Arc::new(SlowSink {
        log,
        entered: Notify::new(),
        permits: Semaphore::new(0),
        received: Mutex::new(vec![]),
    });
    let config = super::tests::test_store_with(|cfg| {
        cfg.llm.enabled = false;
        cfg.gateway.reply_enabled = false;
        cfg.gateway.whitelist_groups = vec!["123456789".into()];
        cfg.gateway.allowed_actions = vec!["get_group_list".into()];
        cfg.gateway.react_emoji_id = None;
    });
    let gateway = Gateway::new(config, sink.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let gw = gateway.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        gw.handle_connection(socket, 1).await
    });
    let owned = Connection { server, directory };
    let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    client
        .send(WsMessage::Text(message(1).into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), sink.entered.notified())
        .await
        .unwrap();
    client
        .send(WsMessage::Text(message(2).into()))
        .await
        .unwrap();
    for id in 3..=256 {
        client
            .send(WsMessage::Text(message(id).into()))
            .await
            .unwrap();
    }
    wait_pending(&gateway, 256).await;
    assert_eq!(gateway.status().await["intake"]["wake_capacity"], 1);

    let gw = gateway.clone();
    let mut action = tokio::spawn(async move { gw.send_action("get_group_list", json!({})).await });
    let echo = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let Some(Ok(WsMessage::Text(text))) = client.next().await else {
                continue;
            };
            let request: Value = serde_json::from_str(text.as_str()).unwrap();
            if request["action"] == "get_group_list" {
                break request["echo"].clone();
            }
        }
    })
    .await
    .unwrap();
    client
        .send(WsMessage::Text(
            json!({"status":"ok","retcode":0,"data":[],"echo":echo})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let delivered = tokio::time::timeout(Duration::from_millis(500), &mut action).await;
    let responsive = delivered
        .as_ref()
        .is_ok_and(|r| r.as_ref().is_ok_and(|v| v.is_ok()));
    if !responsive {
        action.abort();
    }
    sink.permits.add_permits(256);
    wait_pending(&gateway, 0).await;
    assert_eq!(
        *sink.received.lock().unwrap(),
        (1..=256).map(|id| id.to_string()).collect::<Vec<_>>()
    );
    let _ = client.close(None).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        while gateway.status().await["clients"] != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    drop(owned);
    assert!(
        responsive,
        "same-connection echo was blocked behind the business writer"
    );
}

async fn wait_pending(gateway: &Gateway, count: u64) {
    // This is a disk acceptance/drain deadline, not the separate 500ms echo
    // requirement. Full-suite fsync contention must not masquerade as echo loss.
    let completed = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if gateway.status().await["intake"]["pending"] == count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        completed.is_ok(),
        "pending intake did not converge to {count}: {}",
        gateway.status().await["intake"]
    );
}

fn slow_fixture() -> (Arc<ConfigStore>, Arc<SlowSink>, Connection) {
    let directory = std::env::temp_dir().join(format!("paigu-intake-{}", uuid::Uuid::new_v4()));
    let log = MessageLog::new(directory.join("messages"), directory.join("events"));
    let sink = Arc::new(SlowSink {
        log,
        entered: Notify::new(),
        permits: Semaphore::new(0),
        received: Mutex::new(vec![]),
    });
    let config = super::tests::test_store_with(|cfg| {
        cfg.round.round_id = "intake_test".into();
        cfg.round.group_id = "123456789".into();
        cfg.gateway.whitelist_groups = vec!["123456789".into()];
        cfg.gateway.whitelist_members.clear();
        cfg.gateway.react_emoji_id = None;
        cfg.gateway.reply_enabled = false;
        cfg.llm.enabled = false;
    });
    (
        config,
        sink,
        Connection {
            server: tokio::spawn(async { Ok(()) }),
            directory,
        },
    )
}

#[tokio::test]
async fn aborted_worker_resumes_durable_fifo_without_a_live_connection() {
    let (config, sink, _owned) = slow_fixture();
    let gateway = Gateway::new(config.clone(), sink.clone());
    gateway.on_text(&message(1)).await.unwrap();
    sink.entered.notified().await;
    gateway.on_text(&message(2)).await.unwrap();
    wait_pending(&gateway, 2).await;
    let weak = Arc::downgrade(&gateway);
    drop(gateway);
    assert!(
        weak.upgrade().is_none(),
        "worker must not retain Gateway forever"
    );
    let restored = Gateway::new(config, sink.clone());
    sink.permits.add_permits(2);
    restored.start_worker();
    wait_pending(&restored, 0).await;
    assert_eq!(*sink.received.lock().unwrap(), ["1", "2"]);
}

#[tokio::test]
async fn inactive_round_is_parked_and_binding_is_rechecked_before_delivery() {
    let (config, sink, _owned) = slow_fixture();
    let gateway = Gateway::new(config.clone(), sink.clone());
    let original = config.get().await;
    let event = serde_json::from_str(&message(1)).unwrap();
    gateway.intake.accept(&original, event, None).await.unwrap();
    let mut changed = original.clone();
    changed.round.round_id = "intake_other".into();
    config.put(changed, original.revision).await.unwrap();
    gateway.start_worker();
    gateway.on_text(&message(2)).await.unwrap();
    sink.permits.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while sink.received.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(*sink.received.lock().unwrap(), ["2"]);
    wait_pending(&gateway, 1).await;
    let now = config.get().await;
    let mut wrong_binding = original.clone();
    wrong_binding.round.group_id = "other_group".into();
    config.put(wrong_binding, now.revision).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*sink.received.lock().unwrap(), ["2"]);
    let now = config.get().await;
    config.put(original, now.revision).await.unwrap();
    sink.permits.add_permits(1);
    wait_pending(&gateway, 0).await;
    assert_eq!(*sink.received.lock().unwrap(), ["2", "1"]);
}

struct FailingSink {
    log: Arc<MessageLog>,
    attempts: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl EventSink for FailingSink {
    async fn handle(&self, _: IncomingEvent) {
        panic!("must use durable delivery");
    }
    async fn deliver(
        &self,
        _: &crate::bus::IngressBinding,
        _: IncomingEvent,
    ) -> anyhow::Result<crate::bus::Delivery> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            anyhow::bail!("injected write failure");
        }
        Ok(crate::bus::Delivery::Committed)
    }
    fn messages(&self) -> Arc<MessageLog> {
        self.log.clone()
    }
}

#[tokio::test]
async fn failure_keeps_file_and_pauses_until_explicit_retry() {
    let (config, slow, _owned) = slow_fixture();
    let sink = Arc::new(FailingSink {
        log: slow.log.clone(),
        attempts: std::sync::atomic::AtomicUsize::new(0),
    });
    let gateway = Gateway::new(config, sink.clone());
    let mut raw: Value = serde_json::from_str(&message(1)).unwrap();
    raw["extra_metadata"] = json!("占位元数据");
    gateway.on_text(&raw.to_string()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while gateway.status().await["intake"]["paused"] != true {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        sink.attempts.load(Ordering::SeqCst),
        1,
        "no hidden retry/model loop"
    );
    assert!(slow.log.has_pending_ingress("intake_test").unwrap());
    gateway.retry_intake().await;
    wait_pending(&gateway, 0).await;
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 2);
    let raw = slow.log.read_raw_events("intake_test", 0).await.unwrap();
    assert_eq!(
        raw.len(),
        1,
        "retry after raw checkpoint must not append the frame again"
    );
    assert_eq!(raw[0]["extra_metadata"], "占位元数据");
}

#[tokio::test]
async fn failed_worker_is_visible_and_explicit_retry_recreates_only_its_wake_channel() {
    struct PanicSink {
        log: Arc<MessageLog>,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl EventSink for PanicSink {
        async fn handle(&self, _: IncomingEvent) {
            assert_ne!(
                self.calls.fetch_add(1, Ordering::SeqCst),
                0,
                "injected worker panic"
            );
        }
        fn messages(&self) -> Arc<MessageLog> {
            self.log.clone()
        }
    }
    let (config, slow, _owned) = slow_fixture();
    let sink = Arc::new(PanicSink {
        log: slow.log.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let gateway = Gateway::new(config, sink.clone());
    gateway.on_text(&message(1)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while gateway.status().await["intake"]["paused"] != true {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    gateway.on_text(&message(2)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        sink.calls.load(Ordering::SeqCst),
        1,
        "new arrivals cannot silently restart failed work"
    );
    gateway.retry_intake().await;
    wait_pending(&gateway, 0).await;
    assert_eq!(sink.calls.load(Ordering::SeqCst), 3);
}
