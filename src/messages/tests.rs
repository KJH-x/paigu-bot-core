use super::*;

#[test]
fn private_snapshot_root_follows_the_message_store_unless_explicitly_overridden() {
    let normal = MessageLog::new("data/messages", "data/events");
    assert_eq!(
        normal.snapshot_dir_override(None),
        PathBuf::from("data/snapshots")
    );
    let isolated = MessageLog::new("owned/messages", "owned/events");
    assert_eq!(
        isolated.snapshot_dir_override(None),
        PathBuf::from("owned/snapshots")
    );
    for empty in ["", "  "] {
        assert_eq!(
            isolated.snapshot_dir_override(Some(empty)),
            PathBuf::from("owned/snapshots")
        );
    }
    assert_eq!(
        isolated.snapshot_dir_override(Some("explicit/private")),
        PathBuf::from("explicit/private")
    );
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("paigu-msgs-test-{}", uuid::Uuid::new_v4()))
}

fn record(seq: i64, text: &str) -> MessageRecord {
    MessageRecord {
        record_id: format!("record-{seq}"),
        record_revision: 1,
        processing: None,
        seq,
        group_id: "123456789".to_string(),
        user_id: "10001".to_string(),
        nickname: "成员01".to_string(),
        message_id: format!("m{seq}"),
        text: text.to_string(),
        timestamp_ms: 1_788_782_400_000 + seq,
        is_admin: false,
        routed: "message".to_string(),
        status: "Applied".to_string(),
        detail: "claim".to_string(),
        sender_user_id: None,
        delegation: None,
        ingress: None,
        sender_nickname: None,
    }
}

#[test]
fn messages_dir_from_defaults_and_overrides() {
    assert_eq!(messages_dir_from(None), PathBuf::from(DEFAULT_MESSAGES_DIR));
    assert_eq!(
        messages_dir_from(Some("  ".to_string())),
        PathBuf::from(DEFAULT_MESSAGES_DIR)
    );
    assert_eq!(
        messages_dir_from(Some("tmp/msgs".to_string())),
        PathBuf::from("tmp/msgs")
    );
}

#[tokio::test]
async fn read_only_context_does_not_migrate_or_rewrite_a_legacy_message_file() {
    let dir = temp_dir();
    let store = JsonlMessageStore::new(&dir, "readonly").unwrap();
    let mut old = record(7, "历史消息");
    old.record_id.clear();
    store.append(&old).await.unwrap();
    let before = std::fs::read(store.path()).unwrap();
    let first = store.read_only().await.unwrap();
    let second = store.read_only().await.unwrap();
    assert!(first[0].record_id.starts_with("legacy-"));
    assert_eq!(first[0].record_id, second[0].record_id);
    assert_eq!(std::fs::read(store.path()).unwrap(), before);
    assert!(!store
        .path()
        .with_extension("jsonl.pre-record-id.bak")
        .exists());
}

#[test]
fn file_handle_uses_round_id_filename() {
    let store = JsonlMessageStore::new("data/messages", "月行水上").unwrap();
    assert_eq!(
        store.path(),
        Path::new("data/messages").join("月行水上.jsonl")
    );
}

#[tokio::test]
async fn append_then_read_round_trips_in_order() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append("r1", &record(1, "第一条")).await.unwrap();
    log.append("r1", &record(2, "第二条")).await.unwrap();

    let all = log.read_all("r1").await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].seq, 1);
    assert_eq!(all[0].text, "第一条");
    assert_eq!(all[1].seq, 2);
    assert_eq!(all[1].nickname, "成员01");
}

#[tokio::test]
async fn read_all_missing_file_is_empty() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    assert!(log.read_all("missing").await.unwrap().is_empty());
}

#[tokio::test]
async fn replace_all_overwrites_existing() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append("r2", &record(1, "旧一")).await.unwrap();
    log.append("r2", &record(2, "旧二")).await.unwrap();

    log.replace_all("r2", &[record(9, "新唯一")]).await.unwrap();
    let all = log.read_all("r2").await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].seq, 9);
    assert_eq!(all[0].text, "新唯一");
}

#[tokio::test]
async fn replace_all_with_empty_clears_file() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append("r3", &record(1, "x")).await.unwrap();
    log.replace_all("r3", &[]).await.unwrap();
    assert!(log.read_all("r3").await.unwrap().is_empty());
}

#[tokio::test]
async fn query_update_and_delete_by_record_id() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append("r4", &record(1, "甲")).await.unwrap();
    log.append("r4", &record(2, "乙")).await.unwrap();

    let hit = log.query("r4", |r| r.text == "乙").await.unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].seq, 2);

    assert!(log
        .update("r4", "record-2", |r| r.status = "Edited".to_string())
        .await
        .unwrap()
        .is_some());
    let edited = log.query("r4", |r| r.seq == 2).await.unwrap();
    assert_eq!(edited[0].status, "Edited");

    assert!(log.delete("r4", "record-1").await.unwrap());
    assert!(!log.delete("r4", "record-1").await.unwrap());
    let remaining = log.read_all("r4").await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].seq, 2);
}

#[tokio::test]
async fn duplicate_seq_updates_exactly_one_record() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    let mut first = record(7, "甲");
    first.record_id = "first".to_string();
    let mut second = record(7, "乙");
    second.record_id = "second".to_string();
    log.append("dup", &first).await.unwrap();
    log.append("dup", &second).await.unwrap();

    log.update("dup", "second", |record| record.text = "已修改".to_string())
        .await
        .unwrap();
    let records = log.read_all("dup").await.unwrap();
    assert_eq!(records[0].text, "甲");
    assert_eq!(records[1].text, "已修改");
    assert!(matches!(
        log.resolve_record_id("dup", "7").await.unwrap(),
        Err(2)
    ));
}

#[tokio::test]
async fn legacy_log_is_migrated_with_backup() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.jsonl");
    let mut value = serde_json::to_value(record(3, "旧记录")).unwrap();
    value.as_object_mut().unwrap().remove("record_id");
    value.as_object_mut().unwrap().remove("record_revision");
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&value).unwrap()),
    )
    .unwrap();
    let log = MessageLog::new(&dir, dir.join("events"));

    let records = log.read_all("legacy").await.unwrap();
    assert!(records[0].record_id.starts_with("legacy-"));
    assert_eq!(records[0].record_revision, 1);
    assert!(dir.join("legacy.jsonl.pre-record-id.bak").exists());
}

#[tokio::test]
async fn raw_events_round_trip() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    let ev = serde_json::json!({ "post_type": "message", "user_id": 10001 });
    log.append_raw_event("r5", &ev).await.unwrap();

    let events = log.read_raw_events("r5", 0).await.unwrap();
    assert_eq!(events, vec![ev]);
}

#[test]
fn next_seq_is_monotonic() {
    let a = next_seq();
    let b = next_seq();
    assert!(b > a);
}
