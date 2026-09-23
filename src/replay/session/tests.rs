use super::*;
use crate::messages::MessageLog;
use crate::round::RoundPhase;
use crate::settings::{default_config, VariantConfig};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("paigu-session-{tag}-{}", uuid::Uuid::new_v4()))
}

fn test_config() -> AppConfig {
    let mut cfg = default_config();
    cfg.llm.enabled = false;
    cfg.round.round_id = "test_round".to_string();
    cfg.round.title = "测试团".to_string();
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
    cfg
}

fn record(seq: i64, user_id: &str, text: &str, ts: i64) -> MessageRecord {
    MessageRecord {
        seq,
        group_id: "123456789".to_string(),
        user_id: user_id.to_string(),
        nickname: user_id.to_string(),
        message_id: format!("m{seq}"),
        text: text.to_string(),
        timestamp_ms: ts,
        is_admin: false,
        routed: "message".to_string(),
        status: "Applied".to_string(),
        detail: String::new(),
    }
}

fn slot_user(board: &AllocationSnapshot, box_index: u32, slot_index: u32) -> Option<String> {
    board
        .item_allocations
        .iter()
        .find(|ia| ia.item_id.0 == "badge")
        .and_then(|ia| ia.boxes.iter().find(|b| b.box_index == box_index))
        .and_then(|b| b.slots.iter().find(|s| s.slot_index == slot_index))
        .and_then(|s| s.user_id.as_ref().map(|u| u.0.clone()))
}

#[tokio::test]
async fn replay_is_deterministic() {
    let cfg = test_config();
    let records = vec![
        record(1, "u1", "排 徽章 甲 1", 1_000),
        record(2, "u2", "排 徽章 甲 1", 2_000),
    ];
    let first = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();
    let second = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();

    assert_eq!(
        serde_json::to_value(&first.board).unwrap(),
        serde_json::to_value(&second.board).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.outcomes).unwrap(),
        serde_json::to_value(&second.outcomes).unwrap()
    );
    assert_eq!(first.version, second.version);
}

