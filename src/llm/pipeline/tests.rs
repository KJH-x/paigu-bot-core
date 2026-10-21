use super::*;
use crate::messages::JsonlMessageStore;
use crate::replay::session::ReplayOverrides;
use crate::round::{PhaseWindow, RoundPhase};
use crate::settings::{default_config, AppConfig, LlmSettings, PriorityWindow};
use std::path::PathBuf;

struct MockClient {
    replies: std::sync::Mutex<std::collections::VecDeque<MockReply>>,
}

#[derive(Clone)]
enum MockReply {
    Ok(String),
    Err(String),
}

impl MockClient {
    fn script(replies: Vec<MockReply>) -> Arc<Self> {
        Arc::new(Self {
            replies: std::sync::Mutex::new(replies.into()),
        })
    }

    fn ok(body: &str) -> Arc<Self> {
        Self::script(vec![MockReply::Ok(body.to_string())])
    }

    fn err(message: &str) -> Arc<Self> {
        Self::script(vec![MockReply::Err(message.to_string())])
    }
}

#[async_trait]
impl LlmClient for MockClient {
    async fn complete(
        &self,
        _settings: &LlmSettings,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> anyhow::Result<String> {
        let mut queue = self.replies.lock().unwrap();
        let next = if queue.len() > 1 {
            queue.pop_front().unwrap()
        } else {
            queue.front().cloned().unwrap()
        };
        match next {
            MockReply::Ok(body) => Ok(body),
            MockReply::Err(message) => Err(anyhow::anyhow!(message)),
        }
    }
}

fn base_config() -> AppConfig {
    let mut cfg = default_config();
    cfg.gateway.reply_enabled = false;
    cfg.llm.enabled = true;
    cfg.llm.fallback_to_rules = true;
    cfg.llm.api_key_env = "PAIGU_TEST_KEY_UNSET".to_string();
    cfg.round.priority_users = vec!["prio_user".to_string()];
    cfg.round.priority_window = None;
    cfg
}

#[tokio::test]
async fn llm_actions_survive_recompute_without_reinterpreting_original_text() {
    let mut cfg = base_config();
    cfg.round.phases.clear();
    let reply = r#"{"intent":"claim","items":[{"item":"通行证","subitem":"结城理","quantity":2,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&cfg, MockClient::ok(reply));
    let outcome = pipeline
        .process(event("u1", "成员01", "帮我留两份", 1000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let records = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    let history = records[0]
        .processing
        .as_ref()
        .expect("all automatic understanding must be persisted");
    assert_eq!(
        history.results[0].understanding.source,
        crate::messages::processing::UnderstandingSource::Llm
    );
    assert_eq!(history.results[0].understanding.events.len(), 1);
    let before = pipeline.board().await.1;
    // Prove that replay is independent of the current parser/model.
    let replay = pipeline.recompute().await.unwrap();
    assert_eq!(
        canonical_allocations(before),
        canonical_allocations(serde_json::to_value(replay.board).unwrap())
    );
    let after = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&records[0].processing).unwrap(),
        serde_json::to_value(&after[0].processing).unwrap()
    );
}

fn canonical_allocations(mut board: Value) -> Value {
    if let Some(object) = board.as_object_mut() {
        object.remove("generated_at");
    }
    board
}

#[tokio::test]
async fn active_replay_view_uses_recorded_steps_without_mutating_live_state() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    assert_eq!(
        pipeline
            .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
            .await
            .status,
        "Applied"
    );
    pipeline.process(event("u2", "成员02", "闲聊", 2_000)).await;
    assert_eq!(
        pipeline
            .process(event("u1", "成员01", "改 通行证 结城理 2", 3_000))
            .await
            .status,
        "Applied"
    );
    let before = pipeline.board().await;
    let view = pipeline.replay_view().await.unwrap();
    let after = pipeline.board().await;
    assert_eq!(
        before, after,
        "read-only history must not change the live board"
    );
    assert_eq!(view["source"], "active_round");
    assert_eq!(view["round_id"], cfg.round.round_id);
    let steps = view["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["who_whats"][0]["items"][0]["qty"], 1);
    assert_eq!(steps[1]["who_whats"], steps[0]["who_whats"]);
    assert_eq!(steps[2]["who_whats"][0]["items"][0]["qty"], 2);
    assert_eq!(
        steps[2]["board"]["item_allocations"],
        before.1["item_allocations"]
    );
    pipeline.reset().await;
    let empty = pipeline.replay_view().await.unwrap();
    assert_eq!(empty["round_id"], cfg.round.round_id);
    assert_eq!(empty["steps"], serde_json::json!([]));
    assert_eq!(empty["who_whats"], serde_json::json!([]));
}

fn store(cfg: &AppConfig) -> Arc<ConfigStore> {
    let path = std::env::temp_dir().join(format!("paigu-llm-test-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
    Arc::new(ConfigStore::load(path).unwrap())
}

fn test_pipeline_with_dir(cfg: &AppConfig, llm: Arc<dyn LlmClient>) -> (Arc<Pipeline>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("paigu-msgs-test-{}", uuid::Uuid::new_v4()));
    (
        Pipeline::new_with_client_dir(store(cfg), llm, dir.clone()),
        dir,
    )
}

fn test_pipeline(cfg: &AppConfig, llm: Arc<dyn LlmClient>) -> Arc<Pipeline> {
    test_pipeline_with_dir(cfg, llm).0
}

fn api_state(pipeline: Arc<Pipeline>) -> crate::api::ApiState {
    crate::api::ApiState {
        cfg: pipeline.cfg.clone(),
        gateway: crate::gateway::Gateway::new(pipeline.cfg.clone(), pipeline.clone()),
        messages: pipeline.messages(),
        pipeline,
    }
}

fn override_body(
    round_id: &str,
    revision: u64,
    quantity: u32,
) -> crate::services::messages::OverrideInput {
    serde_json::from_value(serde_json::json!({
        "round_id": round_id, "record_revision": revision,
        "parsed": {
            "intent":"Claim", "round_hint":null,
            "items":[{"name":"通行证 结城理", "quantity":quantity}],
            "cancel_target_hint":null, "admin_command":null,
            "confidence":1.0, "ambiguous_parts":[]
        }
    }))
    .unwrap()
}

#[tokio::test]
async fn override_guards_round_revision_and_exact_record_identity() {
    use crate::services::{messages::override_message, ServiceError};
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::script(vec![]));
    pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    let state = api_state(pipeline);
    let record = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    let audit_before = state
        .messages
        .read_raw_events(&cfg.round.round_id, 0)
        .await
        .unwrap()
        .len();
    assert!(matches!(
        override_message(
            &state,
            &record.record_id,
            override_body("other_round", 1, 2)
        )
        .await,
        Err(ServiceError::Conflict(_))
    ));
    assert!(matches!(
        override_message(
            &state,
            &record.record_id,
            override_body(&cfg.round.round_id, 99, 2)
        )
        .await,
        Err(ServiceError::Conflict(_))
    ));
    assert!(matches!(
        override_message(
            &state,
            &record.seq.to_string(),
            override_body(&cfg.round.round_id, 1, 2)
        )
        .await,
        Err(ServiceError::NotFound(_))
    ));
    assert_eq!(
        state
            .messages
            .read_raw_events(&cfg.round.round_id, 0)
            .await
            .unwrap()
            .len(),
        audit_before
    );
    // Two simultaneous editors of revision 1 cannot silently overwrite each other.
    let (a, b) = tokio::join!(
        override_message(
            &state,
            &record.record_id,
            override_body(&cfg.round.round_id, 1, 2)
        ),
        override_message(
            &state,
            &record.record_id,
            override_body(&cfg.round.round_id, 1, 3)
        ),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(
        matches!(a, Err(ServiceError::Conflict(_))) || matches!(b, Err(ServiceError::Conflict(_)))
    );
    let current = state
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        current.record_revision, 1,
        "人工版本留在事务投影，原记录不改写"
    );
    assert_eq!(current.record_id, record.record_id);
    assert_eq!(current.seq, record.seq);
    assert_eq!(current.timestamp_ms, record.timestamp_ms);
    assert_eq!(current.text, record.text);
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["record_revision"],
        2
    );
}

