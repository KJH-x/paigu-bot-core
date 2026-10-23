use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::services::{configuration, message_actions, rounds};
use std::sync::{atomic::Ordering, Arc};

#[tokio::test]
async fn inactive_round_traffic_cannot_postpone_the_active_round_reload_until_it_goes_quiet() {
    let (state, _) = fixture(false, "").await;
    let state = Arc::new(state);
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let weak = Arc::downgrade(&state);
    state.cfg.spawn_watch(move || {
        let weak = weak.clone();
        async move {
            if let Some(state) = weak.upgrade() {
                configuration::reload(&state, false).await.unwrap();
            }
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let mut inactive = cfg.round.clone();
    inactive.round_id = format!("inactive_{}", uuid::Uuid::new_v4().simple());
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = running.clone();
    let traffic = std::thread::spawn(move || {
        for _ in 0..150 {
            if !flag.load(Ordering::SeqCst) {
                break;
            }
            crate::settings::rounds::write_round(&inactive).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    let mut active = cfg.round.clone();
    active.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: 10_000,
    });
    crate::settings::rounds::write_round(&active).unwrap();
    let updated = tokio::time::timeout(std::time::Duration::from_millis(1500), async {
        loop {
            if state.pipeline.messages_since(0).await[0]["status"] == "Rejected" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await;
    running.store(false, Ordering::SeqCst);
    traffic.join().unwrap();
    assert!(updated.is_ok(), "活跃轮变更不能等待无关轮次连续IO停止");
}

#[tokio::test]
async fn configuration_prepares_before_commit_and_policy_reload_never_calls_model() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let mut next = cfg.clone();
    next.round.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: 10000,
    });
    {
        let _serial = state.pipeline.message_write_guard().await;
        configuration::apply_locked(&state, next, cfg.revision)
            .await
            .unwrap();
    }
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Rejected"
    );
    let mut file = crate::settings::rounds::read_round(&cfg.round.round_id).unwrap();
    file.priority_users.push("u1".into());
    crate::settings::rounds::write_round(&file).unwrap();
    configuration::reload(&state, false).await.unwrap();
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Applied"
    );
    let revision = state.cfg.get().await.revision;
    configuration::reload(&state, false).await.unwrap();
    assert_eq!(
        state.cfg.get().await.revision,
        revision,
        "own writes do not cause revision loop"
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn actual_file_watch_uses_runtime_and_recomputes_external_priority_change() {
    let (state, llm) = fixture(false, "{}").await;
    let state = Arc::new(state);
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let weak = Arc::downgrade(&state);
    state.cfg.spawn_watch(move || {
        let weak = weak.clone();
        async move {
            if let Some(s) = weak.upgrade() {
                configuration::reload(&s, false).await.unwrap();
            }
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let mut file = cfg.round.clone();
    file.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: 10000,
    });
    crate::settings::rounds::write_round(&file).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if state.pipeline.messages_since(0).await[0]["status"] == "Rejected" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("actual watcher publishes policy change");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn locking_and_manual_save_are_serial_and_durable_lock_failures_never_claim_success() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, body) = opened(&state).await;
    let (locked, saved) = tokio::join!(
        state.pipeline.set_locked(true),
        message_actions::save(&state, &id, body)
    );
    locked.unwrap();
    assert!(state.pipeline.is_locked().await);
    if let Err(error) = saved {
        assert!(matches!(error, crate::services::ServiceError::Locked(_)));
    }
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    state.messages.inject_durable_fault(1);
    assert!(state.pipeline.set_locked(false).await.is_err());
    assert!(
        state.pipeline.is_locked().await,
        "failed unlock stays locked"
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn continue_round_switches_keep_own_cached_state_and_never_previous_round_data() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let other = format!("other_{}", uuid::Uuid::new_v4().simple());
    rounds::create_round(
        &state,
        rounds::CreateRoundBody {
            round_id: other.clone(),
            title: Some("另一占位轮次".into()),
            copy_from: Some(cfg.round.round_id.clone()),
            category: None,
        },
    )
    .await
    .unwrap();
    rounds::activate_round(
        &state,
        &other,
        rounds::ActivateBody {
            mode: Some("continue".into()),
        },
    )
    .await
    .unwrap();
    assert!(state.pipeline.messages_since(0).await.is_empty());
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    rounds::activate_round(
        &state,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("continue".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
    assert_eq!(
        state.pipeline.board().await.1["round_id"],
        cfg.round.round_id
    );
    assert_eq!(
        state.pipeline.board().await.1["user_summaries"][0]["items"][0]["quantity"],
        1
    );
}

#[tokio::test]
async fn broken_target_replay_does_not_activate_or_clear_previous_round() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let before = state.pipeline.board().await;
    let cfg = state.cfg.get().await;
    let other = format!("broken_{}", uuid::Uuid::new_v4().simple());
    rounds::create_round(
        &state,
        rounds::CreateRoundBody {
            round_id: other.clone(),
            title: None,
            copy_from: Some(cfg.round.round_id.clone()),
            category: None,
        },
    )
    .await
    .unwrap();
    state
        .messages
        .append_durable_event(&other, &serde_json::json!({"event_type":"invalid"}))
        .await
        .unwrap();
    assert!(rounds::activate_round(
        &state,
        &other,
        rounds::ActivateBody {
            mode: Some("replay".into())
        }
    )
    .await
    .is_err());
    assert_eq!(state.cfg.get().await.round.round_id, cfg.round.round_id);
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
