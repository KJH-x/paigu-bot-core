use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::parser::action_override::ActionSelection;
use crate::services::message_actions::{self, RecordVersion, UndoInput};
use std::sync::atomic::Ordering;

async fn undo_body(state: &crate::api::ApiState) -> UndoInput {
    let cfg = state.cfg.get().await;
    let slot = message_actions::undo_status(state, &cfg.round.round_id)
        .await
        .unwrap();
    assert_eq!(slot["available"], true);
    UndoInput {
        transaction_id: uuid::Uuid::new_v4().to_string(),
        round_id: cfg.round.round_id,
        config_revision: cfg.revision,
        undo_token: slot["undo_token"].as_str().unwrap().into(),
    }
}

#[tokio::test]
async fn atomic_supplements_keep_intervening_other_member_and_undo_after_new_messages() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut other = incoming("排 徽章 甲 2", 2000);
    other.user_id = "u2".into();
    other.nickname = "成员02".into();
    state.pipeline.process(other).await;
    state.pipeline.process(incoming("排 徽章 甲 2", 3000)).await;
    let (id, mut body) = opened(&state).await;
    let raw = state.messages.read_all(&body.round_id).await.unwrap();
    body.supplements = vec![RecordVersion {
        record_id: raw[2].record_id.clone(),
        record_revision: 1,
    }];
    if let ActionSelection::Replace { actions, note } = &mut body.selection {
        actions[0].items[0].quantity = 3;
        *note = "补充说明合在第一条".into();
    }
    let result = message_actions::save(&state, &id, body.clone())
        .await
        .unwrap();
    assert_eq!(result["undo"]["records"], 2);
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        5
    );
    let detail = message_actions::read(&state, &raw[2].record_id, &body.round_id)
        .await
        .unwrap();
    assert_eq!(detail["absorbed_by"], id);
    assert_eq!(detail["current_outcome"]["status"], "Rejected");
    let events = state
        .messages
        .read_events_strict(&body.round_id)
        .await
        .unwrap();
    let event: crate::domain::event::EventEnvelope =
        serde_json::from_value(events[0].clone()).unwrap();
    let t = crate::messages::transactions::decode(&event).unwrap();
    assert_eq!(t.changes.len(), 2);
    assert_eq!(t.checkpoint.message_refs.len(), 3);
    // A new cancel after the edit must still execute when the edit is undone.
    state.pipeline.process(incoming("撤 徽章 1", 4000)).await;
    state.pipeline.process(incoming("排 徽章 甲 1", 5000)).await;
    let undo = undo_body(&state).await;
    message_actions::undo(&state, undo.clone()).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        5
    );
    assert_eq!(
        message_actions::undo_status(&state, &body.round_id)
            .await
            .unwrap()["available"],
        false
    );
    assert_eq!(
        message_actions::undo(&state, undo).await.unwrap()["deduplicated"],
        true
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn automatic_group_conversion_and_undo_preserve_post_edit_units() {
    let (state, _) = fixture(false, "{}").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    let rev = cfg.revision;
    state.cfg.put(cfg, rev).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.ingest(incoming("排 徽章 甲 2", 2000)).await;
    let (id, mut body) = opened(&state).await;
    let raw = state.messages.read_all(&body.round_id).await.unwrap();
    body.supplements = vec![RecordVersion {
        record_id: raw[1].record_id.clone(),
        record_revision: 1,
    }];
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        // The unit includes both original item lines; replace the complete unit
        // with one four-piece line rather than silently retaining the other line.
        actions.truncate(1);
        actions[0].items.truncate(1);
        actions[0].items[0].quantity = 4;
    }
    message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        4
    );
    state.pipeline.ingest(incoming("排 徽章 甲 1", 3000)).await;
    state.pipeline.ingest(incoming("撤 徽章 1", 4000)).await;
    let undo = undo_body(&state).await;
    message_actions::undo(&state, undo).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 3);
    assert_eq!(result.outcomes.len(), 4);
    assert_eq!(result.outcomes[0].status, "Merged");
}

#[tokio::test]
async fn undo_reopens_from_a_fresh_pipeline_and_restores_previous_manual_choice() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut b) = opened(&state).await;
    b.selection = ActionSelection::Ignore {
        note: "之前忽略".into(),
    };
    message_actions::save(&state, &id, b).await.unwrap();
    let (id, mut b) = opened(&state).await;
    b.selection = ActionSelection::Reject {
        note: "最新拒绝".into(),
    };
    message_actions::save(&state, &id, b).await.unwrap();
    let pipeline =
        super::Pipeline::new_with_messages(state.cfg.clone(), llm, state.messages.clone());
    let fresh = crate::api::ApiState {
        members_pull: Default::default(),
        cfg: state.cfg.clone(),
        messages: state.messages.clone(),
        gateway: crate::gateway::Gateway::new(state.cfg.clone(), pipeline.clone()),
        pipeline,
    };
    fresh.pipeline.recompute().await.unwrap();
    let u = undo_body(&fresh).await;
    message_actions::undo(&fresh, u).await.unwrap();
    let detail = message_actions::read(&fresh, &id, &fresh.cfg.get().await.round.round_id)
        .await
        .unwrap();
    assert_eq!(detail["current_outcome"]["status"], "Ignored");
    assert_eq!(detail["manual_note"], "之前忽略");
    assert_eq!(detail["message"]["record_revision"], 4);
}