#[tokio::test]
async fn record_bound_override_does_not_change_another_message_with_same_upstream_id() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::script(vec![]));
    pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    let mut records = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    let original = records.remove(0);
    let mut another = original.clone();
    another.record_id = crate::messages::new_record_id();
    // This independently inserted legacy row must not reuse another record's bound artifact.
    another.processing = None;
    another.seq = crate::messages::next_seq();
    another.user_id = "u2".to_string();
    another.nickname = "成员02".to_string();
    another.timestamp_ms = 2_000;
    pipeline
        .messages
        .append(&cfg.round.round_id, &another)
        .await
        .unwrap();
    let state = api_state(pipeline);
    let result = crate::services::messages::override_message(
        &state,
        &original.record_id,
        override_body(&cfg.round.round_id, 1, 3),
    )
    .await
    .unwrap();
    let messages = result["result"]["messages"].as_array().unwrap();
    assert_eq!(
        messages
            .iter()
            .find(|m| m["record_id"] == another.record_id)
            .unwrap()["record_revision"],
        1
    );
    let board: crate::domain::snapshot::AllocationSnapshot =
        serde_json::from_value(result["result"]["board"].clone()).unwrap();
    let qty = |uid: &str| {
        board
            .user_summaries
            .iter()
            .filter(|u| u.user_id.0 == uid)
            .flat_map(|u| &u.items)
            .map(|i| i.quantity)
            .sum::<u32>()
    };
    assert_eq!(qty("u1"), 3);
    assert_eq!(qty("u2"), 1);
}

