use super::automatic_tests::{fixture, incoming};
use super::*;
use crate::bus::{Delivery, IngressBinding, IngressSource};
use std::sync::atomic::Ordering;

const CLAIM: &str = r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":2,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;

fn stamped(text: &str, timestamp: i64) -> IncomingEvent {
    let mut event = incoming(text, timestamp);
    event.ingress = Some(IngressSource {
        id: uuid::Uuid::new_v4().to_string(),
        part: "frame".into(),
    });
    event
}

async fn board(pipeline: &Pipeline) -> (i64, Value) {
    let (version, mut value) = pipeline.board().await;
    // Live publication uses wall time; deterministic replay uses event time.
    // Compare every business cell, claim identity, policy and total unchanged.
    if let Some(object) = value.as_object_mut() {
        object.remove("generated_at");
    }
    (version, value)
}

#[tokio::test]
async fn committed_receipt_survives_restart_but_upstream_duplicate_keeps_own_row() {
    let (state, llm) = fixture(true, CLAIM).await;
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let event = stamped("请帮忙安排两份", 1000);
    assert_eq!(
        state
            .pipeline
            .deliver(&binding, event.clone())
            .await
            .unwrap(),
        Delivery::Committed
    );
    let original = board(&state.pipeline).await;
    state.pipeline.reset_locked().await;
    state
        .pipeline
        .deliver(&binding, event.clone())
        .await
        .unwrap();
    assert_eq!(board(&state.pipeline).await, original);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        1
    );
    let mut duplicate = event;
    duplicate.ingress.as_mut().unwrap().id = uuid::Uuid::new_v4().to_string();
    state.pipeline.deliver(&binding, duplicate).await.unwrap();
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].status, "Duplicate");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_append_does_not_turn_uncommitted_input_into_duplicate() {
    struct BlockAppendLlm {
        path: std::path::PathBuf,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl LlmClient for BlockAppendLlm {
        async fn complete(
            &self,
            _: &crate::settings::LlmSettings,
            _: &str,
            _: &str,
        ) -> anyhow::Result<String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                std::fs::create_dir_all(&self.path)?;
            }
            Ok(CLAIM.into())
        }
    }
    let (state, _) = fixture(true, CLAIM).await;
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let root = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap();
    let blocked = root
        .join("messages")
        .join(format!("{}.jsonl", cfg.round.round_id));
    let llm = Arc::new(BlockAppendLlm {
        path: blocked.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let pipeline =
        Pipeline::new_with_client_dir(state.cfg.clone(), llm.clone(), root.join("messages"));
    let event = stamped("请安排两份", 1000);
    assert!(pipeline.deliver(&binding, event.clone()).await.is_err());
    std::fs::remove_dir(&blocked).unwrap();
    pipeline.deliver(&binding, event).await.unwrap();
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, "Applied");
    // The failed result was never saved. An explicit retry can call the model,
    // but transient seen state must not convert this operation to Duplicate.
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn batch_recovery_reuses_committed_children_and_preserves_timestamp_order() {
    let (state, llm) = fixture(true, CLAIM).await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.round.category = crate::round::RoundCategory::Test;
    cfg.gateway.test_group_id = Some(cfg.round.group_id.clone());
    cfg.gateway.accept_forged_messages = true;
    state.cfg.put(cfg, revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let event = stamped(
        r#"[{"id":"u2","text":"请安排两份","timestamp":2000},{"id":"u1","text":"请安排两份","timestamp":1000}]"#,
        3000,
    );
    let child = gate_nc_multi(&cfg, event.clone()).remove(0);
    state.pipeline.deliver(&binding, child).await.unwrap();
    state.pipeline.reset_locked().await;
    state
        .pipeline
        .deliver(&binding, event.clone())
        .await
        .unwrap();
    state.pipeline.deliver(&binding, event).await.unwrap();
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        (records[0].timestamp_ms, records[1].timestamp_ms),
        (1000, 2000)
    );
    assert_eq!(records[0].ingress.as_ref().unwrap().part, "frame:nc-0");
    assert_eq!(records[1].ingress.as_ref().unwrap().part, "frame:nc-1");
}

#[tokio::test]
async fn proxy_children_keep_distinct_receipts_after_restart() {
    let (state, _llm) = fixture(false, CLAIM).await;
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let event = stamped("排 徽章 甲 1 代成员02排:徽章 甲 1", 1000);
    assert!(proxy::segments(&cfg, &event).is_some());
    state
        .pipeline
        .deliver(&binding, event.clone())
        .await
        .unwrap();
    let before = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(before.len(), 2);
    assert_ne!(before[0].ingress, before[1].ingress);
    let original = board(&state.pipeline).await;
    state.pipeline.reset_locked().await;
    state.pipeline.deliver(&binding, event).await.unwrap();
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(board(&state.pipeline).await, original);
}

#[tokio::test]
async fn merged_member_recovers_saved_representative_without_another_model_call() {
    let (state, llm) = fixture(true, CLAIM).await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg, revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    state
        .pipeline
        .deliver(&binding, stamped("排 徽章 甲 1", 2000))
        .await
        .unwrap();
    let earlier = stamped("排 徽章 甲 1", 1000);
    state
        .pipeline
        .deliver(&binding, earlier.clone())
        .await
        .unwrap();
    let original = state.pipeline.board().await;
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    let member = records
        .iter()
        .find(|r| r.ingress == earlier.ingress)
        .unwrap();
    assert_eq!(member.status, "Merged");
    state
        .messages
        .update(&cfg.round.round_id, &member.record_id, |r| {
            r.status = "Pending".into();
            r.detail.clear();
            r.processing = Some(Default::default());
        })
        .await
        .unwrap();
    state.pipeline.reset_locked().await;
    state.pipeline.deliver(&binding, earlier).await.unwrap();
    assert_eq!(
        llm.calls.load(Ordering::SeqCst),
        2,
        "saved group artifact must not be reinterpreted"
    );
    assert_eq!(state.pipeline.board().await, original);
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn binding_mismatch_defers_without_log_or_model_side_effects() {
    let (state, llm) = fixture(true, CLAIM).await;
    let cfg = state.cfg.get().await;
    let mut binding = IngressBinding::from_config(&cfg);
    binding.category = crate::round::RoundCategory::Test;
    assert_eq!(
        state
            .pipeline
            .deliver(&binding, stamped("请安排两份", 1000))
            .await
            .unwrap(),
        Delivery::Deferred
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    assert!(state
        .messages
        .read_only(&cfg.round.round_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn pending_without_a_saved_understanding_resumes_same_record_not_duplicate() {
    for merge in [false, true] {
        let (state, llm) = fixture(true, CLAIM).await;
        let cfg = state.cfg.get().await;
        let binding = IngressBinding::from_config(&cfg);
        let event = stamped("排 徽章 甲 1", 1000);
        let context = state.pipeline.processing_context(&cfg, &event).await;
        state
            .messages
            .append(&cfg.round.round_id, &context.record)
            .await
            .unwrap();
        let mut next = cfg.clone();
        next.merge.enabled = merge;
        next.merge.same_sender = true;
        state.cfg.put(next, cfg.revision).await.unwrap();
        state.pipeline.reset_locked().await;
        state.pipeline.deliver(&binding, event).await.unwrap();
        let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record_id, context.record.record_id);
        assert_eq!(records[0].status, "Applied");
        assert_eq!(llm.calls.load(Ordering::SeqCst), usize::from(merge));
    }
}

#[tokio::test]
async fn pending_intake_fences_export_restore_fresh_delete_and_identity_changes() {
    use crate::services::{configuration, rounds, snapshot, ServiceError};
    fn guarded(result: Result<Value, ServiceError>) {
        assert!(
            matches!(result,Err(ServiceError::Conflict(ref body)) if body["error"]=="pending_ingress"),
            "operation must refuse pending intake: {result:?}"
        );
    }
    let (state, _llm) = fixture(false, "{}").await;
    let cfg = state.cfg.get().await;
    let root = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap();
    let path = root.join("pending-backup.json");
    snapshot::snapshot_export(
        &state,
        snapshot::ExportBody {
            format: Some("file".into()),
            out_dir: Some(path.display().to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let preview = snapshot::snapshot_import(
        &state,
        snapshot::ImportBody {
            path: Some(path.display().to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let pending = state
        .messages
        .ingress_dir()
        .join(&cfg.round.round_id)
        .join("00000000000000000001.json");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    std::fs::write(&pending, b"not yet processed").unwrap();
    guarded(snapshot::snapshot_export(&state, Default::default()).await);
    guarded(
        snapshot::snapshot_import(
            &state,
            snapshot::ImportBody {
                path: Some(path.display().to_string()),
                apply: true,
                revision: Some(cfg.revision),
                round_id: Some(cfg.round.round_id.clone()),
                preflight_token: preview["preflight_token"].as_str().map(Into::into),
                ..Default::default()
            },
        )
        .await,
    );
    guarded(
        rounds::activate_round(
            &state,
            &cfg.round.round_id,
            rounds::ActivateBody {
                mode: Some("fresh".into()),
            },
        )
        .await,
    );
    let mut next = cfg.clone();
    next.round.group_id = "987654321".into();
    next.gateway
        .whitelist_groups
        .push(next.round.group_id.clone());
    {
        let _writer = state.pipeline.message_write_guard().await;
        guarded(configuration::apply_locked(&state, next, cfg.revision).await);
    }
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    let mut inactive = cfg.round.clone();
    inactive.round_id = format!("pending_{}", uuid::Uuid::new_v4().simple());
    let inactive_path = crate::settings::rounds::write_round(&inactive).unwrap();
    let other = state
        .messages
        .ingress_dir()
        .join(&inactive.round_id)
        .join("unknown.data");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, b"preserve me").unwrap();
    guarded(rounds::delete_round(&state, &inactive.round_id).await);
    assert!(inactive_path.exists());
    assert_eq!(std::fs::read(&pending).unwrap(), b"not yet processed");
}

#[tokio::test]
async fn damaged_history_does_not_silently_reinterpret_pending_intake() {
    let (state, llm) = fixture(true, CLAIM).await;
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let path = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap()
        .join("messages")
        .join(format!("{}.jsonl", cfg.round.round_id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"broken historical input\n").unwrap();
    assert!(state
        .pipeline
        .deliver(&binding, stamped("请安排两份", 1000))
        .await
        .is_err());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(path).unwrap(), b"broken historical input\n");
}
