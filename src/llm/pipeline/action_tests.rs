use super::automatic_tests::{fixture, incoming, quantity};
use crate::parser::action_override::{ActionSelection, Operation};
use crate::services::message_actions::{self, SaveActions};
use std::sync::atomic::Ordering;

pub(super) async fn opened(state: &crate::api::ApiState) -> (String, SaveActions) {
    let cfg = state.cfg.get().await;
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let context = message_actions::read(state, &record.record_id, &cfg.round.round_id)
        .await
        .unwrap();
    let body = SaveActions {
        transaction_id: uuid::Uuid::new_v4().to_string(),
        round_id: cfg.round.round_id,
        config_revision: cfg.revision,
        record_revision: context["message"]["record_revision"].as_u64().unwrap(),
        selection: serde_json::from_value(context["editable_selection"].clone()).unwrap(),
        supplements: vec![],
        release_supplements: vec![],
    };
    (record.record_id, body)
}

#[tokio::test]
async fn typed_quantity_edit_keeps_raw_and_primary_claim_identity_and_replays_after_restart() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    let cfg = state.cfg.get().await;
    let raw = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    let original_claim = crate::parser::action_override::primary_claim_id(&raw[0]);
    if let ActionSelection::Replace { actions, note } = &mut body.selection {
        actions[0].items[0].quantity = 3;
        *note = "当时指三份".into();
    }
    let result = message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(result["message"]["record_revision"], 2);
    assert_eq!(
        serde_json::to_value(&raw).unwrap(),
        serde_json::to_value(state.messages.read_all(&cfg.round.round_id).await.unwrap()).unwrap()
    );
    state.pipeline.reset().await;
    let replayed = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&replayed.board), 3);
    assert_eq!(
        serde_json::to_value(&replayed.events[0]).unwrap()["payload"]["claim_id"],
        original_claim
    );
    let context = message_actions::read(&state, &id, &cfg.round.round_id)
        .await
        .unwrap();
    assert_eq!(context["manual_note"], "当时指三份");
    assert_eq!(
        context["original_automatic"]["original_outcome"]["status"],
        "Applied"
    );
    assert_eq!(
        context["editable_selection"]["actions"][0]["items"][0]["quantity"],
        3
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn typed_ignore_reject_empty_and_restore_are_distinct_persistent_choices() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    for (choice, status, qty) in [
        (
            ActionSelection::Ignore {
                note: "忽略解释".into(),
            },
            "Ignored",
            0,
        ),
        (
            ActionSelection::Reject {
                note: "拒绝解释".into(),
            },
            "Rejected",
            0,
        ),
        (
            ActionSelection::Replace {
                actions: vec![],
                note: "有意清空".into(),
            },
            "Ignored",
            0,
        ),
        (
            ActionSelection::Automatic {
                note: "恢复初次理解".into(),
            },
            "Applied",
            1,
        ),
    ] {
        let (id, mut body) = opened(&state).await;
        body.selection = choice;
        assert_eq!(
            message_actions::save(&state, &id, body).await.unwrap()["message"]["status"],
            status
        );
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            qty
        );
    }
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn typed_save_is_idempotent_but_reusing_key_for_other_content_is_conflict() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "一次".into(),
    };
    message_actions::save(&state, &id, body.clone())
        .await
        .unwrap();
    let count = state
        .messages
        .read_raw_events(&body.round_id, 0)
        .await
        .unwrap()
        .len();
    assert_eq!(
        message_actions::save(&state, &id, body.clone())
            .await
            .unwrap()["deduplicated"],
        true
    );
    assert_eq!(
        state
            .messages
            .read_raw_events(&body.round_id, 0)
            .await
            .unwrap()
            .len(),
        count
    );
    body.selection = ActionSelection::Reject {
        note: "换内容".into(),
    };
    assert!(matches!(
        message_actions::save(&state, &id, body).await,
        Err(crate::services::ServiceError::Conflict(_))
    ));
}

#[tokio::test]
async fn typed_early_edit_preserves_later_cancel_and_other_members() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut other = incoming("排 徽章 甲 2", 2000);
    other.user_id = "u2".into();
    other.nickname = "成员02".into();
    state.pipeline.process(other).await;
    state.pipeline.process(incoming("撤 徽章 99", 3000)).await;
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        actions[0].items[0].quantity = 3;
    }
    message_actions::save(&state, &id, body).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 2);
    assert_eq!(result.outcomes.len(), 3);
}

#[tokio::test]
async fn typed_multi_action_order_and_ids_survive_reordering_without_duplicate_execution() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        let mut second = actions[0].clone();
        second.action_id = "second".into();
        second.items[0].quantity = 2;
        actions.push(second);
        let mut cancel = actions[0].clone();
        cancel.action_id = "cancel-last".into();
        cancel.operation = Operation::Cancel;
        cancel.items.clear();
        actions.push(cancel);
    }
    message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        actions.swap(0, 1);
    }
    message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        2
    );
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        assert_eq!(actions[0].action_id, "second");
        actions.remove(0);
    }
    message_actions::save(&state, &id, body).await.unwrap();
    let after_delete = state.pipeline.recompute().await.unwrap();
    assert_eq!(
        quantity(&after_delete.board),
        0,
        "delete only the second claim; the remaining latest cancellation still executes"
    );
    assert_eq!(after_delete.events.len(), 2);
    assert!(after_delete
        .events
        .iter()
        .all(|event| !event.event_id.0.ends_with("-second")));
    assert!(after_delete
        .events
        .iter()
        .any(|event| event.event_id.0.ends_with("-primary")));
}