#[tokio::test]
async fn original_position_override_still_executes_later_cancel() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::script(vec![]));
    pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 2", 1_000))
        .await;
    pipeline
        .process(event("u2", "成员02", "排 通行证 结城理 1", 2_000))
        .await;
    pipeline
        .process(event("u1", "成员01", "撤 通行证 1", 90_000))
        .await;
    let records = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    let state = api_state(pipeline);
    let result = crate::services::messages::override_message(
        &state,
        &records[0].record_id,
        override_body(&cfg.round.round_id, 1, 1),
    )
    .await
    .unwrap();
    let board: crate::domain::snapshot::AllocationSnapshot =
        serde_json::from_value(result["result"]["board"].clone()).unwrap();
    assert!(board.user_summaries.iter().all(|u| u.user_id.0 != "u1"));
    assert_eq!(board.user_summaries[0].user_id.0, "u2");
    let after = state.messages.read_all(&cfg.round.round_id).await.unwrap();
    for (before, after) in records.iter().zip(after) {
        assert_eq!(before.record_id, after.record_id);
        assert_eq!(before.text, after.text);
        assert_eq!(before.timestamp_ms, after.timestamp_ms);
        assert_eq!(before.seq, after.seq);
    }
}

#[tokio::test]
async fn override_of_partial_automatic_group_is_blocked_until_group_transactions_exist() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::script(vec![]));
    pipeline
        .ingest(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    pipeline
        .ingest(event("u1", "成员01", "排 通行证 虎狼丸 1", 2_000))
        .await;
    let records = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    let state = api_state(pipeline);
    let before = state.pipeline.board().await;
    let result = crate::services::messages::override_message(
        &state,
        &records[1].record_id,
        override_body(&cfg.round.round_id, 1, 2),
    )
    .await;
    assert!(
        matches!(result, Err(crate::services::ServiceError::Conflict(v)) if v["error"] == "auto_group_requires_transaction")
    );
    assert_eq!(before, state.pipeline.board().await);
    assert!(state
        .messages
        .read_raw_events(&cfg.round.round_id, 0)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn failed_message_persistence_does_not_publish_or_consume_deduplication() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.phases.clear();
    let dir = std::env::temp_dir().join(format!("paigu-write-failure-{}", uuid::Uuid::new_v4()));
    std::fs::write(&dir, "not a directory").unwrap();
    let pipeline =
        Pipeline::new_with_client_dir(store(&cfg), MockClient::script(vec![]), dir.clone());
    let ev = event("u1", "成员01", "排 通行证 结城理 1", 1_000);
    assert_eq!(pipeline.process(ev.clone()).await.status, "Error");
    assert!(pipeline.messages_since(0).await.is_empty());
    assert_eq!(pipeline.board().await.0, 0);
    std::fs::remove_file(&dir).unwrap();
    assert_eq!(
        pipeline.process(ev).await.status,
        "Applied",
        "a failed write must be retryable"
    );
}

#[tokio::test]
async fn live_message_identity_matches_persisted_record_before_and_after_recompute() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.gateway.admin_commands_enabled = true;
    cfg.round.phases.clear();
    let pipeline = test_pipeline(&cfg, MockClient::err("must not call LLM"));
    // Other writers (including drops and audit events) can consume global sequence numbers.
    crate::messages::next_seq();
    let claim = event("u1", "成员01", "排 通行证 结城理 1", 1_000);
    pipeline.process(claim.clone()).await;
    crate::messages::next_seq();
    pipeline.process(event("u2", "成员02", "闲聊", 2_000)).await;
    pipeline.process(claim).await;
    pipeline.process(admin_event("/状态", 3_000)).await;

    let records = pipeline
        .messages
        .read_all(&cfg.round.round_id)
        .await
        .unwrap();
    for view in [pipeline.messages_since(0).await, {
        pipeline.recompute().await.unwrap();
        pipeline.messages_since(0).await
    }] {
        assert_eq!(view.len(), records.len());
        for message in view {
            let rec = records
                .iter()
                .find(|r| message["record_id"] == r.record_id)
                .expect("live and persisted records must have exactly the same stable identity");
            assert_eq!(message["record_revision"], rec.record_revision);
            assert_eq!(
                message["seq"], rec.seq,
                "do not allocate a separate UI sequence"
            );
            assert_eq!(message["round_id"], cfg.round.round_id);
            assert_eq!(message["text"], rec.text);
        }
    }
}

fn set_class(cfg: &mut AppConfig, item_id: &str, class: &str) {
    for it in &mut cfg.round.items {
        if it.item_id == item_id {
            it.class = Some(class.to_string());
        }
    }
}

fn phase_window(phase: RoundPhase, start_ms: i64, end_ms: i64) -> PhaseWindow {
    PhaseWindow {
        phase,
        start_ms,
        end_ms,
    }
}