#[tokio::test]
async fn failed_save_does_not_consume_undo_slot_and_new_manual_transaction_expires_old_token() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut b) = opened(&state).await;
    b.selection = ActionSelection::Ignore {
        note: String::new(),
    };
    message_actions::save(&state, &id, b).await.unwrap();
    let old = undo_body(&state).await;
    let (_, mut bad) = opened(&state).await;
    bad.record_revision = 999;
    assert!(message_actions::save(&state, &id, bad).await.is_err());
    assert_eq!(undo_body(&state).await.undo_token, old.undo_token);
    let (_, mut next) = opened(&state).await;
    next.selection = ActionSelection::Reject {
        note: String::new(),
    };
    message_actions::save(&state, &id, next).await.unwrap();
    assert!(message_actions::undo(&state, old).await.is_err());
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Rejected"
    );
}

#[tokio::test]
async fn undo_rejects_changed_policy_phase_and_corrupted_input_without_rollback() {
    for case in ["policy", "phase", "input"] {
        let (state, _) = fixture(false, "{}").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let (id, mut b) = opened(&state).await;
        b.selection = ActionSelection::Ignore {
            note: String::new(),
        };
        message_actions::save(&state, &id, b).await.unwrap();
        let mut u = undo_body(&state).await;
        if case == "policy" {
            let mut c = state.cfg.get().await;
            c.round.priority_users.push("u1".into());
            let rev = c.revision;
            u.config_revision = state.cfg.put(c, rev).await.unwrap();
        }
        if case == "phase" {
            state.pipeline.set_locked(true).await.unwrap();
            state.pipeline.set_locked(false).await.unwrap();
        }
        if case == "input" {
            let mut rows = state.messages.read_all(&u.round_id).await.unwrap();
            rows[0].text.push('变');
            state
                .messages
                .replace_all(&u.round_id, &rows)
                .await
                .unwrap();
        }
        assert!(message_actions::undo(&state, u).await.is_err(), "{case}");
        assert_eq!(
            state.pipeline.messages_since(0).await[0]["status"],
            "Ignored"
        );
    }
}

#[tokio::test]
async fn transaction_rejects_partial_auto_groups_duplicate_and_reverse_supplements() {
    let (state, _) = fixture(false, "{}").await;
    let mut c = state.cfg.get().await;
    c.merge.enabled = true;
    let rev = c.revision;
    state.cfg.put(c, rev).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.ingest(incoming("排 徽章 甲 2", 2000)).await;
    let (id, b) = opened(&state).await;
    assert!(message_actions::save(&state, &id, b.clone()).await.is_err());
    let mut duplicate = b.clone();
    duplicate.supplements = vec![RecordVersion {
        record_id: id.clone(),
        record_revision: 1,
    }];
    assert!(message_actions::save(&state, &id, duplicate).await.is_err());
    let rows = state.messages.read_all(&b.round_id).await.unwrap();
    let mut reversed = b;
    reversed.supplements = vec![RecordVersion {
        record_id: id,
        record_revision: 1,
    }];
    assert!(message_actions::save(&state, &rows[1].record_id, reversed)
        .await
        .is_err());
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
}

