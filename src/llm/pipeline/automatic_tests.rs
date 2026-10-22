use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::settings::{default_config, ItemConfig, PriorityWindow, SubitemConfig};

pub(super) struct CountingLlm {
    pub(super) calls: AtomicUsize,
    reply: String,
    pub(super) prompts: std::sync::Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl LlmClient for CountingLlm {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        system: &str,
        user: &str,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts
            .lock()
            .unwrap()
            .push((system.into(), user.into()));
        Ok(self.reply.clone())
    }
}

const CLAIM: &str = r#"{"intent":"claim","items":[{"item":"徽章","subitem":"甲","quantity":2,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;

pub(super) async fn fixture(
    enabled: bool,
    reply: &str,
) -> (crate::api::ApiState, Arc<CountingLlm>) {
    let dir = std::env::temp_dir().join(format!("paigu-auto-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = default_config();
    cfg.round.round_id = format!("auto_{}", uuid::Uuid::new_v4().simple());
    cfg.active_round_id = None;
    cfg.gateway.reply_enabled = false;
    cfg.round.group_id = "123456789".to_string();
    cfg.gateway.whitelist_groups = vec![cfg.round.group_id.clone()];
    cfg.gateway.whitelist_members.clear();
    cfg.round.phases.clear();
    cfg.round.priority_users.clear();
    cfg.round.priority_window = None;
    cfg.merge.enabled = false;
    cfg.members.cache_path = dir.join("members.json").display().to_string();
    cfg.members.cn_overrides.clear();
    cfg.llm.enabled = enabled;
    cfg.llm.fallback_to_rules = true;
    cfg.llm.segment_claims = false;
    cfg.round.items = vec![ItemConfig {
        item_id: "badge".to_string(),
        name: "徽章".to_string(),
        kind: "拼团".to_string(),
        class: Some("B".to_string()),
        aliases: vec![],
        unit_price_cents: 1000,
        max_quantity: None,
        box_item_id: None,
        subitems: vec![SubitemConfig {
            subitem_id: "a".to_string(),
            name: "甲".to_string(),
            aliases: vec![],
            unit_price_cents: 1000,
            adjust_cents: 0,
            capacity: Some(1),
        }],
    }];
    let path = dir.join("config.json");
    std::fs::write(&path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    let store = Arc::new(ConfigStore::load(path).unwrap());
    let llm = Arc::new(CountingLlm {
        calls: AtomicUsize::new(0),
        reply: reply.to_string(),
        prompts: std::sync::Mutex::new(vec![]),
    });
    let pipeline = Pipeline::new_with_client_dir(store.clone(), llm.clone(), dir.join("messages"));
    let state = crate::api::ApiState {
        members_pull: Default::default(),
        cfg: store.clone(),
        gateway: crate::gateway::Gateway::new(store, pipeline.clone()),
        messages: pipeline.messages(),
        pipeline,
    };
    (state, llm)
}

pub(super) fn incoming(text: &str, ts: i64) -> IncomingEvent {
    IncomingEvent {
        group_id: "123456789".to_string(),
        user_id: "u1".to_string(),
        nickname: "成员01".to_string(),
        message_id: uuid::Uuid::new_v4().to_string(),
        text: text.to_string(),
        timestamp_ms: ts,
        is_admin: false,
        sender_user_id: None,
        ingress: None,
        sender_nickname: None,
        raw: None,
    }
}

pub(super) fn quantity(board: &crate::domain::snapshot::AllocationSnapshot) -> u32 {
    board
        .user_summaries
        .iter()
        .flat_map(|user| &user.items)
        .map(|item| item.quantity)
        .sum()
}

#[tokio::test]
async fn rejected_llm_actions_revalidate_priority_without_a_second_model_call() {
    let (state, llm) = fixture(true, CLAIM).await;
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_window = Some(PriorityWindow {
        start_ms: 0,
        end_ms: 10_000,
    });
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    assert_eq!(
        state
            .pipeline
            .process(incoming("帮我留两份", 1000))
            .await
            .status,
        "Rejected"
    );
    let cfg = state.cfg.get().await;
    let records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    let original = serde_json::to_value(&records[0].processing).unwrap();
    let auto = &records[0].processing.as_ref().unwrap().results[0];
    assert_eq!(auto.original_outcome.status, "Rejected");
    assert_eq!(auto.understanding.events.len(), 1);
    let mut next = cfg.clone();
    next.round.priority_users.push("u1".to_string());
    next.llm.enabled = false;
    state.cfg.put(next, cfg.revision).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 2);
    assert_eq!(result.outcomes[0].status, "Applied");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    let after = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(
        serde_json::to_value(&after[0].processing).unwrap(),
        original
    );
    let info =
        crate::services::messages::actions(&state, &records[0].record_id, &cfg.round.round_id)
            .await
            .unwrap();
    assert_eq!(
        info["original_automatic"]["original_outcome"]["status"],
        "Rejected"
    );
    assert_eq!(info["current_outcome"]["status"], "Applied");
    assert_eq!(info["effective_source"], "saved_automatic");
}

#[tokio::test]
async fn alias_changes_cannot_retarget_saved_actions_or_revive_ignored_chatter() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.process(incoming("闲聊专用词", 2000)).await;
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.round.items[0].name = "改名商品".to_string();
    cfg.round.items[0].subitems[0].name = "改名款式".to_string();
    let mut other = cfg.round.items[0].clone();
    other.item_id = "other".to_string();
    other.name = "徽章".to_string();
    other.aliases = vec!["闲聊专用词".to_string()];
    other.subitems[0].name = "甲".to_string();
    cfg.round.items.push(other);
    state.cfg.put(cfg, revision).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(result.outcomes[1].status, "Ignored");
    assert_eq!(quantity(&result.board), 1);
    assert_eq!(result.board.user_summaries[0].items[0].item_id.0, "badge");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn manual_override_keeps_automatic_candidates_and_note_in_replay() {
    let reply =
        r#"{"intent":"claim","items":[],"confidence":0.8,"ambiguous_parts":["候选待核对"]}"#;
    let (state, llm) = fixture(true, reply).await;
    assert_eq!(
        state
            .pipeline
            .process(incoming("这两份是哪种", 1000))
            .await
            .status,
        "NeedConfirm"
    );
    let cfg = state.cfg.get().await;
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let original = serde_json::to_value(&record.processing).unwrap();
    assert!(record.processing.as_ref().unwrap().results[0]
        .original_outcome
        .reply
        .as_ref()
        .unwrap()
        .contains("候选待核对"));
    let parsed =
        crate::parser::rule_parser::RuleParser::parse("排 徽章 甲 1", &cfg.round.to_items(), false);
    let body = crate::services::messages::OverrideInput {
        round_id: cfg.round.round_id.clone(),
        record_revision: 1,
        parsed,
        reason: Some("管理员澄清：认购一份".to_string()),
    };
    crate::services::messages::override_message(&state, &record.record_id, body)
        .await
        .unwrap();
    let after = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(serde_json::to_value(&after.processing).unwrap(), original);
    let view = state.pipeline.replay_view().await.unwrap();
    assert_eq!(view["messages"][0]["manual_note"], "管理员澄清：认购一份");
    assert_eq!(
        view["messages"][0]["original_automatic"]["outcome"]["status"],
        "NeedConfirm"
    );
    assert_eq!(view["messages"][0]["status"], "Applied");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn automatic_groups_keep_original_results_and_claim_cancel_are_separate() {
    let (state, llm) = fixture(false, "{}").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    let revision = cfg.revision;
    state.cfg.put(cfg.clone(), revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.ingest(incoming("排 徽章 甲 2", 2000)).await;
    let records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(records[0].status, "Applied", "原自动结果不可被投影改写");
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Merged"
    );
    assert_eq!(
        records[0].processing.as_ref().unwrap().results[0]
            .original_outcome
            .status,
        "Applied"
    );
    assert_eq!(
        records[1].processing.as_ref().unwrap().results[0]
            .members
            .len(),
        2
    );
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("撤 徽章 3", 3000))
            .await
            .status,
        "Applied"
    );
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 0);
    assert_ne!(result.outcomes.last().unwrap().status, "Merged");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn out_of_order_group_appends_history_without_losing_original_result() {
    let (state, _) = fixture(false, "{}").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg.clone(), revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 2000)).await;
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 2", 1000))
            .await
            .status,
        "Merged"
    );
    let records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(records[0].processing.as_ref().unwrap().results.len(), 2);
    assert_eq!(
        records[0].processing.as_ref().unwrap().results[0]
            .members
            .len(),
        1
    );
    assert_eq!(
        records[1].processing.as_ref().unwrap().results[0]
            .original_outcome
            .status,
        "Merged"
    );
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
}