fn event(user_id: &str, nickname: &str, text: &str, timestamp_ms: i64) -> IncomingEvent {
    IncomingEvent {
        group_id: "123456789".to_string(),
        user_id: user_id.to_string(),
        nickname: nickname.to_string(),
        message_id: format!("{}::{}", user_id, text),
        text: text.to_string(),
        timestamp_ms,
        is_admin: false,
        sender_user_id: None,
        sender_nickname: None,
        raw: None,
    }
}

fn admin_event(text: &str, timestamp_ms: i64) -> IncomingEvent {
    let mut ev = event("admin1", "管理员", text, timestamp_ms);
    ev.is_admin = true;
    ev
}

#[tokio::test]
async fn admin_commands_recorded_only_when_disabled() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.gateway.admin_commands_enabled = false;
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));

    let out = pipeline.process(admin_event("/锁位", 1_000)).await;
    assert_eq!(out.status, "Applied");
    assert!(out.detail.contains("开关关闭"), "{}", out.detail);

    // 未执行：仍可排
    let claim = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 2_000))
        .await;
    assert_eq!(claim.status, "Applied");
}

#[tokio::test]
async fn admin_command_lock_blocks_claims_when_enabled() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.gateway.admin_commands_enabled = true;
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));

    let lock = pipeline.process(admin_event("/锁位", 1_000)).await;
    assert_eq!(lock.status, "Applied");
    assert!(lock.detail.contains("已锁定"), "{}", lock.detail);

    let blocked = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 2_000))
        .await;
    assert_eq!(blocked.status, "Rejected");
    assert!(blocked.detail.contains("已锁定"), "{}", blocked.detail);

    let open = pipeline.process(admin_event("/开团", 3_000)).await;
    assert_eq!(open.status, "Applied");

    let ok = pipeline
        .process(event("u1", "小明", "排 通行证 岳羽由加莉 1", 4_000))
        .await;
    assert_eq!(ok.status, "Applied");
}

#[tokio::test]
async fn modify_replaces_previous_claim_quantity() {
    let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));

    let first = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(first.status, "Applied");

    let modified = pipeline
        .process(event("u1", "小明", "改 通行证 结城理 2", 2_000))
        .await;
    assert_eq!(modified.status, "Applied");
    assert!(modified.detail.starts_with("改单"), "{}", modified.detail);

    let who = pipeline.who_whats().await;
    assert_eq!(who.len(), 1);
    let qty = who[0]["items"][0]["qty"].as_i64().unwrap_or(0);
    assert_eq!(qty, 2, "改单应替换为 2：{who:?}");
}

