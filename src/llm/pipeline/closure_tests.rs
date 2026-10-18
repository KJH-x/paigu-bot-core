use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::domain::claim::{ClaimType, SlotPolicy};
use crate::parser::action_override::ActionSelection;
use crate::services::message_actions::{self, RecordVersion};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn complete_multiline_strategy_selection_survives_restart_and_later_modify() {
    let (state, model) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    let mut single = cfg.round.items[0].clone();
    single.item_id = "stand".into();
    single.name = "立牌".into();
    single.kind = "single".into();
    single.subitems.clear();
    cfg.round.items.push(single);
    state.cfg.put(cfg, revision).await.unwrap();
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut body) = opened(&state).await;
    if let ActionSelection::Replace { actions, note } = &mut body.selection {
        *note = "保留各行认领类型和位置策略".into();
        for (n, claim_type, policy) in [
            (1, ClaimType::Single, SlotPolicy::Normal),
            (2, ClaimType::Split, SlotPolicy::TailLocked),
            (3, ClaimType::Split, SlotPolicy::FullBox),
        ] {
            let mut line = actions[0].items[0].clone();
            line.line_id = format!("line-{n}");
            line.claim_type = claim_type;
            if line.claim_type == ClaimType::Single {
                line.item_id = "stand".into();
                line.subitem_id = None;
            }
            line.slot_policy = policy;
            line.quantity = n;
            line.notes = Some(format!("第{n}行占位说明"));
            actions[0].items.push(line);
        }
    }
    let expected = serde_json::to_value(&body.selection).unwrap();
    message_actions::save(&state, &id, body.clone())
        .await
        .unwrap();
    state.pipeline.reset().await;
    state.pipeline.recompute().await.unwrap();
    let current = message_actions::read(&state, &id, &body.round_id)
        .await
        .unwrap();
    assert_eq!(current["editable_selection"], expected);
    assert_eq!(
        current["original_automatic"]["original_outcome"]["status"],
        "Applied"
    );
    // A later independent Modify keeps its normalized original meaning after
    // an earlier claim is corrected; it is not absorbed or asked of the model.
    state.pipeline.process(incoming("改 徽章 甲 2", 2000)).await;
    let (_, mut early) = opened(&state).await;
    if let ActionSelection::Replace { actions, .. } = &mut early.selection {
        actions[0].items.truncate(1);
        actions[0].items[0].quantity = 9;
    }
    message_actions::save(&state, &id, early).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        2
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn supplementary_cycles_reuse_foreign_groups_and_instances_cannot_commit() {
    let (state, model) = fixture(false, "").await;
    for ts in [1000, 2000, 3000, 4000] {
        state.pipeline.process(incoming("排 徽章 甲 1", ts)).await;
    }
    let (id, initial) = opened(&state).await;
    let mut rows = state.messages.read_all(&initial.round_id).await.unwrap();
    rows[3].group_id = "999999".into();
    state
        .messages
        .replace_all(&initial.round_id, &rows)
        .await
        .unwrap();
    let (other, other_model) = fixture(false, "").await;
    other.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (foreign_id, _) = opened(&other).await;
    let pick = |i: usize| RecordVersion {
        record_id: rows[i].record_id.clone(),
        record_revision: 1,
    };
    let mut owner = initial.clone();
    owner.transaction_id = "owner".into();
    owner.supplements = vec![pick(2)];
    message_actions::save(&state, &rows[1].record_id, owner)
        .await
        .unwrap();
    let before = state
        .messages
        .read_events_strict(&initial.round_id)
        .await
        .unwrap();
    for (case, targets) in [
        ("nested", vec![pick(1)]),
        ("reuse", vec![pick(2)]),
        ("foreign-group", vec![pick(3)]),
        (
            "foreign-instance",
            vec![RecordVersion {
                record_id: foreign_id,
                record_revision: 1,
            }],
        ),
    ] {
        let mut body = initial.clone();
        body.transaction_id = case.into();
        body.supplements = targets;
        assert!(
            message_actions::save(&state, &id, body).await.is_err(),
            "{case}"
        );
        assert_eq!(
            state
                .messages
                .read_events_strict(&initial.round_id)
                .await
                .unwrap(),
            before
        );
    }
    let context = message_actions::read(&state, &rows[1].record_id, &initial.round_id)
        .await
        .unwrap();
    let mut cycle = initial.clone();
    cycle.transaction_id = "cycle".into();
    cycle.record_revision = context["message"]["record_revision"].as_u64().unwrap();
    cycle.supplements = vec![pick(0)];
    assert!(message_actions::save(&state, &rows[1].record_id, cycle)
        .await
        .is_err());
    assert_eq!(
        state
            .messages
            .read_events_strict(&initial.round_id)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        model.calls.load(Ordering::SeqCst) + other_model.calls.load(Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn historical_message_outside_live_projection_can_be_opened_corrected_and_resynced() {
    let (state, model) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, body) = opened(&state).await;
    let first = crate::services::display::display(&state, 0).await;
    state.pipeline.reset().await;
    assert!(state.pipeline.messages_since(0).await.is_empty());
    let historical = message_actions::read(&state, &id, &body.round_id)
        .await
        .unwrap();
    assert_eq!(historical["message"]["record_id"], id);
    message_actions::save(&state, &id, body).await.unwrap();
    let pipeline = super::Pipeline::new_with_messages(
        state.cfg.clone(),
        model.clone(),
        state.messages.clone(),
    );
    let reopened = crate::api::ApiState {
        cfg: state.cfg.clone(),
        gateway: crate::gateway::Gateway::new(state.cfg.clone(), pipeline.clone()),
        pipeline,
        messages: state.messages.clone(),
    };
    reopened.pipeline.recompute().await.unwrap();
    let resync = crate::services::display::display_with_cursor(
        &reopened,
        i64::MAX,
        first["view_epoch"].as_str(),
        first["view_revision"].as_u64(),
    )
    .await;
    assert_eq!(resync["message_mode"], "replace");
    assert_ne!(resync["view_epoch"], first["view_epoch"]);
    assert_eq!(resync["messages"][0]["record_id"], id);
    assert_eq!(resync["messages"][0]["record_revision"], 2);
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
