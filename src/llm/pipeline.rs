use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::warn;

use crate::bus::{EventSink, IncomingEvent, PipelineOutcome};
use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::domain::ids::{EligibilityId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::allocation_engine::AllocationEngine;
use crate::engine::event_store::{EventStore, InMemoryEventStore};
use crate::engine::replay::ReplayService;
use crate::parser::parsed_event::{ParsedClaimItem, ParsedIntent, ParsedMessage};
use crate::parser::rule_parser::RuleParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::settings::{AppConfig, ConfigStore};

use super::client::{LlmClient, OpenAiClient};
use super::prompt;

const RULE_CONFIDENCE: f32 = 0.9;
const CONFIDENCE_THRESHOLD: f32 = 0.65;

pub struct Pipeline {
    cfg: Arc<ConfigStore>,
    llm: Arc<dyn LlmClient>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    events: Vec<EventEnvelope>,
    messages: Vec<MessageRecord>,
    seen: HashSet<String>,
    eligibilities: Vec<Eligibility>,
    display: std::collections::HashMap<String, String>,
    identity: std::collections::HashMap<String, String>,
    seq: i64,
    version: i64,
    snapshot: Option<AllocationSnapshot>,
}

struct MessageRecord {
    seq: i64,
    user_id: String,
    display: String,
    text: String,
    status: String,
    detail: String,
    timestamp_ms: i64,
}

impl Pipeline {
    pub fn new(cfg: Arc<ConfigStore>) -> Arc<Self> {
        Self::new_with_client(cfg, Arc::new(OpenAiClient::new()))
    }

    pub fn new_with_client(cfg: Arc<ConfigStore>, llm: Arc<dyn LlmClient>) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            llm,
            state: Mutex::new(State::default()),
        })
    }

    pub async fn process(&self, ev: IncomingEvent) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        let (identity, display_raw) = sanitize_identity(&ev.nickname);
        let display = if display_raw.trim().is_empty() {
            ev.user_id.clone()
        } else {
            display_raw
        };

        let seq = {
            let mut state = self.state.lock().await;
            let key = format!("{}::{}", ev.group_id, ev.message_id);
            if !state.seen.insert(key) {
                let detail = "重复 message_id，已按幂等忽略";
                let seq = state.seq;
                state.messages.push(MessageRecord {
                    seq,
                    user_id: ev.user_id.clone(),
                    display: display.clone(),
                    text: ev.text.clone(),
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                    timestamp_ms: ev.timestamp_ms,
                });
                return PipelineOutcome {
                    status: "Duplicate".to_string(),
                    detail: detail.to_string(),
                    reply: None,
                    version: state.version,
                    snapshot: state
                        .snapshot
                        .as_ref()
                        .and_then(|s| serde_json::to_value(s).ok()),
                };
            }
            state.seq += 1;
            state.display.insert(ev.user_id.clone(), display.clone());
            state.identity.insert(ev.user_id.clone(), identity.clone());
            state.seq
        };

        let text = ev.text.trim().to_string();

        if text.is_empty() {
            return self
                .finish(&ev, &display, seq, "Ignored", "空消息", None)
                .await;
        }

        if text.starts_with('/') {
            if !ev.is_admin {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Rejected",
                        "非管理员斜杠命令",
                        Some("此命令仅管理员可用。".to_string()),
                    )
                    .await;
            }
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Applied",
                    "管理员命令已记录",
                    Some("管理员命令已记录".to_string()),
                )
                .await;
        }

        let items = cfg.round.to_items();
        let rule = RuleParser::parse(&text, &items, ev.is_admin);

        let parsed = if rule.confidence >= RULE_CONFIDENCE && rule.intent != ParsedIntent::Unknown {
            rule
        } else if cfg.llm.enabled {
            match self.llm_parse(&cfg, &ev).await {
                Ok(parsed) => parsed,
                Err(e) => {
                    if cfg.llm.fallback_to_rules {
                        warn!("LLM 解析失败，回退规则解析器: {e}");
                        rule
                    } else {
                        return self
                            .finish(
                                &ev,
                                &display,
                                seq,
                                "Rejected",
                                "没识别成功",
                                Some("没识别成功".to_string()),
                            )
                            .await;
                    }
                }
            }
        } else if cfg.llm.fallback_to_rules {
            rule
        } else {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "LLM 未启用",
                    Some("没识别成功".to_string()),
                )
                .await;
        };

        if parsed.intent == ParsedIntent::Modify {
            return self
                .finish(&ev, &display, seq, "Ignored", "改单功能暂未实现", None)
                .await;
        }

        if parsed.intent == ParsedIntent::Claim
            && parsed.items.is_empty()
            && parsed.ambiguous_parts.is_empty()
        {
            return self
                .finish(&ev, &display, seq, "Ignored", "未解析出商品", None)
                .await;
        }

        let round_id = RoundId(cfg.round.round_id.clone());
        let round_contexts = vec![RoundContext {
            round_id: round_id.clone(),
            title: cfg.round.title.clone(),
            items: items.clone(),
        }];
        let now = DateTime::<Utc>::from_timestamp_millis(ev.timestamp_ms).unwrap_or_else(Utc::now);
        let user_id = UserId(ev.user_id.clone());
        let validator = EventValidator::new(CONFIDENCE_THRESHOLD);

        let event = match validator
            .validate(
                parsed,
                &user_id,
                &ev.group_id,
                Some(ev.message_id.clone()),
                &round_contexts,
                now,
                seq,
            )
            .await
        {
            Ok(ValidationOutcome::Ok(event)) => event,
            Ok(ValidationOutcome::NeedConfirm(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "NeedConfirm", "需要确认", reply)
                    .await;
            }
            Ok(ValidationOutcome::Reject(reply)) => {
                let reply = reply.text_content().map(str::to_string);
                return self
                    .finish(&ev, &display, seq, "Rejected", "校验拒绝", reply)
                    .await;
            }
            Ok(ValidationOutcome::Ignore) => {
                return self
                    .finish(
                        &ev,
                        &display,
                        seq,
                        "Ignored",
                        "无法识别为排谷/撤销意图",
                        None,
                    )
                    .await;
            }
            Err(e) => {
                return self
                    .finish(&ev, &display, seq, "Error", &format!("处理失败: {e}"), None)
                    .await;
            }
        };

        let is_priority = is_priority_user(&cfg, &ev, &identity, &display);
        if in_priority_window(&cfg, ev.timestamp_ms) && !is_priority {
            return self
                .finish(
                    &ev,
                    &display,
                    seq,
                    "Rejected",
                    "优先时段仅限预存(购物金)用户",
                    Some("优先时段仅限预存(购物金)用户，请求已拒绝".to_string()),
                )
                .await;
        }

        let detail = describe_event(&event);
        let (version, snapshot_value, reply) = {
            let mut state = self.state.lock().await;
            if is_priority && !state.eligibilities.iter().any(|e| e.user_id.0 == ev.user_id) {
                state
                    .eligibilities
                    .push(priority_eligibility(&round_id, &ev.user_id));
            }
            state.events.push(event);
            let (version, snapshot) = rebuild(&items, &state.events, &state.eligibilities);
            let snapshot_value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
            state.version = version;
            state.snapshot = Some(snapshot);
            state.messages.push(MessageRecord {
                seq,
                user_id: ev.user_id.clone(),
                display: display.clone(),
                text: ev.text.clone(),
                status: "Applied".to_string(),
                detail: detail.clone(),
                timestamp_ms: ev.timestamp_ms,
            });
            (version, snapshot_value, format!("已记录，当前版本 #{}", version))
        };

        PipelineOutcome {
            status: "Applied".to_string(),
            detail,
            reply: Some(reply),
            version,
            snapshot: Some(snapshot_value),
        }
    }

    pub async fn reset(&self) {
        let mut state = self.state.lock().await;
        *state = State::default();
    }

    pub async fn board(&self) -> (i64, Value) {
        let state = self.state.lock().await;
        let snapshot = state
            .snapshot
            .as_ref()
            .and_then(|s| serde_json::to_value(s).ok())
            .unwrap_or(Value::Null);
        (state.version, snapshot)
    }

    pub async fn messages_since(&self, since: i64) -> Vec<Value> {
        let state = self.state.lock().await;
        state
            .messages
            .iter()
            .filter(|m| m.seq > since)
            .map(|m| {
                json!({
                    "seq": m.seq,
                    "display": m.display,
                    "text": m.text,
                    "status": m.status,
                    "detail": m.detail,
                })
            })
            .collect()
    }

    pub async fn who_whats(&self) -> Vec<Value> {
        let state = self.state.lock().await;
        let Some(snapshot) = state.snapshot.as_ref() else {
            return Vec::new();
        };

        let mut grouped: BTreeMap<String, (String, BTreeMap<String, u32>)> = BTreeMap::new();
        for summary in &snapshot.user_summaries {
            let uid = summary.user_id.0.clone();
            let display = state.display.get(&uid).cloned().unwrap_or_else(|| uid.clone());
            let identity = state.identity.get(&uid).cloned().unwrap_or_else(|| display.clone());
            let entry = grouped
                .entry(display)
                .or_insert_with(|| (identity, BTreeMap::new()));
            for item in &summary.items {
                *entry.1.entry(item.item_name.clone()).or_insert(0) += item.quantity;
            }
        }

        grouped
            .into_iter()
            .map(|(display, (identity, items))| {
                let items: Vec<Value> = items
                    .into_iter()
                    .map(|(name, qty)| json!({ "name": name, "qty": qty }))
                    .collect();
                json!({ "display": display, "identity": identity, "items": items })
            })
            .collect()
    }

    async fn finish(
        &self,
        ev: &IncomingEvent,
        display: &str,
        seq: i64,
        status: &str,
        detail: &str,
        reply: Option<String>,
    ) -> PipelineOutcome {
        let mut state = self.state.lock().await;
        state.messages.push(MessageRecord {
            seq,
            user_id: ev.user_id.clone(),
            display: display.to_string(),
            text: ev.text.clone(),
            status: status.to_string(),
            detail: detail.to_string(),
            timestamp_ms: ev.timestamp_ms,
        });
        PipelineOutcome {
            status: status.to_string(),
            detail: detail.to_string(),
            reply,
            version: state.version,
            snapshot: state
                .snapshot
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
        }
    }

    async fn llm_parse(&self, cfg: &AppConfig, ev: &IncomingEvent) -> anyhow::Result<ParsedMessage> {
        let system = prompt::build_system_prompt(cfg);
        let user = prompt::build_user_prompt(ev);
        let raw = self.llm.complete(&cfg.llm, &system, &user).await?;
        let items = cfg.round.to_items();
        parse_llm_json(&raw, &items, &cfg.round.round_id)
    }
}

