use super::*;
use crate::messages::JsonlMessageStore;
use crate::replay::session::ReplayOverrides;
use crate::round::{PhaseWindow, RoundPhase};
use crate::settings::{default_config, AppConfig, LlmSettings};
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

fn store(cfg: &AppConfig) -> Arc<ConfigStore> {
    let path =
        std::env::temp_dir().join(format!("paigu-llm-test-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
    Arc::new(ConfigStore::load(path).unwrap())
}

fn test_pipeline_with_dir(
    cfg: &AppConfig,
    llm: Arc<dyn LlmClient>,
) -> (Arc<Pipeline>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("paigu-msgs-test-{}", uuid::Uuid::new_v4()));
    (
        Pipeline::new_with_client_dir(store(cfg), llm, dir.clone()),
        dir,
    )
}

fn test_pipeline(cfg: &AppConfig, llm: Arc<dyn LlmClient>) -> Arc<Pipeline> {
    test_pipeline_with_dir(cfg, llm).0
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

fn slot_user(snapshot: &Value, item_id: &str, variant_id: &str) -> Option<String> {
    let allocations = snapshot.get("item_allocations")?.as_array()?;
    for allocation in allocations {
        if allocation.get("item_id").and_then(|v| v.as_str()) == Some(item_id)
            && allocation.get("variant_id").and_then(|v| v.as_str()) == Some(variant_id)
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
    let body = r#"{"intent":"claim","items":[{"item":"通行认证SP-月行水上","variant":"岳羽由加莉","quantity":1,"claim_type":"split","slot_policy":"normal"}],"confidence":0.95,"ambiguous_parts":[]}"#;
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
async fn bare_variant_first_match_resolves_to_catalog_first() {
    let body = r#"{"intent":"claim","items":[{"variant":"结城理","quantity":1,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
    let pipeline = test_pipeline(&base_config(), MockClient::ok(body));
    let outcome = pipeline
        .process(event("u1", "小明", "帮我留一份yukari", 1_000))
        .await;
    assert_eq!(outcome.status, "Applied");
    let who = pipeline.who_whats().await;
    assert_eq!(who[0]["items"][0]["name"], "通行认证SP-月行水上");
}

#[tokio::test]
async fn rule_bare_variant_first_match() {
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
    let parse_body = r#"{"intent":"claim","items":[{"variant":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body =
        r#"{"matches":[{"name":"悠人","item":"通行认证SP-月行水上","variant":"结城理"}]}"#;
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
        raw.iter().any(|v| v["payload"]["event_type"] == "ParseOverride"),
        "应落盘 ParseOverride 事件: {raw:?}"
    );
    assert!(
        raw.iter().any(|v| v["payload"]["target_raw_message_id"] == "u1::要一个悠人"),
        "ParseOverride 应指向原 message_id"
    );
}

/// §U7：重放消费 `ParseOverride`，与实时对同一消息结果一致。
#[tokio::test]
async fn replay_consumes_parse_override_matching_realtime() {
    let parse_body = r#"{"intent":"claim","items":[{"variant":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body =
        r#"{"matches":[{"name":"悠人","item":"通行认证SP-月行水上","variant":"结城理"}]}"#;
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
    let replayed =
        crate::replay::session::replay(log.as_ref(), &cfg, ReplayOverrides::default())
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
    let parse_body = r#"{"intent":"claim","items":[{"variant":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
    let clarify_body = r#"{"matches":[{"name":"悠人","item":"不存在的商品","variant":"谁"}]}"#;
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
    let parse_body = r#"{"intent":"claim","items":[{"variant":"悠人","quantity":1,"claim_type":"split"}],"confidence":0.9,"ambiguous_parts":[]}"#;
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
}