#[tokio::test]
async fn raw_edit_and_delete_cannot_destroy_a_saved_automatic_baseline() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let input: crate::services::messages::UpdateInput =
        serde_json::from_value(serde_json::json!({"text":"改原文","record_revision":1,"round_id":cfg.round.round_id,"config_revision":cfg.revision})).unwrap();
    assert!(
        crate::services::messages::update_message(&state, &record.record_id, input)
            .await
            .is_err()
    );
    assert!(crate::services::messages::delete_message(
        &state,
        &record.record_id,
        &cfg.round.round_id,
        cfg.revision,
        1,
        None
    )
    .await
    .is_err());
    let after = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(record).unwrap()
    );
}

#[tokio::test]
async fn empty_override_stays_effective_and_automatic_claim_identity_is_stable() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let automatic = &record.processing.as_ref().unwrap().results[0];
    let original_claim = serde_json::to_value(&automatic.understanding.events[0]).unwrap()
        ["payload"]["claim_id"]
        .clone();
    let replay = state.pipeline.recompute().await.unwrap();
    assert_eq!(
        serde_json::to_value(&replay.events[0]).unwrap()["payload"]["claim_id"],
        original_claim
    );
    let parsed =
        crate::parser::rule_parser::RuleParser::parse("闲聊", &cfg.round.to_items(), false);
    crate::services::messages::override_message(
        &state,
        &record.record_id,
        crate::services::messages::OverrideInput {
            round_id: cfg.round.round_id,
            record_revision: 1,
            parsed,
            reason: Some("人工忽略".to_string()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        0
    );
}

#[tokio::test]
async fn lock_history_does_not_revoke_earlier_claims_or_reactivate_locked_messages() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.set_locked(true).await.unwrap();
    assert_eq!(
        state
            .pipeline
            .process(incoming("排 徽章 甲 2", 2000))
            .await
            .status,
        "Rejected"
    );
    state.pipeline.set_locked(false).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 1);
    assert_eq!(result.outcomes[0].status, "Applied");
    assert_eq!(result.outcomes[1].status, "Rejected");
}

