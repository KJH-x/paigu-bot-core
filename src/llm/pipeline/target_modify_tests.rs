use super::automatic_tests::{fixture, incoming, quantity};
use crate::api::ApiState;
use crate::bus::{Delivery, EventSink, IngressBinding, IngressSource};
use crate::domain::event::DomainEvent;
use crate::settings::{CnOverride, PriorityWindow};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

async fn setup(state: &ApiState) {
    let mut cfg = state.cfg.get().await;
    cfg.members.group_id = cfg.round.group_id.clone();
    cfg.gateway.admin_commands_enabled = true;
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"u2","card":"成员乙"},{"user_id":"u3","card":"成员丙"}]"#,
    )
    .unwrap();
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
}

async fn send(
    state: &ApiState,
    uid: &str,
    text: &str,
    admin: bool,
    ts: i64,
) -> crate::bus::PipelineOutcome {
    let mut event = incoming(text, ts);
    event.user_id = uid.into();
    event.is_admin = admin;
    state.pipeline.ingest(event).await
}

async fn records(state: &ApiState) -> Vec<crate::messages::MessageRecord> {
    state
        .messages
        .read_only(&state.cfg.get().await.round.round_id)
        .await
        .unwrap()
}

async fn parity(state: &ApiState) {
    let before = state.pipeline.board().await.1;
    let replay = state.pipeline.recompute().await.unwrap();
    let canonical = |mut value: Value| {
        if let Some(object) = value.as_object_mut() {
            object.remove("generated_at");
        }
        value
    };
    if before.is_null() {
        assert_eq!(quantity(&replay.board), 0);
        assert!(replay.events.is_empty());
    } else {
        assert_eq!(
            canonical(before),
            canonical(serde_json::to_value(replay.board).unwrap())
        );
    }
}

#[tokio::test]
async fn admin_target_modify_replaces_only_the_selected_members_order_at_the_original_position() {
    let (state, _) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.members.group_id = cfg.round.group_id.clone();
    cfg.gateway.admin_commands_enabled = true;
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"u2","card":"成员乙"}]"#,
    )
    .unwrap();
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let mut claim = incoming("排 徽章 甲 1", 1000);
    claim.user_id = "u2".into();
    claim.nickname = "成员乙".into();
    assert_eq!(state.pipeline.ingest(claim).await.status, "Applied");
    let mut command = incoming("/改单 成员乙 徽章 甲 3", 2000);
    command.is_admin = true;
    let out = state.pipeline.ingest(command).await;
    assert_eq!(out.status, "Applied", "{}", out.detail);
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 3);
    assert!(result.events.iter().all(|event| event.user_id.0 == "u2"));
    let records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].text, "/改单 成员乙 徽章 甲 3");
    assert_eq!(records[1].timestamp_ms, 2000);
    assert_eq!(records[1].user_id, "u2");
}