#[async_trait]
impl EventSink for Pipeline {
    async fn handle(&self, ev: IncomingEvent) {
        self.process(ev).await;
    }
}

fn rebuild(
    items: &[Item],
    events: &[EventEnvelope],
    eligibilities: &[Eligibility],
) -> (i64, AllocationSnapshot) {
    let mut sorted = events.to_vec();
    sorted.sort_by(crate::domain::event::compare_event_order);
    let store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let service = ReplayService::new(store);
    let lines = service.collect_effective_claims(&sorted, eligibilities);
    let engine = AllocationEngine::new();
    let mut snapshot = engine
        .allocate(items, &lines, &sorted)
        .unwrap_or_else(|_| empty_snapshot(items));
    snapshot.version = sorted.len() as i64;
    (snapshot.version, snapshot)
}

fn empty_snapshot(items: &[Item]) -> AllocationSnapshot {
    AllocationSnapshot {
        round_id: items
            .first()
            .map(|i| i.round_id.clone())
            .unwrap_or_else(|| RoundId("unknown".to_string())),
        version: 0,
        generated_at: Utc::now(),
        item_allocations: vec![],
        user_summaries: vec![],
        warnings: vec![],
    }
}

fn describe_event(event: &EventEnvelope) -> String {
    match &event.payload {
        DomainEvent::ClaimCreated(c) => {
            let items: Vec<String> = c
                .items
                .iter()
                .map(|l| format!("{}x{}[{}]", l.item_id.0, l.quantity, l.slot_policy.as_str()))
                .collect();
            format!("claim: {}", items.join(", "))
        }
        DomainEvent::ClaimCancelled(c) => format!(
            "cancel: item={:?} qty={:?}",
            c.target_item_id.as_ref().map(|i| i.0.clone()),
            c.quantity
        ),
        other => format!("event: {}", other.event_type_str()),
    }
}

