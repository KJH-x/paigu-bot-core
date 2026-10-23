use super::automatic_tests::{fixture, incoming};
use crate::replay::{computed, session};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

async fn same_as_full(state: &crate::api::ApiState) -> session::ReplayResult {
    let cfg = state.cfg.get().await;
    let expected = session::replay(&state.messages, &cfg, Default::default())
        .await
        .unwrap();
    assert_eq!(
        state.pipeline.board().await.1,
        serde_json::to_value(&expected.board).unwrap()
    );
    let rows = state.pipeline.messages_since(0).await;
    let expected_rows: Vec<_>=expected.messages.iter().filter(|r|r.status!="Dropped").map(|m|serde_json::json!({
        "original_automatic":m.original_automatic,"manual_note":m.manual_note,
        "record_id":m.record_id,"record_revision":m.record_revision,"round_id":expected.round_id,
        "seq":m.seq,"user_id":m.user_id,"delegation":m.delegation,"display":m.nickname,"text":m.text,"status":m.status,"detail":m.detail,
    })).collect();
    assert_eq!(
        rows, expected_rows,
        "全部消息字段/原产物/说明仍同源，不只比较认购数量"
    );
    let _guard = state.pipeline.message_write_guard().await;
    let (cached, hit) = computed::replay(&state.messages, &cfg, computed::ReadPolicy::Prefer)
        .await
        .unwrap();
    assert!(hit, "实际完成后检查点命中，不能先用校验函数补生成");
    assert_eq!(
        serde_json::to_value(&cached).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    expected
}

#[tokio::test]
async fn a_legacy_duplicate_without_an_automatic_artifact_cannot_turn_into_a_second_order() {
    let (state, _) = fixture(false, "").await;
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let mut duplicate = state
        .messages
        .read_only(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    duplicate.record_id = "legacy-duplicate-placeholder".into();
    duplicate.seq += 1;
    duplicate.timestamp_ms += 1;
    duplicate.nickname = "重复污染称呼".into();
    duplicate.processing = None;
    duplicate.status = "Duplicate".into();
    duplicate.detail = "旧幂等结果".into();
    state
        .messages
        .append(&cfg.round.round_id, &duplicate)
        .await
        .unwrap();
    let full = state.pipeline.recompute().await.unwrap();
    assert_eq!(
        full.events.len(),
        1,
        "旧Duplicate不得因兼容规则解析而变成新认购"
    );
    assert_eq!(full.messages[1].status, "Duplicate");
    assert_eq!(full.board.user_summaries[0].display_name, "成员01");
}

#[tokio::test]
async fn ordinary_success_has_an_actual_complete_checkpoint_without_explicit_recompute() {
    let (state, _) = fixture(false, "").await;
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 1", 1000))
            .await
            .status,
        "Applied"
    );
    let cfg = state.cfg.get().await;
    assert!(
        state
            .messages
            .computed_path(&cfg.round.round_id)
            .unwrap()
            .is_file(),
        "普通提交仍未接统一检查点"
    );
}

#[tokio::test]
async fn authorized_control_keeps_its_original_automatic_outcome_after_replay() {
    let (state, _) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let mut event = incoming("/状态", 1000);
    event.is_admin = true;
    let outcome = state.pipeline.ingest(event).await;
    assert_eq!(outcome.status, "Applied");
    let cfg = state.cfg.get().await;
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert!(
        records[0].processing.is_some(),
        "控制命令缺少完整原自动产物"
    );
    let full = state.pipeline.recompute().await.unwrap();
    assert_eq!(full.messages[0].detail, outcome.detail);
    assert!(full.messages[0].original_automatic.is_some());
}

#[tokio::test]
async fn every_ordinary_terminal_and_action_branch_publishes_one_new_group_and_full_source_checkpoint(
) {
    let (state, llm) = fixture(false, "").await;
    let first = incoming("排 徽章 甲 1", 1000);
    let mut duplicate = first.clone();
    duplicate.timestamp_ms = 4000;
    duplicate.nickname = "重复不改名".into();
    let mut dropped = incoming("排 徽章 甲 1", 7000);
    dropped.group_id = "987654321".into();
    let mut private = incoming("排 徽章 甲 1", 8000);
    private.group_id.clear();
    let cases = vec![
        (first, "Applied"),
        (incoming("占位闲聊", 2000), "Ignored"),
        (incoming("撤 徽章 甲 1", 3000), "Applied"),
        (duplicate, "Duplicate"),
        (incoming("/状态", 5000), "Rejected"),
        (incoming("", 6000), "Ignored"),
        (dropped, "Dropped"),
        (private, "Dropped"),
    ];
    for (index, (event, status)) in cases.into_iter().enumerate() {
        let outcome = state.pipeline.ingest(event).await;
        assert_eq!(outcome.status, status, "branch {index}: {}", outcome.detail);
        let work = state.pipeline.state.lock().await.memo.work;
        assert_eq!(work.evaluated_groups, 1);
        assert_eq!(work.reused_groups, index);
        let full = same_as_full(&state).await;
        assert_eq!(full.messages.len(), index + 1);
        assert!(full.messages.iter().all(|r| r.original_automatic.is_some()));
    }
    let cfg = state.cfg.get().await;
    assert_eq!(
        state
            .messages
            .read_only(&cfg.round.round_id)
            .await
            .unwrap()
            .len(),
        8
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn controls_are_frozen_and_saved_retry_does_not_repeat_export_or_revive_locked_input() {
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    for (index, text) in ["/状态", "/锁位"].into_iter().enumerate() {
        let mut command = incoming(text, 2000 + index as i64 * 1000);
        command.is_admin = true;
        assert_eq!(state.pipeline.ingest(command).await.status, "Applied");
        same_as_full(&state).await;
    }
    assert!(state.pipeline.is_locked().await);
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 2", 4000))
            .await
            .status,
        "Rejected"
    );
    let mut unlock = incoming("/解锁", 5000);
    unlock.is_admin = true;
    assert_eq!(state.pipeline.ingest(unlock).await.status, "Applied");
    let full = same_as_full(&state).await;
    assert_eq!(
        full.messages[3].status, "Rejected",
        "解锁不能复活当时锁后输入"
    );
    let mut export = incoming("/导出", 6000);
    export.is_admin = true;
    export.ingress = Some(crate::bus::IngressSource {
        id: "control-receipt-placeholder".into(),
        part: "0".into(),
    });
    let outcome = state.pipeline.ingest(export.clone()).await;
    assert_eq!(outcome.status, "Applied", "{}", outcome.detail);
    let path = std::path::PathBuf::from(
        outcome
            .reply
            .as_ref()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .strip_prefix("已导出快照：")
            .unwrap(),
    );
    let bundle = crate::snapshot_bundle::SnapshotBundle::import_file(&path).unwrap();
    assert_eq!(
        bundle.messages.as_array().unwrap().len(),
        5,
        "导出仍是本控制保存前切面"
    );
    let before_source = state
        .messages
        .read_only(&state.cfg.get().await.round.round_id)
        .await
        .unwrap();
    assert_eq!(
        state.pipeline.ingest(export.clone()).await.status,
        "AlreadyDelivered"
    );
    assert_eq!(
        state
            .messages
            .read_only(&state.cfg.get().await.round.round_id)
            .await
            .unwrap()
            .len(),
        before_source.len()
    );
    export.ingress.as_mut().unwrap().id = "upstream-duplicate-placeholder".into();
    export.timestamp_ms = 7000;
    assert_eq!(state.pipeline.ingest(export).await.status, "Duplicate");
    let cfg = state.cfg.get().await;
    let prefix = format!("{}-", cfg.round.round_id);
    let files = std::fs::read_dir(path.parent().unwrap().parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .count();
    assert_eq!(files, 1, "同收据/上游重复不得执行第二次导出");
    same_as_full(&state).await;
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

struct FailPublication {
    path: std::path::PathBuf,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl super::LlmClient for FailPublication {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::fs::create_dir_all(self.path.parent().unwrap())?;
        std::fs::write(&self.path, b"{bad-private-lock}")?;
        Ok(r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":2,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#.into())
    }
}

#[tokio::test]
async fn a_committed_understanding_survives_publication_failure_and_recovers_without_another_model_call(
) {
    for stamped in [false, true] {
        let (state, _) = fixture(true, "").await;
        let cfg = state.cfg.get().await;
        let good = serde_json::to_vec(
            &crate::services::settlement::checkpoint(&cfg.round.round_id).unwrap(),
        )
        .unwrap();
        let root = std::path::PathBuf::from(std::env::var_os("PAIGU_SETTLEMENT_DIR").unwrap());
        assert!(root
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with("paigu-check-")));
        let path = root.join(format!("{}.json", cfg.round.round_id));
        let model = Arc::new(FailPublication {
            path: path.clone(),
            calls: AtomicUsize::new(0),
        });
        let pipeline = super::Pipeline::new_with_messages(
            state.cfg.clone(),
            model.clone(),
            state.messages.clone(),
        );
        let mut event = incoming("请帮我安排两份", 1000);
        if stamped {
            event.ingress = Some(crate::bus::IngressSource {
                id: "committed-recovery-placeholder".into(),
                part: "0".into(),
            });
        }
        let original = pipeline.board().await;
        let failed = pipeline.ingest(event.clone()).await;
        assert_eq!(failed.status, "Error");
        assert!(failed.detail.contains("已保存"));
        assert_eq!(pipeline.board().await, original, "失败不能发布部分新板");
        let rows = state.messages.read_only(&cfg.round.round_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "Applied");
        let original_auto = serde_json::to_value(&rows[0].processing).unwrap();
        assert_eq!(pipeline.ingest(event.clone()).await.status, "Error");
        assert_eq!(
            model.calls.load(Ordering::SeqCst),
            1,
            "源未恢复前不能再次理解已提交消息"
        );
        std::fs::write(path, good).unwrap();
        assert_eq!(
            pipeline.ingest(event).await.status,
            if stamped {
                "AlreadyDelivered"
            } else {
                "Duplicate"
            }
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
        let rows = state.messages.read_only(&cfg.round.round_id).await.unwrap();
        assert_eq!(rows.len(), if stamped { 1 } else { 2 });
        assert_eq!(
            serde_json::to_value(&rows[0].processing).unwrap(),
            original_auto
        );
        assert_eq!(
            pipeline.board().await.1["user_summaries"][0]["items"][0]["quantity"],
            2
        );
    }
}

#[tokio::test]
async fn cache_write_failure_after_an_ordinary_commit_is_not_a_business_failure() {
    let (state, llm) = fixture(false, "").await;
    let cfg = state.cfg.get().await;
    let path = state.messages.computed_path(&cfg.round.round_id).unwrap();
    assert!(path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with("paigu-check-")));
    std::fs::create_dir_all(&path).unwrap();
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 1", 1000))
            .await
            .status,
        "Applied"
    );
    assert_eq!(
        state.pipeline.board().await.1["user_summaries"][0]["items"][0]["quantity"],
        1
    );
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 1", 2000))
            .await
            .status,
        "Applied"
    );
    same_as_full(&state).await;
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn damaged_persistent_lock_cannot_be_treated_as_unlocked_before_a_control_side_effect() {
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.set_locked(true).await.unwrap();
    let cfg = state.cfg.get().await;
    let good =
        serde_json::to_vec(&crate::services::settlement::checkpoint(&cfg.round.round_id).unwrap())
            .unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("PAIGU_SETTLEMENT_DIR").unwrap());
    assert!(root
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with("paigu-check-")));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(format!("{}.json", cfg.round.round_id));
    std::fs::write(&path, b"{bad-lock}").unwrap();
    let before_board = state.pipeline.board().await;
    let before_events = state
        .messages
        .read_events_strict(&cfg.round.round_id)
        .await
        .unwrap();
    let mut unlock = incoming("/解锁", 2000);
    unlock.is_admin = true;
    assert_eq!(state.pipeline.ingest(unlock.clone()).await.status, "Error");
    assert!(state.pipeline.is_locked().await);
    assert_eq!(state.pipeline.board().await, before_board);
    assert_eq!(
        state
            .messages
            .read_events_strict(&cfg.round.round_id)
            .await
            .unwrap(),
        before_events,
        "不能先清阶段再发现持久锁坏了"
    );
    std::fs::write(path, good).unwrap();
    assert_eq!(state.pipeline.ingest(unlock).await.status, "Applied");
    assert!(!state.pipeline.is_locked().await);
    same_as_full(&state).await;
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn actor_and_group_whitelists_are_checked_before_an_admin_command_side_effect() {
    for blocked_group in [false, true] {
        let (state, llm) = fixture(false, "").await;
        let mut cfg = state.cfg.get().await;
        cfg.gateway.admin_commands_enabled = true;
        if !blocked_group {
            cfg.gateway.whitelist_members = vec!["allowed-placeholder".into()];
        }
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        let cfg = state.cfg.get().await;
        let mut event = incoming("/锁位", 1000);
        event.is_admin = true;
        if blocked_group {
            event.group_id = "987654321".into();
        }
        let outcome = state.pipeline.ingest(event).await;
        assert_eq!(outcome.status, "Dropped");
        assert!(
            !state.pipeline.is_locked().await,
            "不允许先执行副作用再由重放白名单丢弃记录"
        );
        assert!(!state
            .messages
            .domain_events(&cfg.round.round_id)
            .await
            .unwrap()
            .iter()
            .any(|e| matches!(
                e.payload,
                crate::domain::event::DomainEvent::RoundPhaseSet(_)
            )));
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}