#[tokio::test]
async fn journal_write_failures_are_atomic_and_lost_response_retries_recover() {
    for fault in [1, 2, 3] {
        let (state, llm) = fixture(false, "{}").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let (id, mut b) = opened(&state).await;
        b.selection = ActionSelection::Ignore {
            note: "已提交但可能丢响应".into(),
        };
        let raw =
            serde_json::to_value(state.messages.read_only(&b.round_id).await.unwrap()).unwrap();
        let before = state
            .messages
            .read_events_strict(&b.round_id)
            .await
            .unwrap();
        state.messages.inject_durable_fault(fault);
        assert!(message_actions::save(&state, &id, b.clone()).await.is_err());
        assert_eq!(
            state.pipeline.messages_since(0).await[0]["status"],
            "Applied"
        );
        assert_eq!(
            serde_json::to_value(state.messages.read_only(&b.round_id).await.unwrap()).unwrap(),
            raw
        );
        let after = state
            .messages
            .read_events_strict(&b.round_id)
            .await
            .unwrap();
        if fault < 3 {
            assert_eq!(after, before, "no half transaction after interrupted write");
            assert_eq!(
                message_actions::undo_status(&state, &b.round_id)
                    .await
                    .unwrap()["available"],
                false
            );
        } else {
            assert_eq!(after.len(), before.len() + 1);
            state.pipeline.reset().await;
            state.pipeline.recompute().await.unwrap();
            assert_eq!(
                state.pipeline.messages_since(0).await[0]["status"],
                "Ignored"
            );
            assert_eq!(
                message_actions::save(&state, &id, b.clone()).await.unwrap()["deduplicated"],
                true
            );
            assert_eq!(
                state
                    .messages
                    .read_events_strict(&b.round_id)
                    .await
                    .unwrap()
                    .len(),
                after.len()
            );
        }
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn corrupted_atomic_choice_cannot_publish_valid_siblings_or_revive_prior_actions() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    let (id, mut b) = opened(&state).await;
    let rows = state.messages.read_only(&b.round_id).await.unwrap();
    b.supplements = vec![RecordVersion {
        record_id: rows[1].record_id.clone(),
        record_revision: 1,
    }];
    message_actions::save(&state, &id, b.clone()).await.unwrap();
    let mut event = state
        .messages
        .read_events_strict(&b.round_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    event["payload"]["corrected_parsed_message"]["changes"][1]["after"]["value"]
        ["action_selection_version"] = serde_json::json!(99);
    let cfg = state.cfg.get().await;
    let path = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap()
        .join("messages/events")
        .join(format!("{}.jsonl", b.round_id));
    std::fs::write(path, format!("{}\n", event)).unwrap();
    let replay = state.pipeline.recompute().await.unwrap();
    assert!(replay.outcomes.iter().all(|m| m.status == "NeedConfirm"));
    assert_eq!(quantity(&replay.board), 0);
}

#[tokio::test]
async fn releasing_supplement_restores_its_exact_previous_choice_and_is_undoable() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    let (id, mut b) = opened(&state).await;
    let rows = state.messages.read_only(&b.round_id).await.unwrap();
    b.supplements = vec![RecordVersion {
        record_id: rows[1].record_id.clone(),
        record_revision: 1,
    }];
    if let ActionSelection::Replace { actions, .. } = &mut b.selection {
        actions[0].items[0].quantity = 3;
    }
    message_actions::save(&state, &id, b).await.unwrap();
    let (_, mut detach) = opened(&state).await;
    detach.release_supplements = vec![RecordVersion {
        record_id: rows[1].record_id.clone(),
        record_revision: 2,
    }];
    message_actions::save(&state, &id, detach).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        5
    );
    let undo = undo_body(&state).await;
    message_actions::undo(&state, undo).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
}

#[tokio::test]
async fn projection_cursor_detects_old_note_quantity_and_policy_edits_without_event_count_change() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let first = crate::services::display::display(&state, 0).await;
    let cursor = first["view_revision"].as_u64().unwrap();
    let epoch = first["view_epoch"].as_str().unwrap();
    let workflow_first = crate::services::workflow::snapshot(&state).await;
    assert_eq!(workflow_first["view_epoch"], first["view_epoch"]);
    assert_eq!(workflow_first["view_revision"], first["view_revision"]);
    let unchanged =
        crate::services::display::display_with_cursor(&state, 999999, Some(epoch), Some(cursor))
            .await;
    assert_eq!(unchanged["message_mode"], "unchanged");
    let (id, mut b) = opened(&state).await;
    if let ActionSelection::Replace { actions, note } = &mut b.selection {
        actions[0].items[0].quantity = 3;
        *note = "只改数量不加事件".into();
    }
    message_actions::save(&state, &id, b).await.unwrap();
    let next =
        crate::services::display::display_with_cursor(&state, 999999, Some(epoch), Some(cursor))
            .await;
    assert_eq!(next["version"], first["version"]);
    assert!(next["view_revision"].as_u64().unwrap() > cursor);
    assert_eq!(next["full_resync"], true);
    assert_eq!(next["messages"][0]["record_id"], id);
    assert_eq!(next["messages"][0]["manual_note"], "只改数量不加事件");
    let (_, mut b) = opened(&state).await;
    if let ActionSelection::Replace { note, .. } = &mut b.selection {
        *note = "只改解释".into();
    }
    message_actions::save(&state, &id, b).await.unwrap();
    let note = crate::services::display::display_with_cursor(
        &state,
        0,
        Some(epoch),
        next["view_revision"].as_u64(),
    )
    .await;
    let workflow_note = crate::services::workflow::snapshot(&state).await;
    assert_eq!(workflow_note["view_epoch"], note["view_epoch"]);
    assert_eq!(workflow_note["view_revision"], note["view_revision"]);
    assert_eq!(workflow_note["version"], workflow_first["version"]);
    assert!(workflow_note["view_revision"].as_u64() > workflow_first["view_revision"].as_u64());
    assert!(note["view_revision"].as_u64() > next["view_revision"].as_u64());
    assert_eq!(note["board"], next["board"]);
    let unknown = crate::services::display::display_with_cursor(
        &state,
        0,
        Some("old-process"),
        note["view_revision"].as_u64(),
    )
    .await;
    assert_eq!(unknown["message_mode"], "replace");
    assert_eq!(unknown["messages"].as_array().unwrap().len(), 1);
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_users.push("u1".into());
    let rev = cfg.revision;
    state.cfg.put(cfg, rev).await.unwrap();
    let policy = crate::services::display::display_with_cursor(
        &state,
        0,
        Some(epoch),
        note["view_revision"].as_u64(),
    )
    .await;
    assert!(policy["view_revision"].as_u64() > note["view_revision"].as_u64());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