#[tokio::test]
async fn typed_bad_quantity_target_and_stale_versions_do_not_commit() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, body) = opened(&state).await;
    for kind in ["quantity", "target", "config", "record", "round"] {
        let mut bad = body.clone();
        bad.transaction_id = uuid::Uuid::new_v4().to_string();
        if let ActionSelection::Replace { actions, .. } = &mut bad.selection {
            if kind == "quantity" {
                actions[0].items[0].quantity = 0;
            }
            if kind == "target" {
                actions[0].operation = Operation::Cancel;
                actions[0].items.clear();
                actions[0].target_claim_id = Some("other-members-claim".into());
            }
        }
        if kind == "config" {
            bad.config_revision += 100;
        }
        if kind == "record" {
            bad.record_revision += 100;
        }
        if kind == "round" {
            bad.round_id = "other_round".into();
        }
        assert!(
            message_actions::save(&state, &id, bad).await.is_err(),
            "{kind}"
        );
    }
    assert!(state
        .messages
        .read_raw_events(&body.round_id, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
}

#[tokio::test]
async fn typed_restore_missing_auto_is_blocked_and_explicit_correction_remains_available() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    let mut records = state.messages.read_all(&body.round_id).await.unwrap();
    records[0].processing = None;
    state
        .messages
        .replace_all(&body.round_id, &records)
        .await
        .unwrap();
    let correction = body.clone();
    body.selection = ActionSelection::Automatic {
        note: String::new(),
    };
    assert!(message_actions::save(&state, &id, body).await.is_err());
    message_actions::save(&state, &id, correction)
        .await
        .unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
}

#[tokio::test]
async fn typed_torn_journal_tail_does_not_publish_or_report_a_successful_save() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    let cfg = state.cfg.get().await;
    let path = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap()
        .join("messages/events");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join(format!("{}.jsonl", body.round_id)), "{").unwrap();
    body.selection = ActionSelection::Ignore {
        note: "不得发布".into(),
    };
    assert!(message_actions::save(&state, &id, body).await.is_err());
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Applied"
    );
}

#[tokio::test]
async fn typed_permission_rejection_keeps_selection_and_priority_recompute_never_calls_model() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: 10_000,
    });
    cfg.llm.enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        actions[0].items[0].quantity = 3;
    }
    assert_eq!(
        message_actions::save(&state, &id, body).await.unwrap()["message"]["status"],
        "Rejected"
    );
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_users.push("u1".into());
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn typed_and_legacy_overrides_replace_each_other_in_journal_order() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "新入口忽略".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    let (_, body) = opened(&state).await;
    let cfg = state.cfg.get().await;
    crate::services::messages::override_message(
        &state,
        &id,
        crate::services::messages::OverrideInput {
            round_id: body.round_id,
            record_revision: body.record_revision,
            reason: Some("旧入口兼容".into()),
            parsed: crate::parser::rule_parser::RuleParser::parse(
                "排 徽章 甲 2",
                &cfg.round.to_items(),
                false,
            ),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        2
    );
    let (_, mut body) = opened(&state).await;
    body.selection = ActionSelection::Automatic {
        note: "只取原自动".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
}

#[tokio::test]
async fn unknown_manual_selection_version_stops_instead_of_reviving_an_older_choice() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, body) = opened(&state).await;
    message_actions::save(&state, &id, body.clone())
        .await
        .unwrap();
    let mut event = state
        .messages
        .read_raw_events(&body.round_id, 0)
        .await
        .unwrap()
        .remove(0);
    event["payload"]["corrected_parsed_message"]["action_transaction_version"] = 42.into();
    event["payload"]["corrected_parsed_message"]["changes"][0]["revision"] = 3.into();
    event["payload"]["target_record_revision"] = 3.into();
    state
        .messages
        .append_raw_event(&body.round_id, &event)
        .await
        .unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 0);
    assert_eq!(result.outcomes[0].status, "NeedConfirm");
    let context = message_actions::read(&state, &id, &body.round_id)
        .await
        .unwrap();
    assert_eq!(context["effective_source"], "invalid_manual");
    assert_eq!(context["message"]["record_revision"], 3);
    assert!(context["editable_selection"]["actions"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn typed_http_schema_rejects_arbitrary_events_and_duplicate_action_identity() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    let mut injected = serde_json::to_value(&body).unwrap();
    injected["user_id"] = "other".into();
    assert!(serde_json::from_value::<SaveActions>(injected).is_err());
    if let ActionSelection::Replace { actions, .. } = &mut body.selection {
        actions.push(actions[0].clone());
    }
    assert!(message_actions::save(&state, &id, body).await.is_err());
}
