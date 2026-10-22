use super::*;

struct Owned(std::path::PathBuf);
impl Drop for Owned {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("paigu-intake-storage-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Owned, Arc<MessageLog>, crate::settings::AppConfig) {
    let owned =
        Owned(std::env::temp_dir().join(format!("paigu-intake-storage-{}", uuid::Uuid::new_v4())));
    let log = MessageLog::new(owned.0.join("messages"), owned.0.join("events"));
    let mut cfg = crate::settings::default_config();
    cfg.round.round_id = "intake_storage".into();
    cfg.gateway.reply_enabled = false;
    cfg.llm.enabled = false;
    cfg.gateway.react_emoji_id = None;
    (owned, log, cfg)
}
fn event() -> onebot::RouteMessageEvent {
    serde_json::from_value(serde_json::json!({"post_type":"message","message_type":"group","group_id":"123456789","message_id":1,"user_id":"成员01","message":"排 占位 1","time":1000})).unwrap()
}

#[tokio::test]
async fn durable_fifo_reopens_with_one_coalesced_wake_and_exact_ack() {
    let (_owned, log, cfg) = fixture();
    let intake = Intake::new(&log);
    for _ in 0..32 {
        intake.accept(&cfg, event(), None).await.unwrap();
    }
    let mut wake = intake.receiver().unwrap();
    assert!(wake.try_recv().is_ok());
    assert!(wake.try_recv().is_err());
    drop(intake);
    let reopened = Intake::new(&log);
    assert_eq!(reopened.status().await["pending"], 32);
    let mut last = 0;
    for _ in 0..32 {
        let entry = reopened.next(&cfg.round.round_id).await.unwrap().unwrap();
        assert!(entry.sequence > last);
        last = entry.sequence;
        reopened.ack(&entry).await.unwrap();
    }
    assert!(!log.has_pending_ingress(&cfg.round.round_id).unwrap());
}

#[tokio::test]
async fn corrupt_head_is_never_skipped_to_process_later_input() {
    let (_owned, log, cfg) = fixture();
    let intake = Intake::new(&log);
    intake.accept(&cfg, event(), None).await.unwrap();
    intake.accept(&cfg, event(), None).await.unwrap();
    let entry = intake.next(&cfg.round.round_id).await.unwrap().unwrap();
    let path = intake.path(&cfg.round.round_id, entry.sequence);
    let original = std::fs::read(&path).unwrap();
    crate::persistence::atomic_write(&path, b"broken input").unwrap();
    assert!(intake.next(&cfg.round.round_id).await.is_err());
    assert_eq!(intake.status().await["pending"], 2);
    assert!(path.exists());
    crate::persistence::atomic_write(&path, &original).unwrap();
    assert_eq!(
        intake.next(&cfg.round.round_id).await.unwrap().unwrap().id,
        entry.id
    );
}

#[tokio::test]
async fn failed_storage_or_ack_never_decrements_durable_backlog() {
    let (_owned, log, cfg) = fixture();
    let intake = Intake::new(&log);
    std::fs::create_dir_all(intake.directory.parent().unwrap()).unwrap();
    std::fs::write(&intake.directory, b"not a directory").unwrap();
    assert!(intake.accept(&cfg, event(), None).await.is_err());
    assert_eq!(intake.status().await["paused"], true);
    std::fs::remove_file(&intake.directory).unwrap();
    intake.retry().await;
    intake.accept(&cfg, event(), None).await.unwrap();
    let entry = intake.next(&cfg.round.round_id).await.unwrap().unwrap();
    let path = intake.path(&cfg.round.round_id, entry.sequence);
    let raw = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(intake.ack(&entry).await.is_err());
    assert_eq!(intake.status().await["pending"], 1);
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, raw).unwrap();
    intake.ack(&entry).await.unwrap();
    assert_eq!(intake.status().await["pending"], 0);
}

#[tokio::test]
async fn unknown_files_block_recovery_and_destructive_operations() {
    let (_owned, log, cfg) = fixture();
    let intake = Intake::new(&log);
    let round = intake.directory.join(&cfg.round.round_id);
    std::fs::create_dir_all(&round).unwrap();
    std::fs::write(round.join("unknown.backup"), b"keep me").unwrap();
    assert!(log.has_pending_ingress(&cfg.round.round_id).unwrap());
    assert!(intake.next(&cfg.round.round_id).await.is_err());
    assert_eq!(intake.status().await["paused"], true);
    assert_eq!(
        std::fs::read(round.join("unknown.backup")).unwrap(),
        b"keep me"
    );
    assert!(intake.accept(&cfg, event(), None).await.is_err());
}

#[tokio::test]
async fn tampered_identity_or_version_cannot_be_acknowledged_as_valid_input() {
    let (_owned, log, cfg) = fixture();
    let intake = Intake::new(&log);
    intake.accept(&cfg, event(), None).await.unwrap();
    let entry = intake.next(&cfg.round.round_id).await.unwrap().unwrap();
    let path = intake.path(&cfg.round.round_id, entry.sequence);
    let original = std::fs::read(&path).unwrap();
    for (field, value) in [
        ("id", serde_json::json!("invalid")),
        ("version", serde_json::json!(2)),
        ("sequence", serde_json::json!(99)),
    ] {
        let mut changed = serde_json::to_value(&entry).unwrap();
        changed[field] = value;
        crate::persistence::atomic_write(&path, &serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(intake.next(&cfg.round.round_id).await.is_err());
        assert_eq!(intake.status().await["pending"], 1);
    }
    crate::persistence::atomic_write(&path, &original).unwrap();
    assert_eq!(
        intake.next(&cfg.round.round_id).await.unwrap().unwrap().id,
        entry.id
    );
}
