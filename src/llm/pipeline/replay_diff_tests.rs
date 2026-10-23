use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming};
use crate::api::ApiState;
use crate::parser::action_override::ActionSelection;
use crate::replay::session::{self, ReplayOverrides};
use crate::services::{configuration, message_actions, replay, rounds, ServiceError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{atomic::Ordering, Arc};

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, values: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, values);
            } else {
                values.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
    }
    let mut values = BTreeMap::new();
    collect(root, &mut values);
    values
}

struct Server {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn http(state: ApiState) -> (Arc<ApiState>, Server) {
    let state = Arc::new(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = crate::api::build_router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (state, Server { base, task })
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap()
}

#[tokio::test]
async fn diff_uses_new_saved_messages_instead_of_the_last_preview() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let old = replay::do_replay(&state, ReplayOverrides::default())
        .await
        .unwrap();
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    let cfg = state.cfg.get().await;
    let fresh = session::replay(&state.messages, &cfg, ReplayOverrides::default())
        .await
        .unwrap();
    assert_ne!(
        fresh.version, old.version,
        "fixture really has a newer saved generation"
    );
    let actual = replay::replay_diff(&state).await.unwrap();
    assert_eq!(
        actual["version"], fresh.version,
        "old preview cannot mask real new input"
    );
    assert_eq!(actual["live_diff"]["changed"], false);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn diff_does_not_borrow_another_instances_same_round_id() {
    let (first, _) = fixture(false, "").await;
    first.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let first_cfg = first.cfg.get().await;
    replay::do_replay(&first, Default::default()).await.unwrap();
    let (second, llm) = fixture(false, "").await;
    let mut cfg = second.cfg.get().await;
    cfg.round.round_id = first_cfg.round.round_id;
    let revision = cfg.revision;
    second.cfg.put(cfg, revision).await.unwrap();
    let actual = replay::replay_diff(&second).await.unwrap();
    assert_eq!(
        actual["version"], 0,
        "a separate empty store cannot inherit the other instance's order"
    );
    assert!(second.pipeline.messages_since(0).await.is_empty());
    assert_eq!(actual["live_available"], false);
    assert_eq!(actual["consistent"], Value::Null);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_http_preview_keeps_its_own_diff_but_does_not_pollute_active_diagnostics_or_write()
{
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let root = Path::new(&cfg.members.cache_path).parent().unwrap();
    let before_files = files(root);
    let before_board = state.pipeline.board().await;
    let before_messages = state.pipeline.messages_since(0).await;
    let (state, server) = http(state).await;
    let response = client()
        .post(format!("{}/api/replay", server.base))
        .json(&json!({"overrides":{"items":[]}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let preview: Value = response.json().await.unwrap();
    assert_eq!(
        preview["diff"]["changed"], true,
        "fixture preview intentionally differs from real catalog"
    );
    for _ in 0..3 {
        let response = client()
            .get(format!("{}/api/replay/diff", server.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let actual: Value = response.json().await.unwrap();
        assert_eq!(actual["round_id"], cfg.round.round_id);
        assert_eq!(actual["revision"], cfg.revision);
        assert_eq!(actual["source"], "active_round");
        assert_eq!(actual["comparison"], "saved_actions_vs_live_allocation");
        assert_eq!(actual["live_available"], true);
        assert_eq!(actual["consistent"], true);
        assert_eq!(actual["message_count"], 1);
        assert_eq!(actual["diff"]["changed"], false);
        assert_eq!(actual["live_diff"]["changed"], false);
        assert!(actual.get("base_version").is_some() && actual.get("live_version").is_some());
    }
    assert_eq!(
        files(root),
        before_files,
        "GET and preview are not a hidden persistence operation"
    );
    assert_eq!(state.pipeline.board().await, before_board);
    assert_eq!(state.pipeline.messages_since(0).await, before_messages);
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn diff_waits_for_the_common_writer_and_uses_the_latest_config_without_mixing_cuts() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let state = Arc::new(state);
    let guard = state.pipeline.message_write_guard().await;
    let reader = state.clone();
    let task = tokio::spawn(async move { replay::replay_diff(&reader).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        !task.is_finished(),
        "diagnostic must wait, not join a cached result with a later live board"
    );
    let mut next = state.cfg.get().await;
    let previous = next.revision;
    next.display.refresh_ms += 1;
    configuration::apply_locked(&state, next, previous)
        .await
        .unwrap();
    let expected = state.cfg.get().await.revision;
    drop(guard);
    let actual = task.await.unwrap();
    assert_eq!(actual["revision"], expected);
    assert_eq!(actual["consistent"], true);
    // A policy edit changes qualification, not the frozen understanding/model output.
    let guard = state.pipeline.message_write_guard().await;
    let mut next = state.cfg.get().await;
    let previous = next.revision;
    next.round.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: 10_000,
    });
    configuration::apply_locked(&state, next, previous)
        .await
        .unwrap();
    drop(guard);
    let actual = replay::replay_diff(&state).await.unwrap();
    assert_eq!(actual["revision"], previous + 1);
    assert_eq!(actual["version"], 0);
    assert_eq!(actual["consistent"], true);
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Rejected"
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn diff_tracks_manual_choice_and_single_undo_at_the_original_message_without_reasking_model()
{
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let before = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    replay::do_replay(&state, Default::default()).await.unwrap();
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "占位管理员解释".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    let actual = replay::replay_diff(&state).await.unwrap();
    assert_eq!(actual["consistent"], true);
    assert_eq!(actual["version"], 0);
    let mut later = incoming("排 徽章 甲 2", 2000);
    later.user_id = "u2".into();
    later.nickname = "成员02".into();
    state.pipeline.process(later).await;
    let status = message_actions::undo_status(&state, &cfg.round.round_id)
        .await
        .unwrap();
    message_actions::undo(
        &state,
        message_actions::UndoInput {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id.clone(),
            config_revision: cfg.revision,
            undo_token: status["undo_token"].as_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    let actual = replay::replay_diff(&state).await.unwrap();
    assert_eq!(actual["consistent"], true);
    assert_eq!(actual["message_count"], 2);
    assert_eq!(actual["version"], 2);
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(records[0].record_id, before[0].record_id);
    assert_eq!(records[0].text, before[0].text);
    assert_eq!(records[0].timestamp_ms, before[0].timestamp_ms);
    assert_eq!(
        serde_json::to_value(&records[0].processing).unwrap(),
        serde_json::to_value(&before[0].processing).unwrap()
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn diagnostics_refuse_corrupt_message_or_event_sources_without_silent_empty_success() {
    for kind in ["messages", "events"] {
        let (state, llm) = fixture(false, "").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        replay::do_replay(&state, Default::default()).await.unwrap();
        let cfg = state.cfg.get().await;
        let path = if kind == "messages" {
            Path::new(&cfg.members.cache_path)
                .parent()
                .unwrap()
                .join("messages")
                .join(format!("{}.jsonl", cfg.round.round_id))
        } else {
            state
                .messages
                .ingress_dir()
                .parent()
                .unwrap()
                .join(format!("{}.jsonl", cfg.round.round_id))
        };
        use std::io::Write;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .unwrap()
            .write_all(b"corrupt fixture line\n")
            .unwrap();
        let before = state.pipeline.board().await;
        assert!(replay::replay_diff(&state).await.is_err());
        assert!(replay::do_replay(&state, Default::default()).await.is_err());
        assert_eq!(state.pipeline.board().await, before);
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn diagnostics_reject_pending_intake_and_cross_round_live_state_instead_of_claiming_consistency(
) {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let pending = state
        .messages
        .ingress_dir()
        .join(&cfg.round.round_id)
        .join("00000000000000000001.json");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    std::fs::write(&pending, b"pending fixture").unwrap();
    assert!(
        matches!(replay::replay_diff(&state).await, Err(ServiceError::Conflict(ref body)) if body["error"]=="pending_ingress")
    );
    std::fs::remove_file(pending).unwrap();
    let mut next = cfg.clone();
    next.round.round_id = format!("diff_{}", uuid::Uuid::new_v4().simple());
    state.cfg.put(next, cfg.revision).await.unwrap();
    assert!(
        matches!(replay::replay_diff(&state).await, Err(ServiceError::Conflict(ref body)) if body["error"]=="replay_context_changed")
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn round_switch_and_uninitialized_live_state_never_restore_or_substitute_old_projections() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    crate::settings::rounds::write_round(&cfg.round).unwrap();
    let second = format!("diff_{}", uuid::Uuid::new_v4().simple());
    rounds::create_round(
        &state,
        rounds::CreateRoundBody {
            round_id: second.clone(),
            title: None,
            copy_from: None,
            category: None,
        },
    )
    .await
    .unwrap();
    replay::do_replay(&state, Default::default()).await.unwrap();
    rounds::activate_round(
        &state,
        &second,
        rounds::ActivateBody {
            mode: Some("fresh".into()),
        },
    )
    .await
    .unwrap();
    let empty = replay::replay_diff(&state).await.unwrap();
    assert_eq!(empty["round_id"], second);
    assert_eq!(empty["message_count"], 0);
    assert_eq!(empty["live_available"], false);
    assert_eq!(empty["consistent"], Value::Null);
    rounds::activate_round(
        &state,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("continue".into()),
        },
    )
    .await
    .unwrap();
    let actual = replay::replay_diff(&state).await.unwrap();
    assert_eq!(actual["round_id"], cfg.round.round_id);
    assert_eq!(actual["consistent"], true);
    let saved = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    state.pipeline.reset().await;
    let before = state.pipeline.board().await;
    let unavailable = replay::replay_diff(&state).await.unwrap();
    assert_eq!(unavailable["message_count"], 1);
    assert_eq!(unavailable["live_available"], false);
    assert_eq!(unavailable["consistent"], Value::Null);
    assert_eq!(
        state.pipeline.board().await,
        before,
        "GET is not a replay apply or recovery shortcut"
    );
    assert_eq!(
        serde_json::to_value(state.messages.read_only(&cfg.round.round_id).await.unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