fn priority_eligibility(round_id: &RoundId, user_id: &str) -> Eligibility {
    Eligibility {
        eligibility_id: EligibilityId(uuid::Uuid::new_v4().to_string()),
        round_id: round_id.clone(),
        user_id: UserId(user_id.to_string()),
        priority_type: "shopping_fund".to_string(),
        priority_level: 10,
        scope: EligibilityScope {
            item_ids: None,
            item_kinds: None,
            only_before_start_minutes: None,
        },
        max_uses: None,
        used_count: 0,
        valid_from: Some(DateTime::<Utc>::from_timestamp_millis(0).unwrap_or_else(Utc::now)),
        valid_until: None,
        note: Some("预存(购物金)用户".to_string()),
    }
}

fn in_priority_window(cfg: &AppConfig, timestamp_ms: i64) -> bool {
    match &cfg.round.priority_window {
        Some(window) => timestamp_ms >= window.start_ms && timestamp_ms < window.end_ms,
        None => false,
    }
}

fn is_priority_user(cfg: &AppConfig, ev: &IncomingEvent, identity: &str, display: &str) -> bool {
    cfg.round.priority_users.iter().any(|u| {
        let u = u.trim();
        u == ev.user_id || u == ev.nickname || u == identity || u == display
    })
}

fn sanitize_identity(nickname: &str) -> (String, String) {
    let normalized = fullwidth_to_half(nickname);
    let trimmed = normalized.trim();

    if let Some(pos) = trimmed.find("(代") {
        let identity = trimmed[..pos].trim().to_string();
        let rest = &trimmed[pos + 1..];
        if let Some(close) = rest.find(')') {
            let display = format!("{}({})", identity, &rest[..close]);
            return (identity, display);
        }
    }

    let identity = match trimmed.find('(') {
        Some(pos) => trimmed[..pos].trim().to_string(),
        None => trimmed.to_string(),
    };
    (identity.clone(), identity)
}