#[tokio::test]
async fn target_modify_preserves_actor_and_other_members_orders_and_real_command_source() {
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    for (i, uid) in ["u1", "u2", "u3"].into_iter().enumerate() {
        assert_eq!(
            send(&state, uid, "排 徽章 甲 1", false, 1000 + i as i64)
                .await
                .status,
            "Applied"
        );
    }
    let original = records(&state).await;
    assert_eq!(
        send(&state, "u1", "/改单 成员乙 徽章 甲 3", true, 2000)
            .await
            .status,
        "Applied"
    );
    parity(&state).await;
    let result = state.pipeline.recompute().await.unwrap();
    let quantities: std::collections::BTreeMap<_, _> = result
        .board
        .user_summaries
        .iter()
        .map(|summary| {
            (
                summary.user_id.0.as_str(),
                summary.items.iter().map(|line| line.quantity).sum::<u32>(),
            )
        })
        .collect();
    assert_eq!(
        quantities,
        std::collections::BTreeMap::from([("u1", 1), ("u2", 3), ("u3", 1)])
    );
    let rows = records(&state).await;
    assert_eq!(rows.len(), 4);
    for i in 0..3 {
        assert_eq!(
            serde_json::to_value(&rows[i]).unwrap(),
            serde_json::to_value(&original[i]).unwrap()
        );
    }
    let command = &rows[3];
    let meta = command.delegation.as_ref().unwrap();
    assert_eq!(meta.actor_user_id, "u1");
    assert_eq!(meta.source_message_id, command.message_id);
    assert_eq!(meta.source_text, command.text);
    assert_eq!(meta.modify_body(), Some("徽章 甲 3"));
    assert_eq!(
        command.processing.as_ref().unwrap().results[0]
            .members
            .len(),
        1
    );
    let detail =
        crate::services::message_actions::read(&state, &command.record_id, &result.round_id)
            .await
            .unwrap();
    assert_eq!(
        detail["message"]["delegation_context"]["qualification"],
        "member"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_admin_and_disabled_command_never_execute_even_after_switch_is_enabled() {
    for admin in [false, true] {
        let (state, model) = fixture(true, "").await;
        setup(&state).await;
        assert_eq!(
            send(&state, "u2", "排 徽章 甲 1", false, 1000).await.status,
            "Applied"
        );
        let mut cfg = state.cfg.get().await;
        cfg.gateway.admin_commands_enabled = false;
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        let out = send(&state, "u1", "/改单 成员乙 徽章 甲 3", admin, 2000).await;
        assert_eq!(out.status, if admin { "Applied" } else { "Rejected" });
        let row = records(&state).await.remove(1);
        assert!(row.delegation.is_none());
        assert!(row.processing.unwrap().results[0]
            .understanding
            .events
            .is_empty());
        let mut cfg = state.cfg.get().await;
        cfg.gateway.admin_commands_enabled = true;
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn explicit_target_lookup_and_argument_errors_do_not_call_model_or_create_guests() {
    for (cache, command, status) in [
        (
            json!([{"user_id":"u2","card":"同名"},{"user_id":"u3","card":"同名"}]),
            "/改单 同名 徽章 甲 3",
            "NeedConfirm",
        ),
        (
            json!([{"user_id":"u2","card":"成员长名"}]),
            "/改单 成员 徽章 甲 3",
            "NeedConfirm",
        ),
        (json!([]), "/改单 群外 徽章 甲 3", "NeedConfirm"),
        (
            json!({"group_id":"987654321","members":[{"user_id":"u2","card":"成员乙"}]}),
            "/改单 成员乙 徽章 甲 3",
            "NeedConfirm",
        ),
        (
            json!({"members":"broken"}),
            "/改单 成员乙 徽章 甲 3",
            "NeedConfirm",
        ),
        (json!([]), "/改单", "NeedConfirm"),
        (json!([]), "/改单 @u2", "NeedConfirm"),
        (json!([]), "/改单 @u2 /开团", "Rejected"),
        (json!([]), "／改单@u2 徽章 甲 3", "Rejected"),
    ] {
        let (state, model) = fixture(true, "").await;
        setup(&state).await;
        let cfg = state.cfg.get().await;
        std::fs::write(&cfg.members.cache_path, serde_json::to_vec(&cache).unwrap()).unwrap();
        let out = send(&state, "u1", command, true, 1000).await;
        assert_eq!(out.status, status, "{command}: {}", out.detail);
        let row = records(&state).await.remove(0);
        assert_eq!(row.user_id, "u1");
        assert_eq!(row.text, command);
        assert!(row.delegation.is_none());
        parity(&state).await;
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn exact_id_cn_alias_and_fullwidth_commands_have_the_same_member_owner() {
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"1","card":"MemberBeta"}]"#,
    )
    .unwrap();
    cfg.members.cn_overrides = vec![CnOverride {
        user_id: "1".into(),
        cn: "称呼乙".into(),
        aliases: vec!["别名乙".into()],
    }];
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for (i, command) in [
        "/改单 1 徽章 甲 3",
        "/改单 @1 徽章 甲 3",
        "/改单 称呼乙 徽章 甲 3",
        "/改单 别名乙 徽章 甲 3",
        "／改单 ＭｅｍｂｅｒＢｅｔａ 徽章 甲 ３",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            send(&state, "u1", command, true, 1000 + i as i64)
                .await
                .status,
            "Applied",
            "{command}"
        );
        let rows = records(&state).await;
        let row = rows.last().unwrap();
        assert_eq!(row.user_id, "1");
        assert_eq!(row.text, command);
        parity(&state).await;
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            3
        );
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn target_modify_priority_and_phase_use_recipient_and_do_not_borrow_admin_eligibility() {
    for phase in [false, true] {
        for allowed in [false, true] {
            let (state, model) = fixture(false, "").await;
            setup(&state).await;
            send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
            let mut cfg = state.cfg.get().await;
            cfg.round.priority_users = vec![if allowed { "u2" } else { "u1" }.into()];
            if phase {
                cfg.round.items[0].class = None;
                cfg.round.phases = vec![crate::round::PhaseWindow {
                    phase: crate::round::RoundPhase::PhaseII,
                    start_ms: 2000,
                    end_ms: 10000,
                }];
            } else {
                cfg.round.priority_window = Some(PriorityWindow {
                    start_ms: 2000,
                    end_ms: 10000,
                });
            }
            state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
            state.pipeline.recompute().await.unwrap();
            assert_eq!(
                send(&state, "u1", "/改单 成员乙 徽章 甲 3", true, 3000)
                    .await
                    .status,
                if allowed { "Applied" } else { "Rejected" }
            );
            parity(&state).await;
            assert_eq!(
                quantity(&state.pipeline.recompute().await.unwrap().board),
                if allowed { 3 } else { 1 }
            );
            assert_eq!(model.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn target_modify_preflight_enforces_actual_actor_and_active_group_before_model() {
    for wrong_group in [false, true] {
        let (state, model) = fixture(true, "").await;
        setup(&state).await;
        let mut cfg = state.cfg.get().await;
        cfg.gateway.whitelist_members = if wrong_group {
            vec![]
        } else {
            vec!["u2".into()]
        };
        cfg.gateway.whitelist_groups.push("987654321".into());
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        let mut event = incoming("/改单 成员乙 帮我重新安排", 1000);
        event.is_admin = true;
        if wrong_group {
            event.group_id = "987654321".into();
        }
        assert_eq!(state.pipeline.ingest(event).await.status, "Dropped");
        parity(&state).await;
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

const LLM_CLAIM: &str = r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":4,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;

#[tokio::test]
async fn model_claim_is_forced_to_modify_and_saved_actions_survive_disable_rename_and_restart() {
    let (state, model) = fixture(true, LLM_CLAIM).await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    let command = "/改单 成员乙 请把之前那份变成四份";
    assert_eq!(
        send(&state, "u1", command, true, 2000).await.status,
        "Applied"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let rows = records(&state).await;
    let row = &rows[1];
    let original = serde_json::to_value(&row.processing).unwrap();
    let auto = &row.processing.as_ref().unwrap().results[0];
    assert!(auto.understanding.is_modify);
    assert!(matches!(
        auto.understanding.events[0].payload,
        DomainEvent::ClaimCancelled(_)
    ));
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        4,
        "not old one plus four"
    );
    let mut cfg = state.cfg.get().await;
    cfg.gateway.admin_commands_enabled = false;
    cfg.llm.enabled = false;
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"u3","card":"成员乙"}]"#,
    )
    .unwrap();
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.reset().await;
    let replay = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&replay.board), 4);
    assert!(replay.events.iter().all(|event| event.user_id.0 == "u2"));
    assert_eq!(
        serde_json::to_value(&records(&state).await[1].processing).unwrap(),
        original
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn conflicting_model_intent_never_turns_a_target_modify_into_cancel_or_admin_action() {
    for intent in ["cancel", "admin", "unknown"] {
        let reply = LLM_CLAIM.replace("\"claim\"", &format!("\"{intent}\""));
        let (state, model) = fixture(true, &reply).await;
        setup(&state).await;
        send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
        let out = send(
            &state,
            "u1",
            "/改单 成员乙 请把之前那份重新安排",
            true,
            2000,
        )
        .await;
        assert_eq!(out.status, "NeedConfirm", "{intent}: {}", out.detail);
        parity(&state).await;
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert!(!state.pipeline.is_locked().await);
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn locking_rejects_target_modify_without_discarding_understood_actions_or_later_unlocking_it()
{
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    state.pipeline.set_locked(true).await.unwrap();
    assert_eq!(
        send(&state, "u1", "/改单 成员乙 徽章 甲 3", true, 2000)
            .await
            .status,
        "Rejected"
    );
    let rows = records(&state).await;
    let auto = &rows[1].processing.as_ref().unwrap().results[0];
    assert!(!auto.understanding.events.is_empty());
    assert!(auto.manual_phase_at_ingest.is_some());
    state.pipeline.set_locked(false).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn persisted_settlement_lock_and_bad_lock_state_cannot_be_bypassed_with_empty_runtime_state()
{
    for corrupt in [false, true] {
        let (state, model) = fixture(true, "").await;
        setup(&state).await;
        let cfg = state.cfg.get().await;
        let mut checkpoint = crate::services::settlement::checkpoint(&cfg.round.round_id).unwrap();
        checkpoint["locked"] = json!(true);
        let dir = std::path::PathBuf::from(
            std::env::var("PAIGU_SETTLEMENT_DIR")
                .expect("run core tests using the isolated check runner"),
        );
        assert!(dir.is_absolute());
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{}.json", cfg.round.round_id));
        std::fs::write(
            &file,
            if corrupt {
                b"broken".to_vec()
            } else {
                serde_json::to_vec(&checkpoint).unwrap()
            },
        )
        .unwrap();
        state.pipeline.reset().await;
        let out = send(&state, "u1", "/改单 成员乙 徽章 甲 3", true, 2000).await;
        assert_eq!(
            out.status,
            if corrupt { "Error" } else { "Rejected" },
            "{}",
            out.detail
        );
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            0
        );
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn target_modify_is_one_durable_receipt_and_retry_does_not_reapply_or_reask_model() {
    let (state, model) = fixture(true, LLM_CLAIM).await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    let cfg = state.cfg.get().await;
    let binding = IngressBinding::from_config(&cfg);
    let mut event = incoming("/改单 成员乙 请把之前那份变成四份", 2000);
    event.is_admin = true;
    event.ingress = Some(IngressSource {
        id: uuid::Uuid::new_v4().to_string(),
        part: "frame".into(),
    });
    assert_eq!(
        state
            .pipeline
            .deliver(&binding, event.clone())
            .await
            .unwrap(),
        Delivery::Committed
    );
    let original = records(&state).await;
    state.pipeline.reset().await;
    assert_eq!(
        state.pipeline.deliver(&binding, event).await.unwrap(),
        Delivery::Committed
    );
    assert_eq!(records(&state).await.len(), 2);
    assert_eq!(
        serde_json::to_value(records(&state).await).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        4
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn later_self_cancel_keeps_its_position_after_target_modify_and_is_never_coalesced() {
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for (ts, uid, text, admin) in [
        (1000, "u2", "排 徽章 甲 1", false),
        (2000, "u1", "/改单 成员乙 徽章 甲 3", true),
        (3000, "u3", "无关闲聊", false),
        (4000, "u2", "撤 徽章 甲 1", false),
    ] {
        send(&state, uid, text, admin, ts).await;
        parity(&state).await;
    }
    let rows = records(&state).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[1].processing.as_ref().unwrap().results[0]
            .members
            .len(),
        1
    );
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        2
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn target_command_manual_override_and_single_undo_keep_original_source_and_later_messages() {
    use crate::parser::action_override::ActionSelection;
    use crate::services::message_actions::{self, SaveActions, UndoInput};
    let (state, model) = fixture(true, LLM_CLAIM).await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    send(
        &state,
        "u1",
        "/改单 成员乙 请把之前那份变成四份",
        true,
        2000,
    )
    .await;
    let cfg = state.cfg.get().await;
    let original = records(&state).await.remove(1);
    let view = message_actions::read(&state, &original.record_id, &cfg.round.round_id)
        .await
        .unwrap();
    let mut selection: ActionSelection =
        serde_json::from_value(view["editable_selection"].clone()).unwrap();
    let ActionSelection::Replace { actions, note } = &mut selection else {
        panic!("expected complete modify draft")
    };
    actions[0].items[0].quantity = 5;
    *note = "保留管理员原话，修正数量".into();
    message_actions::save(
        &state,
        &original.record_id,
        SaveActions {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id.clone(),
            config_revision: cfg.revision,
            record_revision: view["message"]["record_revision"].as_u64().unwrap(),
            selection,
            supplements: vec![],
            release_supplements: vec![],
        },
    )
    .await
    .unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        5
    );
    send(&state, "u3", "排 徽章 甲 1", false, 3000).await;
    let undo = message_actions::undo_status(&state, &cfg.round.round_id)
        .await
        .unwrap();
    message_actions::undo(
        &state,
        UndoInput {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            round_id: cfg.round.round_id,
            config_revision: cfg.revision,
            undo_token: undo["undo_token"].as_str().unwrap().into(),
        },
    )
    .await
    .unwrap();
    let restored = records(&state).await.remove(1);
    assert_eq!(restored.text, original.text);
    assert_eq!(restored.delegation, original.delegation);
    assert_eq!(
        serde_json::to_value(&restored.processing).unwrap(),
        serde_json::to_value(&original.processing).unwrap()
    );
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        5,
        "four restored plus the later member's one"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn target_modify_reuses_quantity_and_item_validation_without_partially_cancelling_old_claims()
{
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    for (i, body) in ["徽章 甲 0", "徽章 甲 100", "不存在的商品 3"]
        .into_iter()
        .enumerate()
    {
        let out = send(
            &state,
            "u1",
            &format!("/改单 成员乙 {body}"),
            true,
            2000 + i as i64,
        )
        .await;
        assert_ne!(out.status, "Applied", "{body}: {}", out.detail);
        parity(&state).await;
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert!(records(&state)
            .await
            .last()
            .unwrap()
            .processing
            .as_ref()
            .unwrap()
            .results[0]
            .understanding
            .events
            .is_empty());
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_target_modify_persistence_cannot_publish_new_names_or_partial_state() {
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    let before = state.pipeline.board().await;
    let before_names = state.pipeline.who_whats().await;
    let cfg = state.cfg.get().await;
    let root = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap();
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("paigu-auto-test-"));
    let file = root
        .join("messages")
        .join(format!("{}.jsonl", cfg.round.round_id));
    let backup = file.with_extension("before");
    assert!(file.starts_with(root) && backup.starts_with(root));
    std::fs::rename(&file, &backup).unwrap();
    std::fs::create_dir(&file).unwrap();
    let mut event = incoming("/改单 成员乙 徽章 甲 3", 2000);
    event.is_admin = true;
    let out = state.pipeline.ingest(event.clone()).await;
    std::fs::remove_dir(&file).unwrap();
    std::fs::rename(&backup, &file).unwrap();
    assert_eq!(out.status, "Error");
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(state.pipeline.who_whats().await, before_names);
    assert_eq!(state.pipeline.ingest(event).await.status, "Applied");
    parity(&state).await;
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn dropped_and_duplicate_names_never_replace_the_published_member_name() {
    let (state, _) = fixture(false, "").await;
    setup(&state).await;
    let mut event = incoming("排 徽章 甲 1", 1000);
    event.user_id = "u2".into();
    event.nickname = "原称呼".into();
    state.pipeline.ingest(event.clone()).await;
    let before = state.pipeline.board().await;
    let names = state.pipeline.who_whats().await;
    event.nickname = "重复不该改名".into();
    assert_eq!(state.pipeline.ingest(event).await.status, "Duplicate");
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(state.pipeline.who_whats().await, names);
    let mut dropped = incoming("排 徽章 甲 1", 3000);
    dropped.user_id = "u2".into();
    dropped.nickname = "丢弃不该改名".into();
    dropped.group_id = "987654321".into();
    assert_eq!(state.pipeline.ingest(dropped).await.status, "Dropped");
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(state.pipeline.who_whats().await, names);
    parity(&state).await;
}

#[tokio::test]
async fn fabricated_target_metadata_does_not_grant_a_non_admin_role() {
    let (state, model) = fixture(true, "").await;
    setup(&state).await;
    send(&state, "u2", "排 徽章 甲 1", false, 1000).await;
    let mut event = incoming("/改单 成员乙 徽章 甲 3", 2000);
    event.user_id = "u2".into();
    event.nickname.clear();
    event.delegation = Some(crate::bus::Delegation {
        actor_user_id: "u1".into(),
        actor_nickname: "成员01".into(),
        source_message_id: event.message_id.clone(),
        source_text: event.text.clone(),
        target: crate::bus::DelegationKind::AdminModify {
            body: "徽章 甲 3".into(),
        },
    });
    assert!(!event.is_admin);
    assert_eq!(state.pipeline.ingest(event).await.status, "Rejected");
    parity(&state).await;
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn target_modify_replaces_that_members_whole_product_without_erasing_other_products_or_users()
{
    let (state, model) = fixture(false, "").await;
    setup(&state).await;
    let mut cfg = state.cfg.get().await;
    let mut variant = cfg.round.items[0].subitems[0].clone();
    variant.subitem_id = "b".into();
    variant.name = "乙".into();
    cfg.round.items[0].subitems.push(variant);
    let mut stand = cfg.round.items[0].clone();
    stand.item_id = "stand".into();
    stand.name = "立牌".into();
    stand.kind = "single".into();
    stand.subitems.clear();
    cfg.round.items.push(stand);
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for (i, (uid, text)) in [
        ("u2", "排 徽章 甲 1"),
        ("u2", "排 徽章 乙 2"),
        ("u2", "排 立牌 1"),
        ("u3", "排 徽章 乙 4"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            send(&state, uid, text, false, 1000 + i as i64).await.status,
            "Applied"
        );
    }
    assert_eq!(
        send(&state, "u1", "/改单 成员乙 徽章 甲 3", true, 2000)
            .await
            .status,
        "Applied"
    );
    parity(&state).await;
    let result = state.pipeline.recompute().await.unwrap();
    let target = result
        .board
        .user_summaries
        .iter()
        .find(|s| s.user_id.0 == "u2")
        .unwrap();
    let counts: std::collections::BTreeMap<_, _> = target
        .items
        .iter()
        .map(|line| (line.item_id.0.as_str(), line.quantity))
        .collect();
    assert_eq!(
        counts,
        std::collections::BTreeMap::from([("badge", 3), ("stand", 1)])
    );
    let beta = result
        .board
        .item_allocations
        .iter()
        .find(|item| item.item_id.0 == "badge" && item.subitem_id.as_deref() == Some("b"))
        .unwrap();
    assert!(beta
        .boxes
        .iter()
        .flat_map(|column| &column.slots)
        .all(|slot| slot.user_id.as_ref().is_none_or(|id| id.0 == "u3")));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
