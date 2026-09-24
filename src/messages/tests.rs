use super::*;

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("paigu-msgs-test-{}", uuid::Uuid::new_v4()))
}

fn record(seq: i64, text: &str) -> MessageRecord {
    MessageRecord {
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
        detail: "ok".to_string(),
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

#[test]
fn file_handle_uses_round_id_filename() {
    let store = JsonlMessageStore::new("data/messages", "月行水上");
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
async fn query_update_and_delete_by_seq() {
    let dir = temp_dir();
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append("r4", &record(1, "甲")).await.unwrap();
    log.append("r4", &record(2, "乙")).await.unwrap();

    let hit = log.query("r4", |r| r.text == "乙").await.unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].seq, 2);

    assert!(log
        .update("r4", 2, |r| r.status = "Edited".to_string())
        .await
        .unwrap());
    let edited = log.query("r4", |r| r.seq == 2).await.unwrap();
    assert_eq!(edited[0].status, "Edited");

    assert!(log.delete("r4", 1).await.unwrap());
    assert!(!log.delete("r4", 1).await.unwrap());
    let remaining = log.read_all("r4").await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].seq, 2);
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