fn fullwidth_to_half(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let mapped = match ch {
            '０'..='９' => char::from_u32(ch as u32 - 0xFF10 + '0' as u32).unwrap_or(ch),
            'Ａ'..='Ｚ' => char::from_u32(ch as u32 - 0xFF21 + 'A' as u32).unwrap_or(ch),
            'ａ'..='ｚ' => char::from_u32(ch as u32 - 0xFF41 + 'a' as u32).unwrap_or(ch),
            '：' => ':',
            '（' => '(',
            '）' => ')',
            '　' => ' ',
            _ => ch,
        };
        out.push(mapped);
    }
    out
}

#[derive(Deserialize)]
struct LlmOut {
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    items: Vec<LlmItem>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    ambiguous_parts: Vec<String>,
    #[serde(default)]
    cancel_target: Option<String>,
}

#[derive(Deserialize)]
struct LlmItem {
    #[serde(default)]
    item: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    quantity: Option<u32>,
    #[serde(default)]
    claim_type: Option<String>,
    #[serde(default)]
    slot_policy: Option<String>,
    #[serde(default)]
    is_proxy_card: Option<bool>,
    #[serde(default)]
    notes: Option<String>,
}

fn parse_llm_json(raw: &str, items: &[Item], round_id: &str) -> anyhow::Result<ParsedMessage> {
    let json_text = extract_json(raw);
    let out: LlmOut = serde_json::from_str(&json_text).map_err(|e| {
        anyhow::anyhow!(
            "LLM JSON 解析失败: {e}; raw={}",
            truncate(raw, 200)
        )
    })?;

    let intent = match out
        .intent
        .as_deref()
        .unwrap_or("unknown")
        .to_ascii_lowercase()
        .as_str()
    {
        "claim" => ParsedIntent::Claim,
        "cancel" => ParsedIntent::Cancel,
        "modify" => ParsedIntent::Modify,
        "admin" | "admincommand" | "admin_command" => ParsedIntent::AdminCommand,
        _ => ParsedIntent::Unknown,
    };

    let mut parsed_items = Vec::new();
    let mut ambiguous = out.ambiguous_parts;
    for item in &out.items {
        let (parsed_item, ambiguity) = resolve_llm_item(item, items, round_id);
        if let Some(message) = ambiguity {
            ambiguous.push(message);
        }
        parsed_items.push(parsed_item);
    }

    Ok(ParsedMessage {
        intent,
        round_hint: None,
        items: parsed_items,
        cancel_target_hint: out.cancel_target,
        admin_command: None,
        confidence: out.confidence.unwrap_or(0.9),
        ambiguous_parts: ambiguous,
    })
}

