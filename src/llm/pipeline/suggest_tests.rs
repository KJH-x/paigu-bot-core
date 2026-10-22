use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::services::message_actions::{self, SuggestInput};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Weak};

const DRAFT: &str = r#"{"actions":[{"action_id":"primary","operation":"claim","items":[{"line_id":"line-1","item_id":"badge","subitem_id":"a","quantity":3,"claim_type":"Split","slot_policy":"Normal","is_proxy_card":false,"notes":null}],"target_claim_id":null,"target_item_id":null,"quantity":null}]}"#;

struct ConcurrentModel {
    state: Mutex<Weak<crate::api::ApiState>>,
    change: &'static str,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl crate::llm::client::LlmClient for ConcurrentModel {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        // A new automatic group may independently ask for understanding while
        // the explicitly requested draft is waiting. Do not recursively run
        // this test's concurrent mutation from that second model call.
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            return Ok(r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":3,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#.into());
        }
        let state = self.state.lock().unwrap().upgrade().unwrap();
        match self.change {
            "lock" => state.pipeline.set_locked(true).await?,
            "config" => {
                let _writer = state.pipeline.message_write_guard().await;
                let mut cfg = state.cfg.get().await;
                let revision = cfg.revision;
                cfg.display.refresh_ms += 1;
                state.cfg.put(cfg, revision).await?;
            }
            "group" => {
                state.pipeline.ingest(incoming("排 徽章 甲 1", 3000)).await;
            }
            _ => {
                let cfg = state.cfg.get().await;
                let records = state.messages.read_all(&cfg.round.round_id).await?;
                let record = &records[usize::from(self.change == "supplement")];
                let context = message_actions::read(&state, &record.record_id, &cfg.round.round_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                message_actions::save(
                    &state,
                    &record.record_id,
                    message_actions::SaveActions {
                        transaction_id: uuid::Uuid::new_v4().to_string(),
                        round_id: cfg.round.round_id,
                        config_revision: cfg.revision,
                        record_revision: context["message"]["record_revision"].as_u64().unwrap(),
                        selection: crate::parser::action_override::ActionSelection::Ignore {
                            note: "并发人工操作".into(),
                        },
                        supplements: vec![],
                        release_supplements: vec![],
                    },
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            }
        }
        Ok(DRAFT.into())
    }
}

#[tokio::test]
async fn pending_draft_rechecks_main_supplements_config_group_and_lock_without_holding_writer() {
    for change in ["main", "supplement", "config", "group", "lock"] {
        let (old, _) = fixture(false, "").await;
        old.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        old.pipeline.process(incoming("排 徽章 甲 1", 2000)).await;
        let mut cfg = old.cfg.get().await;
        let revision = cfg.revision;
        cfg.llm.enabled = true;
        cfg.merge.enabled = change == "group";
        old.cfg.put(cfg, revision).await.unwrap();
        let model = Arc::new(ConcurrentModel {
            state: Mutex::new(Weak::new()),
            change,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let pipeline = super::Pipeline::new_with_messages(
            old.cfg.clone(),
            model.clone(),
            old.messages.clone(),
        );
        let state = Arc::new(crate::api::ApiState {
            members_pull: Default::default(),
            cfg: old.cfg.clone(),
            gateway: crate::gateway::Gateway::new(old.cfg.clone(), pipeline.clone()),
            pipeline,
            messages: old.messages.clone(),
        });
        *model.state.lock().unwrap() = Arc::downgrade(&state);
        state.pipeline.recompute().await.unwrap();
        let (id, body) = opened(&state).await;
        let records = state.messages.read_all(&body.round_id).await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            message_actions::suggest(
                &state,
                &id,
                SuggestInput {
                    request_id: "controlled-pending".into(),
                    draft_version: 1,
                    round_id: body.round_id,
                    config_revision: body.config_revision,
                    record_revision: body.record_revision,
                    selection: body.selection,
                    supplements: vec![message_actions::RecordVersion {
                        record_id: records[1].record_id.clone(),
                        record_revision: 1,
                    }],
                    instruction: "这里原意为三份".into(),
                },
            ),
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{change}: model wait cannot hold the common writer: {error}")
        });
        assert!(
            result.is_err(),
            "{change}: a draft cannot be returned over a changed selected context or lock"
        );
        assert_eq!(
            model.calls.load(Ordering::SeqCst),
            if change == "group" { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn partial_automatic_unit_is_rejected_before_model_and_complete_unit_is_draft_only() {
    let (state, model) = fixture(false, DRAFT).await;
    let mut cfg = state.cfg.get().await;
    let rev = cfg.revision;
    cfg.merge.enabled = true;
    state.cfg.put(cfg, rev).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.ingest(incoming("排 徽章 甲 2", 2000)).await;
    let mut cfg = state.cfg.get().await;
    let rev = cfg.revision;
    cfg.llm.enabled = true;
    state.cfg.put(cfg, rev).await.unwrap();
    let (id, body) = opened(&state).await;
    let rows = state.messages.read_all(&body.round_id).await.unwrap();
    let input = || SuggestInput {
        request_id: "unit-scope".into(),
        draft_version: 1,
        round_id: body.round_id.clone(),
        config_revision: body.config_revision,
        record_revision: body.record_revision,
        selection: body.selection.clone(),
        supplements: vec![],
        instruction: "当时一共三份".into(),
    };
    assert!(message_actions::suggest(&state, &id, input())
        .await
        .is_err());
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    let mut complete = input();
    complete.supplements.push(message_actions::RecordVersion {
        record_id: rows[1].record_id.clone(),
        record_revision: 1,
    });
    let result = message_actions::suggest(&state, &id, complete)
        .await
        .unwrap();
    assert_eq!(result["executed"], false);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    assert!(state
        .messages
        .read_events_strict(&body.round_id)
        .await
        .unwrap()
        .is_empty());
}

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
