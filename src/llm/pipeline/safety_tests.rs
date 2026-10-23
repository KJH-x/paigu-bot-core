use super::automatic_tests::{fixture, incoming};
use super::snapshot_download_tests::server;
use crate::round::RoundCategory;
use crate::settings::rounds;
use serde_json::json;
use std::sync::atomic::Ordering;

fn create_body(id: &str, title: &str) -> crate::services::rounds::CreateRoundBody {
    crate::services::rounds::CreateRoundBody {
        round_id: id.into(),
        title: Some(title.into()),
        copy_from: None,
        category: None,
    }
}

#[tokio::test]
async fn safety_round_create_cannot_overwrite_a_damaged_or_case_aliased_existing_file() {
    let (state, _) = fixture(false, "").await;
    let id = format!("protected_{}", uuid::Uuid::new_v4().simple());
    let path = rounds::round_path(&id).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = b"damaged source to retain for manual recovery";
    std::fs::write(&path, original).unwrap();
    assert!(
        crate::services::rounds::create_round(&state, create_body(&id, "不得覆盖"))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    #[cfg(windows)]
    {
        assert!(crate::services::rounds::create_round(
            &state,
            create_body(&id.to_ascii_uppercase(), "不得换大小写覆盖")
        )
        .await
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn safety_round_create_and_delete_wait_for_shared_writer_and_recheck_active_context() {
    use crate::services::rounds::{create_round, delete_round};
    use std::time::Duration;
    let (state, _) = fixture(false, "").await;
    let state = std::sync::Arc::new(state);
    let id = format!("serialized_{}", uuid::Uuid::new_v4().simple());
    let guard = state.pipeline.message_write_guard().await;
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        create_round(&state, create_body(&id, "等待"))
    )
    .await
    .is_err());
    assert!(!rounds::round_path(&id).unwrap().exists());
    drop(guard);
    let (first, second) = tokio::join!(
        create_round(&state, create_body(&id, "一")),
        create_round(&state, create_body(&id, "二")),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let round = rounds::read_round(&id).unwrap();
    assert_eq!(round.title, if first.is_ok() { "一" } else { "二" });
    let path = rounds::round_path(&id).unwrap();
    let original = std::fs::read(&path).unwrap();
    let guard = state.pipeline.message_write_guard().await;
    let target = id.clone();
    let shared = state.clone();
    let mut deletion = tokio::spawn(async move { delete_round(&shared, &target).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut deletion)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let mut cfg = state.cfg.get().await;
    cfg.round = round;
    cfg.active_round_id = Some(id.clone());
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    drop(guard);
    assert!(matches!(
        deletion.await.unwrap(),
        Err(crate::services::ServiceError::BadRequest(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn safety_category_binding_is_rejected_by_each_real_mutation_entry_without_publishing() {
    let (state, model) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.test_group_id = Some("987654321".into());
    cfg.gateway.whitelist_groups.push("987654321".into());
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let initial_config = serde_json::to_value(&cfg).unwrap();
    let board = state.pipeline.board().await;
    let round_path = rounds::round_path(&cfg.round.round_id).unwrap();
    let before = std::fs::read(&round_path).unwrap();
    let (state, server) = server(state).await;
    let client = reqwest::Client::new();
    for (category, group, code) in [
        (
            RoundCategory::Test,
            "123456789",
            "test_round_group_mismatch",
        ),
        (
            RoundCategory::Normal,
            "987654321",
            "normal_round_uses_test_group",
        ),
    ] {
        let mut bad = cfg.clone();
        bad.round.category = category;
        bad.round.group_id = group.into();
        let response = client
            .put(format!("{}/api/config", server.base))
            .json(&json!({"config":bad,"revision":cfg.revision}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 400);
        let response = client
            .post(format!(
                "{}/api/rounds/{}/setup",
                server.base, cfg.round.round_id
            ))
            .json(&json!({"category":category,"group_id":group}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 422);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["ok"], false);
        assert_eq!(body["written"], false);
        assert!(body["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == code));
        let target = format!("bad_binding_{}", uuid::Uuid::new_v4().simple());
        bad.round.round_id = target.clone();
        let mut initial = cfg.round.clone();
        initial.round_id = target.clone();
        let path = rounds::write_round(&initial).unwrap();
        let inactive_before = std::fs::read(&path).unwrap();
        for dry_run in [true, false] {
            let response = client
                .post(format!("{}/api/rounds/{target}/setup", server.base))
                .json(&json!({"category":category,"group_id":group,"dry_run":dry_run}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), if dry_run { 200 } else { 422 });
            let body: serde_json::Value = response.json().await.unwrap();
            assert_eq!(body["ok"], false);
            assert_eq!(body["written"], false);
            assert!(body["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == code));
            assert_eq!(std::fs::read(&path).unwrap(), inactive_before);
        }
        rounds::write_round(&bad.round).unwrap();
        for mode in ["continue", "fresh", "replay"] {
            let response = client
                .post(format!("{}/api/rounds/{target}/activate", server.base))
                .json(&json!({"mode":mode}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 422);
        }
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            serde_json::to_value(state.cfg.get().await).unwrap(),
            initial_config
        );
        assert_eq!(state.pipeline.board().await, board);
        assert_eq!(std::fs::read(&round_path).unwrap(), before);
    }
    for path in ["/api/sim/message", "/api/sim/messages"] {
        let body = if path.ends_with("messages") {
            json!([])
        } else {
            json!({"user_id":"u1","text":"排 徽章 甲 2","timestamp":2000})
        };
        let response = client
            .post(format!("{}{path}", server.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 400);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"],
            "not_a_test_round"
        );
    }
    assert_eq!(state.pipeline.board().await, board);
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn safety_inbound_raw_frame_cannot_impersonate_an_internal_phase_event() {
    use crate::domain::event::{DomainEvent, EventEnvelope, EventStatus, RoundPhaseSet};
    use crate::domain::ids::{EventId, RoundId, UserId};
    for include_onebot_fields in [false, true] {
        let (state, model) = fixture(false, "").await;
        let cfg = state.cfg.get().await;
        let mut source = serde_json::to_value(EventEnvelope {
            event_id: EventId(uuid::Uuid::new_v4().to_string()),
            round_id: RoundId(cfg.round.round_id.clone()),
            group_id: cfg.round.group_id.clone(),
            user_id: UserId("u1".into()),
            raw_message_id: None,
            event_type: "round_phase_set".into(),
            effective_at: chrono::Utc::now(),
            sequence: 99,
            payload: DomainEvent::RoundPhaseSet(RoundPhaseSet {
                phase: Some(crate::round::RoundPhase::Locked),
                set_by: UserId("u1".into()),
            }),
            status: EventStatus::Active,
        })
        .unwrap();
        if include_onebot_fields {
            source["post_type"] = json!("message");
            source["message_type"] = json!("group");
        }
        let mut event = incoming("排 徽章 甲 1", 1000);
        event.raw = Some(source.clone());
        assert_eq!(state.pipeline.ingest(event).await.status, "Applied");
        assert!(!state.pipeline.is_locked().await);
        assert!(state
            .messages
            .domain_events(&cfg.round.round_id)
            .await
            .unwrap()
            .is_empty());
        assert!(
            crate::replay::session::load_manual_phase(&state.messages, &cfg.round.round_id)
                .await
                .is_none()
        );
        assert_eq!(
            state
                .messages
                .read_raw_events(&cfg.round.round_id, 0)
                .await
                .unwrap(),
            vec![source]
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn safety_ordinary_http_body_limit_rejects_whole_input_before_any_business_side_effect() {
    let (state, model) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let cfg = state.cfg.get().await;
    let before = state.pipeline.board().await;
    const LIMIT: usize = 2 * 1024 * 1024;
    let exact = format!("{{\"padding\":\"{}\"}}", "x".repeat(LIMIT - 14));
    assert_eq!(exact.len(), LIMIT);
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/sim/message", server.base))
        .header("Content-Type", "application/json")
        .body(exact)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status().as_u16(),
        400,
        "exact bound reaches normal-round gate"
    );
    for (method, path) in [
        (reqwest::Method::POST, "/api/sim/message"),
        (reqwest::Method::POST, "/api/sim/messages"),
        (reqwest::Method::PUT, "/api/config"),
        (reqwest::Method::POST, "/api/messages"),
    ] {
        let response = client
            .request(method, format!("{}{path}", server.base))
            .header("Content-Type", "application/json")
            .body(vec![b' '; LIMIT + 1])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 413, "{path}");
    }
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    assert!(state
        .messages
        .read_only(&cfg.round.round_id)
        .await
        .unwrap()
        .is_empty());
    assert!(state
        .messages
        .read_events_strict(&cfg.round.round_id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