#[tokio::test]
async fn override_priority_changes_board() {
    let cfg = test_config();
    let records = vec![
        record(1, "u1", "排 徽章 甲 1", 1_000),
        record(2, "u2", "排 徽章 甲 1", 2_000),
    ];

    let base = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(slot_user(&base.board, 1, 1).as_deref(), Some("u1"));

    let overridden = replay_messages(
        &cfg,
        &records,
        ReplayOverrides {
            priority_users: Some(vec!["u2".to_string()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(slot_user(&overridden.board, 1, 1).as_deref(), Some("u2"));
    assert!(overridden.diff.changed);
    assert!(!overridden.diff.state.slot_changes.is_empty());
}

#[tokio::test]
async fn override_phases_rejects_and_empties_board() {
    let cfg = test_config();
    let records = vec![record(1, "u1", "排 徽章 甲 1", 1_000)];

    let base = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(base.outcomes[0].status, "Applied");

    let locked = replay_messages(
        &cfg,
        &records,
        ReplayOverrides {
            phases: Some(vec![PhaseWindow {
                phase: RoundPhase::Locked,
                start_ms: 0,
                end_ms: i64::MAX,
            }]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(locked.outcomes[0].status, "Rejected");
    assert!(locked.board.user_summaries.is_empty());
    assert!(locked.diff.changed);
}

#[tokio::test]
async fn override_whitelist_drops_outsider() {
    let cfg = test_config();
    let records = vec![
        record(1, "u1", "排 徽章 甲 1", 1_000),
        record(2, "u2", "排 徽章 甲 1", 2_000),
    ];

    let filtered = replay_messages(
        &cfg,
        &records,
        ReplayOverrides {
            whitelist_members: Some(vec!["u1".to_string()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(filtered.outcomes[0].status, "Applied");
    assert_eq!(filtered.outcomes[1].status, "Dropped");
    assert!(filtered
        .board
        .user_summaries
        .iter()
        .all(|s| s.user_id.0 == "u1"));
}

#[tokio::test]
async fn replay_reads_store_and_supports_edit_recompute() {
    let cfg = test_config();
    let dir = temp_dir("store");
    let log = MessageLog::new(&dir, dir.join("events"));
    log.append(&cfg.round.round_id, &record(1, "u1", "排 徽章 甲 1", 1_000))
        .await
        .unwrap();

    let before = replay(&log, &cfg, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(before.version, 1);
    assert_eq!(before.board.user_summaries.len(), 1);

    log.replace_all(
        &cfg.round.round_id,
        &[record(1, "u1", "今天天气不错", 1_000)],
    )
    .await
    .unwrap();
    let after = replay(&log, &cfg, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(after.outcomes[0].status, "Ignored");
    assert!(after.board.user_summaries.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

struct DisabledLlm;

#[async_trait::async_trait]
impl crate::llm::client::LlmClient for DisabledLlm {
    async fn complete(
        &self,
        _settings: &crate::settings::LlmSettings,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!("llm disabled in test")
    }
}

fn config_store(cfg: &AppConfig) -> std::sync::Arc<crate::settings::ConfigStore> {
    let path =
        std::env::temp_dir().join(format!("paigu-session-cfg-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
    std::sync::Arc::new(crate::settings::ConfigStore::load(path).unwrap())
}

fn incoming(message_id: &str, text: &str, ts: i64) -> crate::bus::IncomingEvent {
    crate::bus::IncomingEvent {
        group_id: "123456789".to_string(),
        user_id: "u1".to_string(),
        nickname: "u1".to_string(),
        message_id: message_id.to_string(),
        text: text.to_string(),
        timestamp_ms: ts,
        is_admin: false,
        raw: None,
    }
}

fn strip_claim_ids(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("claim_id");
            for child in map.values_mut() {
                strip_claim_ids(child);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items.iter_mut() {
                strip_claim_ids(child);
            }
        }
        _ => {}
    }
}

/// 规范化 board：去掉时间戳与实时/重放各自生成的内部 claim_id，只比较分配结果。
fn canonical_board(mut board: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = board.as_object_mut() {
        obj.remove("generated_at");
    }
    strip_claim_ids(&mut board);
    board
}

#[tokio::test]
async fn realtime_and_replay_modify_boards_match() {
    let cfg = test_config();
    let dir = temp_dir("modify-parity");
    let pipeline = crate::llm::Pipeline::new_with_client_dir(
        config_store(&cfg),
        std::sync::Arc::new(DisabledLlm),
        dir.clone(),
    );

    let first = pipeline
        .process(incoming("mod-1", "排 徽章 甲 1", 1_000))
        .await;
    assert_eq!(first.status, "Applied");

    let modified = pipeline
        .process(incoming("mod-2", "改 徽章 甲 2", 2_000))
        .await;
    assert_eq!(modified.status, "Applied");
    assert!(modified.detail.starts_with("改单"), "{}", modified.detail);

    let (live_version, live_board) = pipeline.board().await;

    let log = MessageLog::new(&dir, dir.join("events"));
    let records = log.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 2);

    let replayed = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(replayed.outcomes[1].status, "Applied");
    assert!(
        replayed.outcomes[1].detail.starts_with("改单"),
        "{}",
        replayed.outcomes[1].detail
    );

    assert_eq!(live_version, replayed.version, "实时与重放版本应一致");
    assert_eq!(
        canonical_board(live_board),
        canonical_board(serde_json::to_value(&replayed.board).unwrap()),
        "实时 = 重放"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// D-02：阶段拒绝在实时（Pipeline）与重放（session）间一致。
#[tokio::test]
async fn realtime_and_replay_phase_rejection_match() {
    let mut cfg = test_config();
    cfg.round.phases = vec![PhaseWindow {
        phase: RoundPhase::Locked,
        start_ms: 0,
        end_ms: i64::MAX,
    }];
    let dir = temp_dir("phase-parity");
    let pipeline = crate::llm::Pipeline::new_with_client_dir(
        config_store(&cfg),
        std::sync::Arc::new(DisabledLlm),
        dir.clone(),
    );

    let live = pipeline
        .process(incoming("ph-1", "排 徽章 甲 1", 1_000))
        .await;
    assert_eq!(live.status, "Rejected");
    assert!(live.detail.contains("阶段"), "{}", live.detail);

    let log = MessageLog::new(&dir, dir.join("events"));
    let records = log.read_all(&cfg.round.round_id).await.unwrap();
    let replayed = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();

    assert_eq!(replayed.outcomes[0].status, "Rejected");
    assert!(replayed.outcomes[0].detail.contains("阶段"));
    assert!(replayed.board.user_summaries.is_empty());
    assert_eq!(live.version, replayed.version, "实时与重放版本应一致");

    let _ = std::fs::remove_dir_all(&dir);
}

/// D-02：成员白名单在网关实时路由与重放判定间一致。
#[tokio::test]
async fn realtime_and_replay_whitelist_decisions_match() {
    use crate::gateway::onebot::{decide_route, RouteKind, RouteMessageEvent, RoutePolicy, Sender};
    use serde_json::json;

    let mut cfg = test_config();
    cfg.gateway.whitelist_members = vec!["u1".to_string()];
    let route_policy = RoutePolicy {
        whitelist_groups: vec!["123456789".to_string()],
        whitelist_members: cfg.gateway.whitelist_members.clone(),
    };

    let route_event = |user_id: &str, nickname: &str| RouteMessageEvent {
        post_type: "message".to_string(),
        message_type: Some("group".to_string()),
        group_id: Some(json!("123456789")),
        user_id: Some(json!(user_id)),
        message_id: Some(json!("m1")),
        message: Some(json!([{ "type": "text", "data": { "text": "排 徽章 甲 1" } }])),
        raw_message: Some("排 徽章 甲 1".to_string()),
        sender: Some(Sender {
            user_id: Some(json!(user_id)),
            nickname: Some(nickname.to_string()),
            card: Some(nickname.to_string()),
            role: None,
        }),
        time: Some(1),
        ..Default::default()
    };

    assert_eq!(
        decide_route(&route_event("u1", "成员一"), &route_policy),
        RouteKind::Message
    );
    assert_eq!(
        decide_route(&route_event("u2", "成员二"), &route_policy),
        RouteKind::Drop
    );

    let records = vec![
        record(1, "u1", "排 徽章 甲 1", 1_000),
        MessageRecord {
            seq: 2,
            group_id: "123456789".to_string(),
            user_id: "u2".to_string(),
            nickname: "成员二".to_string(),
            message_id: "m2".to_string(),
            text: "排 徽章 甲 1".to_string(),
            timestamp_ms: 2_000,
            is_admin: false,
            routed: "drop:not_whitelisted_member".to_string(),
            status: "Dropped".to_string(),
            detail: "not_whitelisted_member".to_string(),
        },
    ];
    let replayed = replay_messages(&cfg, &records, ReplayOverrides::default())
        .await
        .unwrap();

    assert_eq!(replayed.outcomes[0].status, "Applied");
    assert_eq!(replayed.outcomes[1].status, "Dropped");
    assert!(
        replayed
            .board
            .user_summaries
            .iter()
            .all(|s| s.user_id.0 == "u1"),
        "只有白名单内用户应上榜"
    );
}