fn resolve_llm_item(item: &LlmItem, items: &[Item], round_id: &str) -> (ParsedClaimItem, Option<String>) {
    let item_tok = item
        .item
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| item.name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("");
    let variant_tok = item
        .variant
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut candidates: Vec<(&Item, Option<String>)> = Vec::new();
    for candidate in items {
        let item_ok = item_tok.is_empty()
            || candidate.exact_matches_name(item_tok)
            || candidate.item_id.0 == item_tok
            || candidate.matches_name_or_alias(item_tok);
        if !item_ok {
            continue;
        }
        match variant_tok {
            None => candidates.push((candidate, None)),
            Some(variant) => {
                if let Some(found) = candidate.find_variant_by_name(variant) {
                    candidates.push((candidate, Some(found.variant_id.clone())));
                }
            }
        }
    }

    if candidates.is_empty() {
        if let Some(variant) = variant_tok {
            for candidate in items {
                if let Some(found) = candidate.find_variant_by_name(variant) {
                    candidates.push((candidate, Some(found.variant_id.clone())));
                }
            }
        }
    }

    let name = variant_tok.unwrap_or(item_tok).to_string();
    let mut parsed = ParsedClaimItem {
        name,
        category_hint: if item_tok.is_empty() {
            None
        } else {
            Some(item_tok.to_string())
        },
        quantity: item.quantity.unwrap_or(1),
        claim_type: item.claim_type.clone(),
        is_proxy_card: item.is_proxy_card,
        slot_policy: item.slot_policy.clone(),
        notes: item.notes.clone(),
        resolved_item_id: None,
        resolved_variant_id: None,
        resolved_round_id: None,
    };

    if candidates.len() == 1 {
        let (candidate, variant_id) = &candidates[0];
        parsed.resolved_item_id = Some(candidate.item_id.0.clone());
        parsed.resolved_variant_id = variant_id.clone();
        parsed.resolved_round_id = Some(round_id.to_string());
        (parsed, None)
    } else if candidates.is_empty() {
        (parsed, None)
    } else {
        let names: Vec<String> = candidates.iter().map(|(c, _)| c.name.clone()).collect();
        (parsed, Some(format!("商品歧义：{}", names.join("、"))))
    }
}

fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (stripped.find('{'), stripped.rfind('}')) {
        (Some(start), Some(end)) if end > start => stripped[start..=end].to_string(),
        _ => stripped.to_string(),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{default_config, LlmSettings};

    struct MockClient {
        reply: std::sync::Mutex<MockReply>,
    }

    enum MockReply {
        Ok(String),
        Err(String),
    }

    impl MockClient {
        fn ok(body: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: std::sync::Mutex::new(MockReply::Ok(body.to_string())),
            })
        }

        fn err(message: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: std::sync::Mutex::new(MockReply::Err(message.to_string())),
            })
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
            match &*self.reply.lock().unwrap() {
                MockReply::Ok(body) => Ok(body.clone()),
                MockReply::Err(message) => Err(anyhow::anyhow!(message.clone())),
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
        let path = std::env::temp_dir().join(format!(
            "paigu-llm-test-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
        Arc::new(ConfigStore::load(path).unwrap())
    }

    fn event(user_id: &str, nickname: &str, text: &str, timestamp_ms: i64) -> IncomingEvent {
        IncomingEvent {
            group_id: "720675572".to_string(),
            user_id: user_id.to_string(),
            nickname: nickname.to_string(),
            message_id: format!("{}::{}", user_id, text),
            text: text.to_string(),
            timestamp_ms,
            is_admin: false,
            raw: Value::Null,
        }
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
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::ok("{}"));
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
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::ok(body));
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
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::ok(body));
        let outcome = pipeline
            .process(event("u1", "小明", "今天天气不错", 1_000))
            .await;
        assert_eq!(outcome.status, "Ignored");
    }

    #[tokio::test]
    async fn ambiguous_need_confirm() {
        let body = r#"{"intent":"claim","items":[{"variant":"结城理","quantity":1,"claim_type":"split"}],"confidence":0.95,"ambiguous_parts":[]}"#;
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::ok(body));
        let outcome = pipeline
            .process(event("u1", "小明", "帮我留一份yukari", 1_000))
            .await;
        assert_eq!(outcome.status, "NeedConfirm");
    }

    #[tokio::test]
    async fn priority_window_rejected() {
        let mut cfg = base_config();
        cfg.round.priority_window = Some(crate::settings::PriorityWindow {
            start_ms: 0,
            end_ms: i64::MAX,
        });
        let pipeline = Pipeline::new_with_client(store(&cfg), MockClient::ok("{}"));
        let outcome = pipeline
            .process(event("u1", "小明", "排 通行证 结城理 1", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert!(outcome.detail.contains("预存"));
    }

    #[tokio::test]
    async fn priority_user_sorts_first() {
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::ok("{}"));
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
        let pipeline = Pipeline::new_with_client(store(&base_config()), MockClient::err("timeout"));
        let outcome = pipeline
            .process(event("u1", "小明", "嗯嗯好的", 1_000))
            .await;
        assert_eq!(outcome.status, "Ignored");
    }

    #[tokio::test]
    async fn llm_failure_without_fallback_rejected() {
        let mut cfg = base_config();
        cfg.llm.fallback_to_rules = false;
        let pipeline = Pipeline::new_with_client(store(&cfg), MockClient::err("timeout"));
        let outcome = pipeline
            .process(event("u1", "小明", "嗯嗯好的", 1_000))
            .await;
        assert_eq!(outcome.status, "Rejected");
        assert_eq!(outcome.detail, "没识别成功");
    }
}