fn slot_user(snapshot: &Value, item_id: &str, subitem_id: &str) -> Option<String> {
    let allocations = snapshot.get("item_allocations")?.as_array()?;
    for allocation in allocations {
        if allocation.get("item_id").and_then(|v| v.as_str()) == Some(item_id)
            && allocation.get("subitem_id").and_then(|v| v.as_str()) == Some(subitem_id)
        {
            let slots = allocation
                .get("boxes")?
                .as_array()?
                .first()?
                .get("slots")?
                .as_array()?;
            return slots
                .first()
                .and_then(|s| s.get("user_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
    }
    None
}

#[tokio::test]
async fn rule_claim_success() {
    let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    assert!(outcome.reply.unwrap().contains("已记录"));
    assert_eq!(outcome.version, 1);
    assert!(!pipeline.messages_since(0).await.is_empty());
}

#[tokio::test]
async fn llm_claim_success() {
    let body = r#"{"intent":"claim","items":[{"item":"通行认证SP-月行水上","subitem":"岳羽由加莉","quantity":1,"claim_type":"split","slot_policy":"normal"}],"confidence":0.95,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
    let outcome = pipeline
        .process(event("u1", "小明", "帮我抢个yukari", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let who = pipeline.who_whats().await;
    assert_eq!(who.len(), 1);
    assert_eq!(who[0]["display"], "小明");
}

#[tokio::test]
async fn non_claim_ignored() {
    let body = r#"{"intent":"unknown","items":[],"confidence":0.2,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
    let outcome = pipeline
        .process(event("u1", "小明", "今天天气不错", 1_000))
        .await;
    assert_eq!(outcome.status, "Ignored");
}

#[tokio::test]
async fn bare_subitem_first_match_resolves_to_catalog_first() {
    let body = r#"{"intent":"claim","items":[{"subitem":"结城理","quantity":1,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
    let outcome = pipeline
        .process(event("u1", "小明", "帮我留一份yukari", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let who = pipeline.who_whats().await;
    assert_eq!(who[0]["items"][0]["name"], "通行认证SP-月行水上");
}

#[tokio::test]
async fn rule_bare_subitem_first_match() {
    let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "小明", "排 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let who = pipeline.who_whats().await;
    assert_eq!(who[0]["items"][0]["name"], "通行认证SP-月行水上");
}

#[tokio::test]
async fn ambiguous_category_need_confirm() {
    let body = r#"{"intent":"claim","items":[{"item":"SP","quantity":1,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
    let outcome = pipeline
        .process(event("u1", "小明", "帮我留一份", 1_000))
        .await;
    assert_eq!(outcome.status, "NeedConfirm");
}

#[tokio::test]
async fn first_match_clarification_emits_parse_override() {
    let parse_body = r#"{"intent":"claim","items":[{"subitem":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body =
        r#"{"matches":[{"name":"悠人","item":"通行认证SP-月行水上","subitem":"结城理"}]}"#;
    let pipeline = test_pipeline(
        &base_config(),
        MockClient::script(vec![
            MockReply::Ok(parse_body.to_string()),
            MockReply::Ok(clarify_body.to_string()),
        ]),
    );
    let outcome = pipeline
        .process(event("u1", "小明", "要一个悠人", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let raw = pipeline
        .messages()
        .read_raw_events("月行水上", 0)
        .await
        .unwrap();
    assert!(
        raw.iter()
            .any(|v| v["payload"]["event_type"] == "ParseOverride"),
        "应落盘 ParseOverride 事件: {raw:?}"
    );
    assert!(
        raw.iter()
            .any(|v| v["payload"]["target_raw_message_id"] == "u1::要一个悠人"),
        "ParseOverride 应指向原 message_id"
    );
}

/// §U7：重放消费 `ParseOverride`，与实时对同一消息结果一致。
#[tokio::test]
async fn replay_consumes_parse_override_matching_realtime() {
    let parse_body = r#"{"intent":"claim","items":[{"subitem":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body =
        r#"{"matches":[{"name":"悠人","item":"通行认证SP-月行水上","subitem":"结城理"}]}"#;
    let cfg = base_config();
    let pipeline = test_pipeline(
        &cfg,
        MockClient::script(vec![
            MockReply::Ok(parse_body.to_string()),
            MockReply::Ok(clarify_body.to_string()),
        ]),
    );

    let live = pipeline
        .process(event("u1", "小明", "要一个悠人", 1_000))
        .await;
    assert_eq!(live.status, "Applied");
    let (_, live_board) = pipeline.board().await;
    assert_eq!(
        slot_user(&live_board, "pass_sp", "v_jcl"),
        Some("u1".to_string())
    );

    let log = pipeline.messages();
    let replayed = crate::replay::session::replay(log.as_ref(), &cfg, ReplayOverrides::default())
        .await
        .unwrap();
    assert_eq!(
        replayed.outcomes[0].status, "Applied",
        "{:?}",
        replayed.outcomes
    );
    let replayed_board = serde_json::to_value(&replayed.board).unwrap();
    assert_eq!(
        slot_user(&replayed_board, "pass_sp", "v_jcl"),
        Some("u1".to_string()),
        "重放应消费 ParseOverride，与实时一致"
    );
}

#[tokio::test]
async fn first_match_clarification_failure_falls_back_to_ignore() {
    let parse_body = r#"{"intent":"claim","items":[{"subitem":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body = r#"{"matches":[{"name":"悠人","item":"不存在的商品","subitem":"谁"}]}"#;
    let pipeline = test_pipeline(
        &base_config(),
        MockClient::script(vec![
            MockReply::Ok(parse_body.to_string()),
            MockReply::Ok(clarify_body.to_string()),
        ]),
    );
    let outcome = pipeline
        .process(event("u1", "小明", "要一个悠人", 1_000))
        .await;
    assert_eq!(outcome.status, "Ignored");
    let raw = pipeline
        .messages()
        .read_raw_events("月行水上", 0)
        .await
        .unwrap();
    assert!(raw.is_empty(), "澄清失败不应落 ParseOverride 事件");
}

#[tokio::test]
async fn first_match_clarification_failure_without_fallback_rejected() {
    let mut cfg = base_config();
    cfg.llm.fallback_to_rules = false;
    let parse_body = r#"{"intent":"claim","items":[{"subitem":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(
        &cfg,
        MockClient::script(vec![
            MockReply::Ok(parse_body.to_string()),
            MockReply::Err("timeout".to_string()),
        ]),
    );
    let outcome = pipeline
        .process(event("u1", "小明", "要一个悠人", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert_eq!(outcome.detail, "没识别成功");
}

#[tokio::test]
async fn priority_window_rejected() {
    let mut cfg = base_config();
    cfg.round.priority_window = Some(crate::settings::PriorityWindow {
        start_ms: 0,
        end_ms: i64::MAX,
    });
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert!(outcome.detail.contains("预存"));
}

#[tokio::test]
async fn priority_user_sorts_first() {
    let pipeline = test_pipeline(&base_config(), MockClient::ok("{}"));
    let first = pipeline
        .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(first.status, "Applied");
    let second = pipeline
        .process(event("prio_user", "prio_user", "排 通行证 结城理 1", 2_000))
        .await;
    assert_eq!(second.status, "Applied");
    let (_, snapshot) = pipeline.board().await;
    assert_eq!(
        slot_user(&snapshot, "pass_sp", "v_jcl"),
        Some("prio_user".to_string())
    );
}

#[tokio::test]
async fn llm_failure_falls_back_to_rules() {
    let pipeline = test_pipeline(&base_config(), MockClient::err("timeout"));
    let outcome = pipeline
        .process(event("u1", "小明", "嗯嗯好的", 1_000))
        .await;
    assert_eq!(outcome.status, "Ignored");
}

#[tokio::test]
async fn llm_failure_without_fallback_rejected() {
    let mut cfg = base_config();
    cfg.llm.fallback_to_rules = false;
    let pipeline = test_pipeline(&cfg, MockClient::err("timeout"));
    let outcome = pipeline
        .process(event("u1", "小明", "嗯嗯好的", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert_eq!(outcome.detail, "没识别成功");
}

#[tokio::test]
async fn phase_ii_non_priority_class_a_rejected() {
    let mut cfg = base_config();
    cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
    set_class(&mut cfg, "pass_sp", "A");
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert!(outcome.detail.contains("阶段"), "detail={}", outcome.detail);
}

#[tokio::test]
async fn phase_ii_priority_class_a_applied() {
    let mut cfg = base_config();
    cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
    set_class(&mut cfg, "pass_sp", "A");
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("prio_user", "成员09", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
}

#[tokio::test]
async fn phase_ii_class_b_allowed_for_non_priority() {
    let mut cfg = base_config();
    cfg.round.phases = vec![phase_window(RoundPhase::PhaseII, 0, i64::MAX)];
    set_class(&mut cfg, "pass_sp", "B");
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
}

#[tokio::test]
async fn phase_i_class_a_rejected() {
    let mut cfg = base_config();
    cfg.round.phases = vec![phase_window(RoundPhase::PhaseI, 0, i64::MAX)];
    set_class(&mut cfg, "pass_sp", "A");
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert!(outcome.detail.contains("阶段"));
}

#[tokio::test]
async fn locked_phase_rejects_claim() {
    let mut cfg = base_config();
    cfg.round.phases = vec![phase_window(RoundPhase::Locked, 0, i64::MAX)];
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("prio_user", "成员09", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Rejected");
    assert!(outcome.detail.contains("阶段"));
}

#[tokio::test]
async fn empty_phases_are_unrestricted() {
    let mut cfg = base_config();
    cfg.round.phases = Vec::new();
    set_class(&mut cfg, "pass_sp", "A");
    let pipeline = test_pipeline(&cfg, MockClient::ok("{}"));
    let outcome = pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
}

#[tokio::test]
async fn message_log_records_every_inbound() {
    let cfg = base_config();
    let (pipeline, dir) = test_pipeline_with_dir(&cfg, MockClient::ok("{}"));
    pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    pipeline
        .process(event("u1", "成员01", "排 通行证 结城理 1", 1_000))
        .await;
    pipeline
        .process(event("u2", "成员02", "今天天气不错", 2_000))
        .await;

    let store = JsonlMessageStore::new(&dir, &cfg.round.round_id);
    let recs = store.read_all().await.unwrap();
    assert_eq!(recs.len(), 3);
    assert_eq!(recs[0].status, "Applied");
    assert_eq!(recs[1].status, "Duplicate");
    assert_eq!(recs[2].status, "Ignored");
    assert!(recs.iter().all(|r| r.routed == "message"));
    assert_eq!(recs[0].nickname, "成员01");
    assert_eq!(recs[0].timestamp_ms, 1_000);
    assert!(recs.windows(2).all(|w| w[1].seq > w[0].seq));

    let visible = pipeline.messages_since(0).await;
    assert!(visible.windows(2).all(|w| {
        w[1]["seq"].as_i64().unwrap_or_default() > w[0]["seq"].as_i64().unwrap_or_default()
    }));
}

#[test]
fn forged_envelope_unwraps_source_text_and_drops_admin() {
    let mut ev = event(
        "10000",
        "测试号",
        r#"{"id":"123456","text":"排 通行证 结城理 1"}"#,
        1_000,
    );
    ev.is_admin = true;
    let message_id = ev.message_id.clone();
    let out = unwrap_forged_envelope(ev);
    assert_eq!(out.user_id, "123456");
    assert_eq!(out.nickname, "123456");
    assert_eq!(out.text, "排 通行证 结城理 1");
    assert_eq!(out.group_id, "123456789");
    assert_eq!(out.message_id, message_id);
    assert_eq!(out.timestamp_ms, 1_000);
    assert!(!out.is_admin);
}

#[test]
fn forged_envelope_parses_numeric_id_and_optional_fields() {
    let ev = event(
        "10000",
        "测试号",
        r#"{"user_id":123456,"text":"撤销","nickname":"甲","is_admin":true}"#,
        2,
    );
    let out = unwrap_forged_envelope(ev);
    assert_eq!(out.user_id, "123456");
    assert_eq!(out.nickname, "甲");
    assert_eq!(out.text, "撤销");
    assert!(out.is_admin);
}

#[test]
fn forged_envelope_overrides_timestamp() {
    let ev = event(
        "10000",
        "测试号",
        r#"{"id":"1","text":"排 通行证 1","timestamp":1791813600000}"#,
        5,
    );
    assert_eq!(unwrap_forged_envelope(ev).timestamp_ms, 1791813600000);

    let ev2 = event(
        "10000",
        "测试号",
        r#"{"id":"1","text":"x","timestamp":"2026-10-12T22:30:00+08:00"}"#,
        5,
    );
    assert_eq!(unwrap_forged_envelope(ev2).timestamp_ms, 1791815400000);

    // 缺省用外层时间。
    let ev3 = event("10000", "测试号", r#"{"id":"1","text":"x"}"#, 7);
    assert_eq!(unwrap_forged_envelope(ev3).timestamp_ms, 7);
}

#[tokio::test]
async fn segment_claims_routes_multi_item_message_through_llm() {
    let mut cfg = base_config();
    cfg.llm.segment_claims = true;
    let body = r#"{"intent":"claim","items":[
        {"item":"pass_sp","subitem":"结城理","quantity":1},
        {"item":"hr_resume","quantity":1}
    ],"confidence":0.9}"#;
    let (pipeline, _dir) = test_pipeline_with_dir(&cfg, MockClient::ok(body));
    let out = pipeline
        .process(event(
            "u1",
            "成员01",
            "排 通行证 结城理 1 人事部简历 1",
            1000,
        ))
        .await;
    assert_eq!(out.status, "Applied", "detail={}", out.detail);
    assert!(out.detail.contains("pass_sp"), "detail={}", out.detail);
    assert!(out.detail.contains("hr_resume"), "detail={}", out.detail);
}

#[tokio::test]
async fn ambiguous_bare_subitem_is_first_matched_not_need_confirm() {
    let mut cfg = base_config();
    cfg.llm.segment_claims = true;
    // LLM 把「只报角色名」放进 ambiguous_parts（模拟 "排 结城理 1"）。
    let body = r#"{"intent":"claim","items":[],"ambiguous_parts":["结城理（可能为多个商品）"],"confidence":0.3}"#;
    let (pipeline, _dir) = test_pipeline_with_dir(&cfg, MockClient::ok(body));
    let out = pipeline
        .process(event("u1", "成员01", "排 结城理 1", 1000))
        .await;
    assert_eq!(out.status, "Applied", "detail={}", out.detail);
    assert!(out.detail.contains("pass_sp"), "detail={}", out.detail);
}

#[test]
fn proxy_segments_split_mixed_message() {
    let segs = proxy_segments("成员01：旅骨通行证1 代成员02排:人事部简历1").unwrap();
    assert_eq!(segs.len(), 2, "{segs:?}");
    assert!(segs[0].0.is_none());
    assert!(segs[0].1.contains("旅骨通行证"));
    assert_eq!(segs[1].0.as_deref(), Some("成员02"));
    assert!(segs[1].1.contains("人事部简历"));
    assert!(proxy_segments("排 通行证 1").is_none());
}

#[tokio::test]
async fn proxy_attributes_in_group_and_out_group() {
    let dir = std::env::temp_dir().join(format!("paigu-proxy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let members = dir.join("members.json");
    std::fs::write(
        &members,
        r#"[{"user_id":"8888","card":"小明","nickname":"ming"}]"#,
    )
    .unwrap();
    let mut cfg = base_config();
    cfg.members.cache_path = members.to_string_lossy().to_string();
    let (pipeline, _d) = test_pipeline_with_dir(&cfg, MockClient::ok("{}"));

    // 群内被代「小明」→ 归属 8888
    let out = pipeline
        .process(event(
            "10000",
            "发送者",
            "代小明排:排 通行证 结城理 1",
            1000,
        ))
        .await;
    assert_eq!(out.status, "Applied", "detail={}", out.detail);

    // 群外被代「One」→ 记 `发送者(代One)`
    let out2 = pipeline
        .process(event("10000", "发送者", "代One排:排 通行证 结城理 1", 2000))
        .await;
    assert_eq!(out2.status, "Applied", "detail={}", out2.detail);
}

#[test]
fn non_forged_messages_pass_through() {
    for text in [
        "排 通行证 1",
        "{\"foo\":\"bar\"}",
        "{\"id\":\"1\"}",
        "{\"text\":\"x\"}",
        "not json",
    ] {
        let ev = event("u", "n", text, 5);
        let out = unwrap_forged_envelope(ev);
        assert_eq!(out.user_id, "u");
        assert_eq!(out.text, text);
    }
}

#[test]
fn gate_nc_unwraps_only_for_test_round_and_test_group() {
    use crate::round::RoundCategory;

    let env = r#"{"id":"123456","text":"排 通行证 结城理 1"}"#;

    let mut cfg = base_config();
    cfg.gateway.accept_forged_messages = true;
    cfg.gateway.test_group_id = Some("123456789".to_string());
    cfg.round.group_id = "123456789".to_string();

    // 正常轮（非测试轮）→ 不解包。
    cfg.round.category = RoundCategory::Normal;
    let out = gate_nc(&cfg, event("10000", "测试号", env, 1));
    assert_eq!(out.user_id, "10000");
    assert_eq!(out.text, env);

    // 测试轮 + 测试群 → 解包。
    cfg.round.category = RoundCategory::Test;
    let out = gate_nc(&cfg, event("10000", "测试号", env, 1));
    assert_eq!(out.user_id, "123456");
    assert_eq!(out.text, "排 通行证 结城理 1");

    // 测试轮但来自其它群 → 不解包。
    let mut other = event("10000", "测试号", env, 1);
    other.group_id = "999".to_string();
    assert_eq!(gate_nc(&cfg, other).user_id, "10000");

    // 开关关闭 → 不解包。
    cfg.gateway.accept_forged_messages = false;
    assert_eq!(
        gate_nc(&cfg, event("10000", "测试号", env, 1)).user_id,
        "10000"
    );

    // 未配置测试群 → 不解包。
    cfg.gateway.accept_forged_messages = true;
    cfg.gateway.test_group_id = None;
    assert_eq!(
        gate_nc(&cfg, event("10000", "测试号", env, 1)).user_id,
        "10000"
    );
}

#[test]
fn forged_batch_unpacks_into_multiple() {
    use crate::round::RoundCategory;
    let mut cfg = base_config();
    cfg.gateway.accept_forged_messages = true;
    cfg.gateway.test_group_id = Some("123456789".to_string());
    cfg.round.category = RoundCategory::Test;
    cfg.round.group_id = "123456789".to_string();

    let text = r#"[{"id":"1","text":"排 通行证 1","timestamp":1000},{"id":"2","text":"排 人事部 1","timestamp":2000}]"#;
    let evs = gate_nc_multi(&cfg, event("99999", "外层", text, 5));
    assert_eq!(evs.len(), 2, "数组应解包为 2 条");
    assert_eq!(evs[0].user_id, "1");
    assert_eq!(evs[0].text, "排 通行证 1");
    assert_eq!(evs[0].timestamp_ms, 1000);
    assert_eq!(evs[1].user_id, "2");
    assert!(evs[0].message_id.contains('#'));
    // 非测试群 → 不解包（单条原样透传）。
    let plain = gate_nc_multi(&cfg, event("99999", "x", text, 5));
    cfg.gateway.test_group_id = Some("777".to_string());
    let not_test = gate_nc_multi(&cfg, event("99999", "x", text, 5));
    assert_eq!(not_test.len(), 1);
    let _ = plain;
}

#[test]
fn forged_batch_is_sorted_by_timestamp() {
    use crate::round::RoundCategory;
    let mut cfg = base_config();
    cfg.gateway.accept_forged_messages = true;
    cfg.gateway.test_group_id = Some("123456789".to_string());
    cfg.round.category = RoundCategory::Test;
    cfg.round.group_id = "123456789".to_string();

    let text = r#"[{"id":"b","text":"第二条","timestamp":2000},{"id":"a","text":"第一条","timestamp":1000}]"#;
    let evs = gate_nc_multi(&cfg, event("99999", "外层", text, 5));
    assert_eq!(evs.len(), 2);
    assert_eq!(evs[0].user_id, "a", "应按 timestamp 升序");
    assert_eq!(evs[0].timestamp_ms, 1000);
    assert_eq!(evs[1].user_id, "b");
    assert!(evs[0].message_id.ends_with("#0"));
    assert!(evs[1].message_id.ends_with("#1"));
}

#[tokio::test]
async fn anonymous_proxy_uses_sender_permission() {
    let mut cfg = base_config();
    cfg.llm.enabled = false;
    cfg.round.priority_users = vec!["1000".to_string()];
    cfg.round.priority_window = Some(PriorityWindow {
        start_ms: 0,
        end_ms: i64::MAX,
    });
    let (pipeline, _d) = test_pipeline_with_dir(&cfg, MockClient::ok("{}"));

    // 发送者 id「1000」是预存用户 → `小明(代)` 可排；席位填 `小明(代)`。
    let ok = pipeline
        .ingest(event("1000", "小明", "排 通行证 结城理 1(代)", 1_000))
        .await;
    assert_eq!(ok.status, "Applied", "{}", ok.detail);
    let snap = serde_json::to_string(&ok.snapshot).unwrap();
    assert!(snap.contains("小明(代)"), "席位应填 发送者(代)：{snap}");

    // 发送者 id「2000」非预存 → 代排项按发送者权限拒绝。
    let denied = pipeline
        .ingest(event("2000", "小红", "排 通行证 结城理 1(代)", 2_000))
        .await;
    assert_eq!(denied.status, "Rejected", "{}", denied.detail);
}
