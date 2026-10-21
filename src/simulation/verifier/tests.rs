use super::*;

pub(super) struct FixtureDir(pub(super) std::path::PathBuf);

impl FixtureDir {
    pub(super) fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("paigu-cli-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub(super) fn queue(&self, records: &[QueueMessageRecord]) -> std::path::PathBuf {
        let path = self.0.join("queue.jsonl");
        let body = records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("paigu-cli-contract-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn fixture() -> RoundFixture {
    let mut config = crate::settings::default_config();
    config.llm.enabled = false;
    config.gateway.reply_enabled = false;
    config.gateway.whitelist_groups = vec!["g1".into()];
    config.members.cache_path.clear();
    config.members.cn_overrides.clear();
    config.round.round_id = "round_x".into();
    config.round.title = "公开占位团".into();
    config.round.group_id = "g1".into();
    config.round.priority_users.clear();
    config.round.priority_window = None;
    config.round.phases.clear();
    config.round.items = fixture::catalog_settings(&[sample_item()]);
    RoundFixture {
        fixture_version: 2,
        config,
        items: vec![sample_item()],
    }
}
use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::ItemKind;
use crate::domain::money::MoneyCents;
use crate::simulation::queue_file::QueueMessageRecord;

fn sample_item() -> Item {
    Item {
        item_id: ItemId("pass_sp".to_string()),
        round_id: RoundId("round_x".to_string()),
        name: "通行认证SP-月行水上".to_string(),
        kind: ItemKind::Split,
        unit_price: MoneyCents(4500),
        box_size: Some(2),
        max_quantity: None,
        is_blind: false,
        is_proxy_card: false,
        aliases: vec!["通行证".to_string()],
        sort_order: 0,
        metadata: serde_json::json!({}),
        subitems: vec![],
    }
}

pub(super) fn record(sequence: u64, user: &str, text: &str) -> QueueMessageRecord {
    QueueMessageRecord {
        source_sequence: sequence,
        group_id: "g1".to_string(),
        user_id: user.to_string(),
        nickname: format!("成员{sequence}"),
        message_id: format!("m{sequence}"),
        timestamp_ms: 1_700_000_000_000 + sequence as i64 * 1000,
        text: text.to_string(),
        attachments: vec![],
        reply_to_message_id: None,
        is_admin: false,
    }
}

#[tokio::test]
async fn end_to_end_parse_validate_replay_compares_slots() {
    let path = std::env::temp_dir().join(format!("paigu-verifier-{}.jsonl", uuid::Uuid::new_v4()));
    let records = [
        record(1, "u1", "排 通行证 1"),
        record(2, "u2", "排 通行证 1"),
    ];
    let body = records
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, body).unwrap();

    let result = verify(&path, fixture()).await.expect("verify");
    let _ = std::fs::remove_file(&path);

    assert_eq!(result.applied_count, 2);
    assert_eq!(result.rejected_count, 0);

    let allocation = result
        .final_snapshot
        .item_allocations
        .iter()
        .find(|i| i.item_id.0 == "pass_sp")
        .expect("pass_sp allocation");
    assert_eq!(allocation.boxes.len(), 1);
    let slots = &allocation.boxes[0].slots;
    assert_eq!(slots[0].user_id_str(), Some("u1"));
    assert_eq!(slots[1].user_id_str(), Some("u2"));
}

#[test]
fn fixture_loader_preserves_explicit_legacy_catalog_inputs() {
    let dir = FixtureDir::new();
    let path = dir.0.join("round.json");
    std::fs::write(&path, serde_json::json!({
        "round_id":"fixture", "title":"占位", "group_id":"123456789",
        "priority_users":["成员01"], "priority_window":{"start_ms":1000,"end_ms":2000},
        "items":[{"item_id":"badge","name":"徽章","kind":"split","box_size":2,
            "unit_price":1700,"aliases":["吧唧"],
            "subitems":[{"subitem_id":"a","name":"甲","capacity":3,"unit_price_cents":2100,"aliases":["款甲"]}]}]
    }).to_string()).unwrap();
    let loaded = load_round_fixture(&path).unwrap();
    assert_eq!(loaded.config.round.round_id, "fixture");
    assert_eq!(loaded.config.round.priority_users, ["成员01"]);
    assert_eq!(
        loaded
            .config
            .round
            .priority_window
            .as_ref()
            .map(|w| (w.start_ms, w.end_ms)),
        Some((1000, 2000))
    );
    let item = &loaded.items[0];
    assert_eq!(item.box_size, Some(2));
    assert_eq!(item.unit_price.0, 1700);
    assert_eq!(item.aliases, ["吧唧"]);
    assert_eq!(item.subitems[0].capacity, Some(3));
    assert_eq!(item.subitems[0].unit_price.0, 2100);
    assert_eq!(item.subitems[0].aliases, ["款甲"]);
}

#[tokio::test]
async fn ordered_queue_duplicate_cancel_and_report_contract() {
    let dir = FixtureDir::new();
    let first = record(1, "u2", "排 通行证 1");
    let second = record(2, "u1", "排 通行证 2");
    let cancel = record(3, "u1", "撤 通行证 1");
    let mut duplicate = record(4, "u2", "排 通行证 9");
    duplicate.message_id = first.message_id.clone();
    let chatter = record(5, "u2", "占位闲聊");
    let queue = dir.queue(&[cancel, duplicate, second, chatter, first]);
    let result = verify(&queue, fixture()).await.unwrap();
    assert_eq!(result.message_count, 5);
    assert_eq!(result.applied_count, 3);
    assert_eq!(result.duplicate_count, 1);
    assert_eq!(result.ignored_count, 1);
    assert_eq!(
        result
            .outcomes
            .iter()
            .map(|o| o.source_sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(
        result
            .outcomes
            .iter()
            .map(|o| o.status.as_str())
            .collect::<Vec<_>>(),
        ["Applied", "Applied", "Applied", "Duplicate", "Ignored"]
    );
    let slots = &result.final_snapshot.item_allocations[0].boxes[0].slots;
    assert_eq!(
        slots.iter().map(|s| s.user_id_str()).collect::<Vec<_>>(),
        [Some("u2"), Some("u1")]
    );
    assert_eq!(result.replay.steps.len(), 3);
    assert_eq!(
        serde_json::to_value(&result.final_snapshot).unwrap(),
        serde_json::to_value(&result.replay.final_snapshot).unwrap()
    );

    let output = dir.0.join("out");
    report::write_outputs(&output, &result).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("result.json")).unwrap())
            .unwrap();
    fn assert_keys(value: &serde_json::Value, expected: &[&str]) {
        let actual = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected.iter().copied().collect());
    }
    assert_keys(
        &json,
        &[
            "round_id",
            "group_id",
            "message_count",
            "applied_count",
            "rejected_count",
            "need_confirm_count",
            "ignored_count",
            "unsupported_count",
            "duplicate_count",
            "dropped_count",
            "merged_count",
            "error_count",
            "outcomes",
            "eligibility",
            "final_snapshot",
            "final_settlement",
            "replay",
        ],
    );
    assert_keys(
        &json["outcomes"][0],
        &[
            "source_sequence",
            "message_id",
            "user_id",
            "nickname",
            "text",
            "status",
            "detail",
        ],
    );
    assert_keys(
        &json["replay"],
        &[
            "replay_id",
            "final_snapshot",
            "final_settlement",
            "steps",
            "input_message_count",
        ],
    );
    assert_keys(
        &json["replay"]["steps"][0],
        &[
            "replay_id",
            "round_id",
            "step_index",
            "event_id",
            "raw_message_id",
            "occurred_at",
            "input_event",
            "before_version",
            "after_version",
            "state_diff",
            "allocation_snapshot",
            "settlement_snapshot",
            "decision_trace",
            "warnings",
            "errors",
        ],
    );
    assert_eq!(json, serde_json::to_value(&result).unwrap());
    let outcomes = std::fs::read_to_string(output.join("outcomes.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = outcomes
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines, json["outcomes"].as_array().unwrap().clone());
    let markdown = std::fs::read_to_string(output.join("report.md")).unwrap();
    assert!(markdown.contains("消息总数: 5"));
    assert!(markdown.contains("重复(Duplicate): 1"));
    assert!(markdown.contains("box1: u2 | u1"));
}

#[tokio::test]
async fn priority_half_open_window_and_legacy_admin_grant_are_explicit_baselines() {
    let dir = FixtureDir::new();
    let mut window = fixture();
    window.config.round.priority_users = vec!["u1".into()];
    window.config.round.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 1_700_000_001_000,
        end_ms: 1_700_000_003_000,
    });
    let queue = dir.queue(&[
        record(1, "u2", "排 通行证 1"),
        record(2, "u1", "排 通行证 1"),
        record(3, "u2", "排 通行证 1"),
    ]);
    let result = verify(&queue, window).await.unwrap();
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.applied_count, 2);
    assert_eq!(
        result.final_snapshot.item_allocations[0].boxes[0].slots[0].user_id_str(),
        Some("u1")
    );

    let mut command = record(2, "admin", "/加优先 用户=u1 等级=20 范围=全部");
    command.is_admin = true;
    let queue = dir.queue(&[
        record(1, "u2", "排 通行证 1"),
        command,
        record(3, "u1", "排 通行证 1"),
    ]);
    let result = verify(&queue, fixture()).await.unwrap();
    assert_eq!(result.applied_count, 3);
    assert_eq!(result.eligibility.len(), 1);
    assert_eq!(result.eligibility[0].user_id.0, "u1");
    assert_eq!(result.eligibility[0].priority_level, 20);
    assert_eq!(
        result.final_snapshot.item_allocations[0].boxes[0].slots[0].user_id_str(),
        Some("u1")
    );
}
