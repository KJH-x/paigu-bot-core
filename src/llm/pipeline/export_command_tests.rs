use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming};
use crate::parser::action_override::ActionSelection;
use crate::services::message_actions;
use crate::snapshot_bundle::SnapshotBundle;
use serde_json::json;
use std::sync::atomic::Ordering;

fn exports(state: &crate::api::ApiState, round: &str) -> Vec<std::path::PathBuf> {
    let root = state.messages.snapshot_dir();
    if !root.exists() {
        return vec![];
    }
    let mut files: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(round)
        })
        .map(|path| path.join(format!("{round}.snapshot.json")))
        .collect();
    files.sort();
    files
}

async fn business_board(state: &crate::api::ApiState) -> serde_json::Value {
    let mut board = state.pipeline.board().await.1;
    // A receipt retry rebuilds with event time rather than wall-clock publication.
    board.as_object_mut().unwrap().remove("generated_at");
    board
}

fn exported_path(reply: &str) -> std::path::PathBuf {
    reply
        .lines()
        .next()
        .unwrap()
        .strip_prefix("已导出快照：")
        .unwrap()
        .into()
}

#[tokio::test]
async fn control_export_keeps_complete_draft_human_audit_and_exact_raw_integer() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    state
        .messages
        .append_raw_event(
            &cfg.round.round_id,
            &json!({"opaque_id":9007199254740993_u64}),
        )
        .await
        .unwrap();
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "占位管理员解释".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    let before = state.pipeline.board().await;
    let guard = state.pipeline.message_write_guard().await;
    let (status, _, reply) = state.pipeline.export_snapshot().await;
    drop(guard);
    assert_eq!(status, "Applied");
    let path = exported_path(&reply);
    assert!(path.starts_with(state.messages.snapshot_dir()));
    let bundle = SnapshotBundle::import_file(&path).unwrap();
    assert_eq!(bundle.settlement["draft"]["round_id"], cfg.round.round_id);
    assert!(bundle
        .events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["opaque_id"].as_u64() == Some(9007199254740993)));
    assert!(bundle
        .events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["payload"]["event_type"] == "ParseOverride"));
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn control_export_refuses_bad_message_or_event_lines_instead_of_writing_a_successful_empty_bundle(
) {
    for kind in ["messages", "events"] {
        let (state, llm) = fixture(false, "").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let cfg = state.cfg.get().await;
        let path = if kind == "messages" {
            std::path::Path::new(&cfg.members.cache_path)
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
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"corrupt fixture line\n").unwrap();
        let guard = state.pipeline.message_write_guard().await;
        let (status, _, _) = state.pipeline.export_snapshot().await;
        drop(guard);
        assert_eq!(status, "Error");
        assert!(exports(&state, &cfg.round.round_id).is_empty());
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn default_control_and_path_exports_do_not_overwrite_the_previous_file() {
    use crate::services::snapshot::{snapshot_export, ExportBody};
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let guard = state.pipeline.message_write_guard().await;
    let (_, _, reply) = state.pipeline.export_snapshot().await;
    drop(guard);
    let first = exported_path(&reply);
    let original = std::fs::read(&first).unwrap();
    let second = snapshot_export(
        &state,
        ExportBody {
            format: Some("file".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let second = std::path::PathBuf::from(second["path"].as_str().unwrap());
    assert_ne!(first, second);
    assert!(first.is_file() && second.is_file());
    assert_eq!(std::fs::read(first).unwrap(), original);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn control_and_idle_exports_refuse_pending_records_and_import_does_not_execute_unfinished_input(
) {
    use crate::services::snapshot::{self, transfer, ExportBody, ImportBody};
    use crate::services::ServiceError;
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let ready = snapshot::snapshot_export(
        &state,
        ExportBody {
            format: Some("file".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut bundle = SnapshotBundle::import_file(ready["path"].as_str().unwrap()).unwrap();
    bundle.messages[0]["status"] = json!("Pending");
    bundle.manifest["hash"] = json!(bundle.recompute_hash().unwrap());
    for apply in [false, true] {
        let error = snapshot::snapshot_import(
            &state,
            ImportBody {
                bundle: Some(bundle.clone()),
                apply,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, ServiceError::Conflict(ref body) if body["error"]=="pending_saved_message")
        );
    }
    let mut pending = state
        .messages
        .read_only(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    pending.record_id = uuid::Uuid::new_v4().to_string();
    pending.message_id = uuid::Uuid::new_v4().to_string();
    pending.status = "Pending".into();
    pending.processing = None;
    pending.seq += 1;
    state
        .messages
        .append(&cfg.round.round_id, &pending)
        .await
        .unwrap();
    let before = exports(&state, &cfg.round.round_id);
    let guard = state.pipeline.message_write_guard().await;
    assert_eq!(state.pipeline.export_snapshot().await.0, "Error");
    drop(guard);
    assert!(
        matches!(snapshot::snapshot_export(&state,Default::default()).await,Err(ServiceError::Conflict(ref body)) if body["error"]=="pending_saved_message")
    );
    assert!(
        matches!(transfer::download(&state,transfer::DownloadBody{round_id:cfg.round.round_id.clone(),config_revision:cfg.revision,private_ack:true}).await,Err(ServiceError::Conflict(ref body)) if body["error"]=="pending_saved_message")
    );
    assert_eq!(
        exports(&state, &cfg.round.round_id),
        before,
        "no incomplete or empty fallback artifact"
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn control_keeps_the_pre_receipt_cut_and_durable_retry_never_reexports_or_acknowledges_the_frame(
) {
    use crate::bus::{Delivery, EventSink, IngressBinding, IngressSource};
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let pending = state
        .messages
        .ingress_dir()
        .join(&cfg.round.round_id)
        .join("00000000000000000001.json");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    std::fs::write(&pending, b"owned pending-frame fixture").unwrap();
    let mut command = incoming("/导出", 2000);
    command.is_admin = true;
    command.ingress = Some(IngressSource {
        id: uuid::Uuid::new_v4().to_string(),
        part: "frame".into(),
    });
    let before = business_board(&state).await;
    assert_eq!(
        state
            .pipeline
            .deliver(&binding, command.clone())
            .await
            .unwrap(),
        Delivery::Committed
    );
    let files = exports(&state, &cfg.round.round_id);
    assert_eq!(files.len(), 1);
    let bundle = SnapshotBundle::import_file(&files[0]).unwrap();
    assert_eq!(
        bundle.messages.as_array().unwrap().len(),
        1,
        "this control receipt is saved after its export cut"
    );
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].text, "/导出");
    assert_eq!(records[1].status, "Applied");
    assert!(records[1].detail.contains(files[0].to_str().unwrap()));
    assert_eq!(
        state.pipeline.deliver(&binding, command).await.unwrap(),
        Delivery::Committed
    );
    assert_eq!(exports(&state, &cfg.round.round_id), files);
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(business_board(&state).await, before);
    assert_eq!(
        std::fs::read(pending).unwrap(),
        b"owned pending-frame fixture",
        "only original intake worker may acknowledge it"
    );
    assert!(
        matches!(crate::services::snapshot::snapshot_export(&state,Default::default()).await,Err(crate::services::ServiceError::Conflict(ref body)) if body["error"]=="pending_ingress")
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admin_and_enabled_switch_are_still_required_but_read_only_export_does_not_unlock_a_locked_round(
) {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = false;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let mut command = incoming("/导出", 2000);
    command.is_admin = true;
    let recorded = state.pipeline.process(command).await;
    assert_eq!(recorded.status, "Applied");
    assert!(
        recorded.detail.contains("执行开关关闭"),
        "the existing contract records a disabled control without executing it"
    );
    assert!(exports(&state, &cfg.round.round_id).is_empty());
    let mut next = cfg.clone();
    next.gateway.admin_commands_enabled = true;
    state.cfg.put(next, cfg.revision).await.unwrap();
    assert_eq!(
        state.pipeline.process(incoming("/导出", 3000)).await.status,
        "Rejected"
    );
    assert!(exports(&state, &cfg.round.round_id).is_empty());
    state.pipeline.set_locked(true).await.unwrap();
    let before = business_board(&state).await;
    let mut command = incoming("/export", 4000);
    command.is_admin = true;
    let result = state.pipeline.process(command.clone()).await;
    assert_eq!(result.status, "Applied");
    let path = exported_path(result.reply.as_deref().unwrap());
    assert!(path.is_file());
    assert!(state.pipeline.is_locked().await);
    assert_eq!(business_board(&state).await, before);
    assert_eq!(state.pipeline.process(command).await.status, "Duplicate");
    assert_eq!(exports(&state, &cfg.round.round_id).len(), 1);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
