use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::services::message_actions::{self, SuggestInput};
use std::sync::atomic::Ordering;

const DRAFT: &str = r#"{"actions":[{"action_id":"primary","operation":"claim","items":[{"line_id":"line-1","item_id":"badge","subitem_id":"a","quantity":3,"claim_type":"Split","slot_policy":"Normal","is_proxy_card":false,"notes":null}],"target_claim_id":null,"target_item_id":null,"quantity":null}]}"#;

#[tokio::test]
async fn explicit_draft_calls_model_once_but_never_saves_or_executes() {
    let (state, llm) = fixture(false, DRAFT).await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.llm.enabled = true;
    cfg.members.cn_overrides.push(crate::settings::CnOverride {
        user_id: "private-user".into(),
        cn: "不应外发的名单".into(),
        aliases: vec![],
    });
    let rev = cfg.revision;
    state.cfg.put(cfg, rev).await.unwrap();
    let (id, mut b) = opened(&state).await;
    let before = state
        .messages
        .read_events_strict(&b.round_id)
        .await
        .unwrap();
    let response = message_actions::suggest(
        &state,
        &id,
        SuggestInput {
            request_id: "explicit-draft".into(),
            draft_version: 7,
            round_id: b.round_id.clone(),
            config_revision: b.config_revision,
            record_revision: b.record_revision,
            selection: b.selection.clone(),
            supplements: vec![],
            instruction: "原意是总共三份".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(response["executed"], false);
    assert_eq!(response["draft_version"], 7);
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    assert_eq!(
        state
            .messages
            .read_events_strict(&b.round_id)
            .await
            .unwrap(),
        before
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    {
        let prompts = llm.prompts.lock().unwrap();
        let input = &prompts[0].1;
        assert!(input.contains("原意是总共三份"));
        for forbidden in [
            "不应外发的名单",
            "private-user",
            "cn_overrides",
            "raw_message",
            "sender_user_id",
            "123456789",
        ] {
            assert!(!input.contains(forbidden), "{forbidden}");
        }
    }
    b.selection = serde_json::from_value(response["selection"].clone()).unwrap();
    if let crate::parser::action_override::ActionSelection::Replace { actions, .. } =
        &mut b.selection
    {
        actions[0].items[0].quantity = 4;
    }
    message_actions::save(&state, &id, b).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        4
    );
    assert_eq!(
        llm.calls.load(Ordering::SeqCst),
        1,
        "manual edit and save do not call model again"
    );
}

#[tokio::test]
async fn unavailable_malformed_or_unsafe_draft_keeps_manual_choice_and_journal() {
    for (enabled, reply) in [
        (false, DRAFT),
        (true, "not json"),
        (true, r#"{"actions":[],"execute":true}"#),
        (
            true,
            r#"{"actions":[{"action_id":"x","operation":"cancel","items":[],"target_claim_id":"invented-claim","target_item_id":null,"quantity":null}]}"#,
        ),
    ] {
        let (state, llm) = fixture(false, reply).await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let mut cfg = state.cfg.get().await;
        cfg.llm.enabled = enabled;
        let rev = cfg.revision;
        state.cfg.put(cfg, rev).await.unwrap();
        let (id, b) = opened(&state).await;
        let before = state
            .messages
            .read_events_strict(&b.round_id)
            .await
            .unwrap();
        let result = message_actions::suggest(
            &state,
            &id,
            SuggestInput {
                request_id: "try-draft".into(),
                draft_version: 1,
                round_id: b.round_id.clone(),
                config_revision: b.config_revision,
                record_revision: b.record_revision,
                selection: b.selection,
                supplements: vec![],
                instruction: "纠正".into(),
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert_eq!(
            state
                .messages
                .read_events_strict(&b.round_id)
                .await
                .unwrap(),
            before
        );
        assert_eq!(llm.calls.load(Ordering::SeqCst), usize::from(enabled));
    }
}
