use super::*;
use crate::messages::MessageRecord;
use crate::replay::session::{replay_messages, ReplayOverrides};
use crate::settings::{default_config, ItemConfig, VariantConfig};
use serde_json::json;

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paigu-bundle-{tag}-{}", uuid::Uuid::new_v4()))
}

fn sample_bundle() -> SnapshotBundle {
    SnapshotBundle::seal(
        "test_round",
        3,
        "2026-09-14T00:00:00Z",
        json!({ "revision": 3 }),
        json!([{ "seq": 1, "text": "排 徽章 甲 1" }]),
        json!([{ "event_type": "claim_created" }]),
        json!({ "version": 1 }),
        Value::Null,
    )
    .unwrap()
}

#[test]
fn seal_computes_hash_and_counts() {
    let bundle = sample_bundle();
    let manifest = bundle.manifest_typed().unwrap();
    assert_eq!(manifest.round_id, "test_round");
    assert_eq!(manifest.revision, 3);
    assert_eq!(manifest.message_count, 1);
    assert_eq!(manifest.event_count, 1);
    assert_eq!(manifest.version, 1);
    assert_eq!(bundle.verify().unwrap(), manifest);
}

#[test]
fn export_import_round_trips_and_detects_tamper() {
    let bundle = sample_bundle();
    let dir = temp_dir("roundtrip");
    bundle.export_dir(&dir).unwrap();

    let imported = SnapshotBundle::import_dir(&dir).unwrap();
    assert_eq!(
        serde_json::to_value(&imported).unwrap(),
        serde_json::to_value(&bundle).unwrap()
    );

    std::fs::write(dir.join(MESSAGES_FILE), "{\"seq\":9}\n").unwrap();
    assert!(SnapshotBundle::import_dir(&dir).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn imported_bundle_replays_to_same_board() {
    let mut cfg = default_config();
    cfg.llm.enabled = false;
    cfg.round.round_id = "test_round".to_string();
    cfg.round.priority_users = vec![];
    cfg.round.priority_window = None;
    cfg.round.phases = vec![];
    cfg.round.items = vec![ItemConfig {
        item_id: "badge".to_string(),
        name: "徽章".to_string(),
        kind: "split".to_string(),
        class: Some("B".to_string()),
        unit_price_cents: 0,
        box_size: None,
        max_quantity: None,
        aliases: vec!["徽章".to_string()],
        variants: vec![VariantConfig {
            variant_id: "v_a".to_string(),
            name: "甲".to_string(),
            unit_price_cents: 0,
            pieces: 0,
            capacity: Some(1),
            aliases: vec![],
        }],
    }];

    let record = MessageRecord {
        seq: 1,
        group_id: "123456789".to_string(),
        user_id: "u1".to_string(),
        nickname: "u1".to_string(),
        message_id: "m1".to_string(),
        text: "排 徽章 甲 1".to_string(),
        timestamp_ms: 1_000,
        is_admin: false,
        routed: "message".to_string(),
        status: "Applied".to_string(),
        detail: String::new(),
    };

    let result = replay_messages(
        &cfg,
        std::slice::from_ref(&record),
        ReplayOverrides::default(),
    )
    .await
    .unwrap();

    let bundle = SnapshotBundle::seal(
        cfg.round.round_id.clone(),
        cfg.revision,
        "2026-09-14T00:00:00Z",
        serde_json::to_value(&cfg).unwrap(),
        serde_json::to_value(&[record]).unwrap(),
        serde_json::to_value(&result.events).unwrap(),
        serde_json::to_value(&result.board).unwrap(),
        Value::Null,
    )
    .unwrap();

    let export_dir = temp_dir("export");
    bundle.export_dir(&export_dir).unwrap();
    let imported = SnapshotBundle::import_dir(&export_dir).unwrap();

    let records: Vec<MessageRecord> = serde_json::from_value(imported.messages.clone()).unwrap();
    let imported_cfg: crate::settings::AppConfig =
        serde_json::from_value(imported.config.clone()).unwrap();
    let recomputed = replay_messages(&imported_cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();

    assert_eq!(
        serde_json::to_value(&recomputed.board).unwrap(),
        serde_json::to_value(&result.board).unwrap()
    );

    let _ = std::fs::remove_dir_all(&export_dir);
}

#[test]
fn single_json_snapshot_round_trips() {
    let bundle = SnapshotBundle::seal(
        "r1",
        3,
        "2026-09-17T00:00:00Z",
        serde_json::json!({ "a": 1 }),
        serde_json::json!([{ "seq": 1, "text": "x" }]),
        serde_json::json!([{ "raw": true }]),
        serde_json::json!({ "version": 7 }),
        serde_json::json!({ "grand_total": 100 }),
    )
    .unwrap();

    let dir = temp_dir("snapfile");
    let path = dir.join("s.json");
    bundle.export_file(&path).unwrap();

    let back = SnapshotBundle::import_file(&path).unwrap();
    let manifest = back.manifest_typed().unwrap();
    assert_eq!(manifest.version, 7);
    assert_eq!(manifest.message_count, 1);
    assert_eq!(manifest.event_count, 1);
    assert_eq!(back.config, bundle.config);
    assert_eq!(back.settlement, bundle.settlement);
    back.verify().unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_file_rejects_unknown_format() {
    let dir = temp_dir("snapfile-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.json");
    std::fs::write(&path, r#"{"format":"nope"}"#).unwrap();
    assert!(SnapshotBundle::import_file(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
