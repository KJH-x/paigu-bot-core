use super::automatic_tests::{fixture, incoming};
use crate::replay::{
    computed::{self, ReadPolicy},
    session::{self, ReplayOverrides},
};
use std::sync::atomic::Ordering;

async fn preferred(
    state: &crate::api::ApiState,
    cfg: &crate::settings::AppConfig,
) -> (session::ReplayResult, bool) {
    let _guard = state.pipeline.message_write_guard().await;
    computed::replay(&state.messages, cfg, ReadPolicy::Prefer)
        .await
        .unwrap()
}

fn no_times(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("generated_at");
            for child in map.values_mut() {
                no_times(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                no_times(child);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn authoritative_recompute_creates_a_private_derived_checkpoint() {
    let (state, _) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    state.pipeline.recompute().await.unwrap();
    assert!(
        state
            .messages
            .computed_path(&cfg.round.round_id)
            .unwrap()
            .is_file(),
        "缺少可重建的私有派生检查点"
    );
}

#[tokio::test]
async fn cache_hit_restores_internal_names_eligibilities_steps_and_later_cancel_without_a_model() {
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_users = vec!["u1".into()];
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.process(incoming("排 徽章 甲 2", 1000)).await;
    let cfg = state.cfg.get().await;
    let original = state.pipeline.recompute().await.unwrap();
    assert!(!original.eligibilities.is_empty());
    assert!(!original.display_names.is_empty());
    let (hit, cached) = preferred(&state, &cfg).await;
    assert!(cached, "必须实际读缓存，不以相等结果冒充命中");
    assert_eq!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(&hit).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&original.eligibilities).unwrap(),
        serde_json::to_value(&hit.eligibilities).unwrap()
    );
    assert_eq!(original.display_names, hit.display_names);
    let before_source = state
        .messages
        .read_ingress_records(&cfg.round.round_id)
        .await
        .unwrap();
    let new =
        super::Pipeline::new_with_messages(state.cfg.clone(), llm.clone(), state.messages.clone());
    let recovered = new.restore().await.unwrap();
    assert_eq!(
        serde_json::to_value(&recovered.eligibilities).unwrap(),
        serde_json::to_value(&original.eligibilities).unwrap()
    );
    assert_eq!(new.board().await, state.pipeline.board().await);
    let mut old_view = state.pipeline.replay_view().await.unwrap();
    let mut new_view = new.replay_view().await.unwrap();
    no_times(&mut old_view);
    no_times(&mut new_view);
    assert_eq!(old_view, new_view);
    assert_eq!(
        serde_json::to_value(before_source).unwrap(),
        serde_json::to_value(
            state
                .messages
                .read_ingress_records(&cfg.round.round_id)
                .await
                .unwrap()
        )
        .unwrap()
    );
    let outcome = new.process(incoming("撤 徽章 甲 1", 2000)).await;
    assert_eq!(outcome.status, "Applied");
    let before = new.board().await.1;
    let full = new.recompute().await.unwrap();
    let mut before = before;
    let mut after = serde_json::to_value(full.board).unwrap();
    no_times(&mut before);
    no_times(&mut after);
    assert_eq!(before, after);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn semantic_changes_miss_but_observed_revision_is_rebased_without_stale_metadata() {
    let (state, _) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    state.pipeline.recompute().await.unwrap();
    let mut next = cfg.clone();
    next.revision += 9;
    next.active_round_id = None;
    let (rebased, hit) = preferred(&state, &next).await;
    assert!(hit);
    assert_eq!(rebased.revision, next.revision);
    for mutation in 0..8 {
        let mut next = cfg.clone();
        match mutation {
            0 => next.round.items[0].subitems[0].capacity = Some(3),
            1 => next.round.items[0].subitems[0].unit_price_cents += 10,
            2 => next.gateway.whitelist_members = vec!["other".into()],
            3 => next.round.priority_users.push("u1".into()),
            4 => next.members.cn_overrides.push(crate::settings::CnOverride {
                user_id: "u1".into(),
                cn: "成员改名".into(),
                aliases: vec!["别称".into()],
            }),
            5 => next.merge.enabled = !next.merge.enabled,
            6 => next.round.items[0].aliases.push("目录别称".into()),
            _ => next.round.title.push_str("副本"),
        }
        // Warm the original key each time, then change only one semantic input.
        {
            let _guard = state.pipeline.message_write_guard().await;
            computed::replay(&state.messages, &cfg, ReadPolicy::Force)
                .await
                .unwrap();
        }
        let (actual, hit) = preferred(&state, &next).await;
        assert!(!hit, "semantic mutation {mutation} must miss");
        let reference = session::replay(&state.messages, &next, ReplayOverrides::default())
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(reference).unwrap()
        );
    }
}

#[tokio::test]
async fn older_same_seq_record_and_same_mtime_member_content_invalidate_the_checkpoint() {
    let (state, llm) = fixture(false, "").await;
    let cfg = state.cfg.get().await;
    let mut cfg = cfg;
    cfg.members.group_id = cfg.round.group_id.clone();
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let names = std::path::Path::new(&cfg.members.cache_path);
    std::fs::write(names, r#"[{"user_id":"u1","card":"成员01"}]"#).unwrap();
    let stamp = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);
    let retime = || {
        std::fs::File::options()
            .write(true)
            .open(names)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp))
            .unwrap()
    };
    retime();
    let mut message = incoming("排 徽章 甲 1", 1000);
    message.nickname = message.user_id.clone();
    state.pipeline.process(message).await;
    state.pipeline.recompute().await.unwrap();
    assert!(preferred(&state, &cfg).await.1);
    std::fs::write(names, r#"[{"user_id":"u1","card":"成员02"}]"#).unwrap();
    retime();
    let (changed, hit) = preferred(&state, &cfg).await;
    assert!(!hit);
    assert_eq!(changed.messages[0].nickname, "成员02");
    assert_eq!(
        crate::settings::member_display_name(&cfg, "u1").as_deref(),
        Some("成员02")
    );
    let before = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    let cache_path = state.messages.computed_path(&cfg.round.round_id).unwrap();
    let cache_before = std::fs::read(&cache_path).unwrap();
    state
        .messages
        .update(&cfg.round.round_id, &before[0].record_id, |row| {
            row.record_revision += 1;
            row.nickname = "新原始称呼".into();
        })
        .await
        .unwrap();
    {
        let _guard = state.pipeline.message_write_guard().await;
        computed::checkpoint(&state.messages, &cfg, &changed, &before).await;
    }
    assert_eq!(
        std::fs::read(cache_path).unwrap(),
        cache_before,
        "准备后来源变化不可被检查点盖章为当前"
    );
    let (changed, hit) = preferred(&state, &cfg).await;
    assert!(!hit);
    assert_eq!(changed.messages[0].seq, before[0].seq);
    assert_eq!(
        changed.messages[0].record_revision,
        before[0].record_revision + 1
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn manual_replacement_undo_and_phase_events_replace_the_cache_without_losing_originals() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let original = state.pipeline.recompute().await.unwrap();
    let old_file =
        std::fs::read(state.messages.computed_path(&cfg.round.round_id).unwrap()).unwrap();
    let (id, mut body) = super::action_tests::opened(&state).await;
    body.selection = crate::parser::action_override::ActionSelection::Ignore {
        note: "占位解释".into(),
    };
    crate::services::message_actions::save(&state, &id, body)
        .await
        .unwrap();
    let (modified, hit) = preferred(&state, &cfg).await;
    assert!(hit);
    assert_eq!(modified.messages[0].status, "Ignored");
    assert_eq!(modified.messages[0].seq, original.messages[0].seq);
    assert!(modified.messages[0].original_automatic.is_some());
    assert_eq!(modified.messages[0].text, original.messages[0].text);
    assert_ne!(
        std::fs::read(state.messages.computed_path(&cfg.round.round_id).unwrap()).unwrap(),
        old_file
    );
    let undo = crate::services::message_actions::undo_status(&state, &cfg.round.round_id)
        .await
        .unwrap();
    assert_eq!(undo["available"], true);
    crate::services::message_actions::undo(
        &state,
        crate::services::message_actions::UndoInput {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id.clone(),
            config_revision: cfg.revision,
            undo_token: undo["undo_token"].as_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    let (restored, hit) = preferred(&state, &cfg).await;
    assert!(hit);
    assert_eq!(restored.messages[0].status, "Applied");
    assert_eq!(
        serde_json::to_value(&restored.messages[0].original_automatic).unwrap(),
        serde_json::to_value(&original.messages[0].original_automatic).unwrap()
    );
    state.pipeline.set_locked(true).await.unwrap();
    let (locked, hit) = preferred(&state, &cfg).await;
    assert!(!hit);
    assert!(locked.locked);
    state.pipeline.set_locked(false).await.unwrap();
    let (unlocked, hit) = preferred(&state, &cfg).await;
    assert!(!hit);
    assert!(!unlocked.locked);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn corrupt_or_foreign_cache_falls_back_but_corrupt_source_fails_before_a_warm_hit() {
    let (state, _) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let expected = state.pipeline.recompute().await.unwrap();
    let path = state.messages.computed_path(&cfg.round.round_id).unwrap();
    let clean = std::fs::read(&path).unwrap();
    for field in ["format", "build", "context", "integrity", "payload"] {
        let mut value: serde_json::Value = serde_json::from_slice(&clean).unwrap();
        value[field] = serde_json::json!("invalid");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let (rebuilt, hit) = preferred(&state, &cfg).await;
        assert!(!hit, "corrupted field {field}");
        assert_eq!(
            serde_json::to_value(&rebuilt).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert!(preferred(&state, &cfg).await.1);
    }
    std::fs::write(&path, b"{torn").unwrap();
    assert!(!preferred(&state, &cfg).await.1);
    state
        .messages
        .append_raw_event(
            &cfg.round.round_id,
            &serde_json::json!({"event_type":"invalid_domain_row"}),
        )
        .await
        .unwrap();
    let _guard = state.pipeline.message_write_guard().await;
    assert!(
        computed::replay(&state.messages, &cfg, ReadPolicy::Prefer)
            .await
            .is_err(),
        "缓存不能掩盖损坏权威事件"
    );
}

#[tokio::test]
async fn pending_input_does_not_overwrite_cache_and_cache_write_failure_does_not_fail_business() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    state.pipeline.recompute().await.unwrap();
    let path = state.messages.computed_path(&cfg.round.round_id).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let records = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    state
        .messages
        .update(&cfg.round.round_id, &records[0].record_id, |r| {
            r.status = "Pending".into()
        })
        .await
        .unwrap();
    assert!(!preferred(&state, &cfg).await.1);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    state
        .messages
        .update(&cfg.round.round_id, &records[0].record_id, |r| {
            r.status = "Applied".into()
        })
        .await
        .unwrap();
    assert!(path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name.starts_with("paigu-check-") || name.starts_with("paigu-auto-test-")
    }));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let rebuilt = state.pipeline.recompute().await.unwrap();
    assert_eq!(rebuilt.messages[0].status, "Applied");
    assert!(state.pipeline.board().await.1.is_object());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    std::fs::remove_dir(&path).unwrap();
    assert!(!preferred(&state, &cfg).await.1);
    assert!(preferred(&state, &cfg).await.1);
    for invalid in ["", "../a", "a/b", " a", "a ", "a.b"] {
        assert!(state.messages.computed_path(invalid).is_err());
    }
}

#[tokio::test]
async fn cold_continue_recovers_this_round_but_fresh_parked_state_and_explicit_replay_remain_distinct(
) {
    use crate::services::rounds;
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_users = vec!["u1".into()];
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let expected = state.pipeline.recompute().await.unwrap();
    let cfg = state.cfg.get().await;
    let other = format!("cache_{}", uuid::Uuid::new_v4().simple());
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
    rounds::activate_round(
        &state,
        &other,
        rounds::ActivateBody {
            mode: Some("fresh".into()),
        },
    )
    .await
    .unwrap();
    let pipeline =
        super::Pipeline::new_with_messages(state.cfg.clone(), llm.clone(), state.messages.clone());
    let recovered = crate::api::ApiState {
        cfg: state.cfg.clone(),
        messages: state.messages.clone(),
        gateway: crate::gateway::Gateway::new(state.cfg.clone(), pipeline.clone()),
        pipeline,
        members_pull: Default::default(),
    };
    rounds::activate_round(
        &recovered,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("continue".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        recovered.pipeline.board().await.1,
        serde_json::to_value(&expected.board).unwrap()
    );
    let observed = recovered.cfg.get().await;
    let (hit_result, hit) = preferred(&recovered, &observed).await;
    assert!(hit);
    assert_eq!(hit_result.revision, observed.revision);
    assert_eq!(
        serde_json::to_value(&hit_result.eligibilities).unwrap(),
        serde_json::to_value(&expected.eligibilities).unwrap()
    );
    rounds::activate_round(
        &recovered,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("fresh".into()),
        },
    )
    .await
    .unwrap();
    assert!(recovered.pipeline.board().await.1.is_null());
    assert!(!state
        .messages
        .computed_path(&cfg.round.round_id)
        .unwrap()
        .exists());
    // An explicitly fresh in-memory view must not be replaced by a disk recovery on continue.
    for id in [&other, &cfg.round.round_id] {
        rounds::activate_round(
            &recovered,
            id,
            rounds::ActivateBody {
                mode: Some("continue".into()),
            },
        )
        .await
        .unwrap();
    }
    assert!(recovered.pipeline.board().await.1.is_null());
    let cfg = recovered.cfg.get().await;
    let (_, hit) = preferred(&recovered, &cfg).await;
    assert!(!hit);
    let (warm, hit) = preferred(&recovered, &cfg).await;
    assert!(hit);
    rounds::activate_round(
        &recovered,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("replay".into()),
        },
    )
    .await
    .unwrap();
    let cfg = recovered.cfg.get().await;
    let (forced, hit) = preferred(&recovered, &cfg).await;
    assert!(hit);
    assert_ne!(
        warm.eligibilities[0].eligibility_id, forced.eligibilities[0].eligibility_id,
        "显式replay不可读旧派生物"
    );
    assert_eq!(
        recovered.pipeline.board().await.1["user_summaries"][0]["items"][0]["quantity"],
        1
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn warm_cache_cannot_mask_bad_message_lines_or_clear_a_persistent_settlement_lock() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let original = state.pipeline.recompute().await.unwrap();
    assert!(!original.locked);
    assert!(preferred(&state, &cfg).await.1);
    let settlement_root = std::env::var_os("PAIGU_SETTLEMENT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap();
    assert!(settlement_root
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with("paigu-check-")));
    let mut draft = crate::services::settlement::checkpoint(&cfg.round.round_id).unwrap();
    draft["locked"] = serde_json::json!(true);
    std::fs::create_dir_all(&settlement_root).unwrap();
    std::fs::write(
        settlement_root.join(format!("{}.json", cfg.round.round_id)),
        serde_json::to_vec(&draft).unwrap(),
    )
    .unwrap();
    let pipeline =
        super::Pipeline::new_with_messages(state.cfg.clone(), llm.clone(), state.messages.clone());
    pipeline.restore().await.unwrap();
    let recovered = crate::api::ApiState {
        cfg: state.cfg.clone(),
        messages: state.messages.clone(),
        gateway: crate::gateway::Gateway::new(state.cfg.clone(), pipeline.clone()),
        pipeline,
        members_pull: Default::default(),
    };
    crate::services::settlement::restore_persistent_lock(&recovered)
        .await
        .unwrap();
    assert!(recovered.pipeline.is_locked().await);
    assert_eq!(
        recovered
            .pipeline
            .process(incoming("排 徽章 甲 1", 2000))
            .await
            .status,
        "Rejected"
    );
    let source = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap()
        .join("messages")
        .join(format!("{}.jsonl", cfg.round.round_id));
    let before = recovered.pipeline.board().await;
    let mut raw = std::fs::read(&source).unwrap();
    raw.extend_from_slice(b"{bad-message}\n");
    std::fs::write(source, raw).unwrap();
    assert!(recovered.pipeline.restore().await.is_err());
    assert_eq!(
        recovered.pipeline.board().await,
        before,
        "恢复失败不发布空板/旧缓存"
    );
    assert!(recovered.pipeline.is_locked().await);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