#[tokio::test]
async fn missing_damaged_or_future_artifacts_never_fall_back_to_rule_execution() {
    for case in ["empty", "damaged", "future", "mismatch"] {
        let (state, llm) = fixture(false, "{}").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let cfg = state.cfg.get().await;
        let mut records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
        let mut encoded = serde_json::to_value(&records[0]).unwrap();
        match case {
            "empty" => encoded["processing"] = serde_json::json!({"results":[]}),
            "damaged" => {
                encoded["processing"] = serde_json::json!({"results":[{"invalid":"retained"}]});
            }
            "future" => encoded["processing"]["results"][0]["format_version"] = 42.into(),
            "mismatch" => {
                encoded["processing"]["results"][0]["members"][0]["input_hash"] =
                    "different-input".into();
            }
            _ => unreachable!(),
        }
        records[0] = serde_json::from_value(encoded).unwrap();
        state
            .messages
            .replace_all(&cfg.round.round_id, &records)
            .await
            .unwrap();
        let before = serde_json::to_value(&records).unwrap();
        let result = state.pipeline.recompute().await.unwrap();
        assert_eq!(quantity(&result.board), 0, "{case}");
        assert_eq!(result.outcomes[0].status, "NeedConfirm", "{case}");
        assert!(result.outcomes[0].detail.contains("未重新解析"), "{case}");
        let info =
            crate::services::messages::actions(&state, &records[0].record_id, &cfg.round.round_id)
                .await
                .unwrap();
        assert_eq!(info["automatic_matches"], false, "{case}");
        let after = state.messages.read_all(&cfg.round.round_id).await.unwrap();
        // Only the current execution projection may change; never replace the damaged baseline.
        assert_eq!(
            serde_json::to_value(&after[0].processing).unwrap(),
            before[0]["processing"]
        );
        assert_eq!(after[0].text, "排 徽章 甲 1");
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn legacy_record_is_readable_but_details_do_not_invent_an_automatic_baseline() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    let mut records = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    records[0].processing = None;
    state
        .messages
        .replace_all(&cfg.round.round_id, &records)
        .await
        .unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    let before = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    let info =
        crate::services::messages::actions(&state, &records[0].record_id, &cfg.round.round_id)
            .await
            .unwrap();
    assert_eq!(info["automatic_missing"], true);
    assert!(info["original_automatic"].is_null());
    assert!(info["current_automatic"].is_null());
    let after = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn merge_setting_changes_do_not_repartition_persisted_automatic_units() {
    let (state, llm) = fixture(false, "{}").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.ingest(incoming("排 徽章 甲 2", 2000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = false;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    let split = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&split.board), 3);
    assert_eq!(split.outcomes[0].status, "Merged");
    assert_eq!(split.outcomes[1].status, "Applied");
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        3
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn removed_catalog_target_is_rejected_instead_of_reinterpreted() {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let mut cfg = state.cfg.get().await;
    cfg.round.items[0].item_id = "replacement".to_string();
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 0);
    assert_eq!(result.outcomes[0].status, "Rejected");
    assert!(result.outcomes[0].detail.contains("未按新别名改排"));
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn locked_messages_are_group_boundaries_and_cannot_swallow_earlier_claims() {
    let (state, llm) = fixture(false, "{}").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    state.pipeline.ingest(incoming("排 徽章 甲 1", 1000)).await;
    state.pipeline.set_locked(true).await.unwrap();
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 2", 2000))
            .await
            .status,
        "Rejected"
    );
    state.pipeline.set_locked(false).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 1);
    assert_eq!(result.outcomes[0].status, "Applied");
    assert_eq!(result.outcomes[1].status, "Rejected");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
