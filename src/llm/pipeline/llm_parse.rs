use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use crate::bus::IncomingEvent;
use crate::domain::event::{DomainEvent, EventEnvelope, EventStatus, ParseOverrideEvent};
use crate::domain::ids::{EventId, RoundId, UserId};
use crate::domain::item::{Item, RoundContext};
use crate::llm::prompt;
use crate::parser::alias_match;
use crate::parser::parsed_event::{ParsedClaimItem, ParsedIntent, ParsedMessage};
use crate::settings::AppConfig;

use super::Pipeline;

impl Pipeline {
    pub(super) async fn llm_parse(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
    ) -> anyhow::Result<ParsedMessage> {
        let system = prompt::build_system_prompt(cfg);
        let user = prompt::build_user_prompt(ev);
        let raw = self.llm.complete(&cfg.llm, &system, &user).await?;
        let items = cfg.round.to_items();
        parse_llm_json(&raw, &items, &cfg.round.round_id)
    }
}

/// §U7 first-match 澄清结果。
pub(super) enum BareSubitemOutcome {
    /// 全部解析成功（含经 LLM 澄清修复）。
    Resolved(Box<ParsedMessage>, bool),
    /// 澄清失败/超时且 `fallback_to_rules`：按非排谷忽略。
    FallbackIgnore,
    /// 澄清失败/超时且未启用回退：按“没识别成功”拒绝。
    FallbackReject,
}

impl Pipeline {
    /// §U7：对未解析的「只报角色名/子项名」请求先做 first-match；全部 fail 时走 LLM 澄清。
    /// 澄清成功后发射并落盘 `ParseOverride` 事件（供重放消费）。
    pub(super) async fn resolve_bare_subitem_claims(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
        mut parsed: ParsedMessage,
        active_rounds: &[RoundContext],
        now: DateTime<Utc>,
        seq: i64,
    ) -> BareSubitemOutcome {
        // 先消解「只报角色名但多处命中」的伪歧义：取歧义片段首 token 做 §U7 first-match。
        absorb_ambiguous_bare_subitems(&mut parsed, active_rounds);
        // 仍有真歧义（如大类匹配到多个商品）时保留 NeedConfirm，不做 first-match 澄清。
        if !parsed.ambiguous_parts.is_empty() {
            return BareSubitemOutcome::Resolved(Box::new(parsed), false);
        }
        let mut unresolved: Vec<String> = Vec::new();
        for pi in &mut parsed.items {
            if pi.resolved_item_id.is_some() {
                continue;
            }
            match alias_match::first_match(&pi.name, active_rounds) {
                Some((rid, iid, vid)) => {
                    pi.resolved_round_id = Some(rid.0);
                    pi.resolved_item_id = Some(iid.0);
                    pi.resolved_subitem_id = vid;
                }
                None => unresolved.push(pi.name.clone()),
            }
        }
        if parsed.items.is_empty() || unresolved.is_empty() {
            return BareSubitemOutcome::Resolved(Box::new(parsed), false);
        }
        if !cfg.llm.enabled {
            return fallback(cfg.llm.fallback_to_rules);
        }
        match self
            .clarify_first_match(cfg, ev, &parsed, &unresolved, active_rounds)
            .await
        {
            Ok(Some(corrected)) => {
                if let Err(error) = self
                    .emit_parse_override(cfg, ev, seq, now, &corrected, "first-match LLM 澄清")
                    .await
                {
                    warn!("ParseOverride 事件写入失败: {error}");
                    return fallback(cfg.llm.fallback_to_rules);
                }
                BareSubitemOutcome::Resolved(Box::new(corrected), true)
            }
            Ok(None) | Err(_) => fallback(cfg.llm.fallback_to_rules),
        }
    }

    async fn clarify_first_match(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
        parsed: &ParsedMessage,
        unresolved: &[String],
        active_rounds: &[RoundContext],
    ) -> anyhow::Result<Option<ParsedMessage>> {
        let system = prompt::build_first_match_system_prompt();
        let user = prompt::build_first_match_user_prompt(cfg, &ev.text, unresolved);
        let raw = self.llm.complete(&cfg.llm, &system, &user).await?;
        let out: LlmClarify = serde_json::from_str(&crate::llm::extract_json(&raw))
            .map_err(|e| anyhow::anyhow!("澄清 JSON 解析失败: {e}"))?;

        let mut corrected = parsed.clone();
        for (index, name) in unresolved.iter().enumerate() {
            let Some((item_tok, subitem_tok)) = out.answer_for(index, name) else {
                return Ok(None);
            };
            let Some((rid, iid, vid)) =
                resolve_clarified(item_tok.as_deref(), subitem_tok.as_deref(), active_rounds)
            else {
                return Ok(None);
            };
            let Some(pi) = corrected
                .items
                .iter_mut()
                .find(|pi| pi.resolved_item_id.is_none() && pi.name == *name)
            else {
                return Ok(None);
            };
            pi.resolved_round_id = Some(rid.0);
            pi.resolved_item_id = Some(iid.0);
            pi.resolved_subitem_id = vid;
        }
        Ok(Some(corrected))
    }

    /// 把澄清修复结果写成 `ParseOverride` 事件并持久化到该轮次事件日志。
    pub(super) async fn emit_parse_override(
        &self,
        cfg: &AppConfig,
        ev: &IncomingEvent,
        seq: i64,
        now: DateTime<Utc>,
        corrected: &ParsedMessage,
        reason: &str,
    ) -> anyhow::Result<()> {
        let round_id = RoundId(cfg.round.round_id.clone());
        let event = EventEnvelope {
            event_id: EventId(uuid::Uuid::new_v4().to_string()),
            round_id: round_id.clone(),
            group_id: ev.group_id.clone(),
            user_id: UserId(ev.user_id.clone()),
            raw_message_id: Some(ev.message_id.clone()),
            event_type: "parse_override".to_string(),
            effective_at: now,
            sequence: seq,
            payload: DomainEvent::ParseOverride(ParseOverrideEvent {
                event_id: EventId(uuid::Uuid::new_v4().to_string()),
                round_id,
                target_raw_message_id: ev.message_id.clone(),
                target_record_id: None,
                target_record_revision: None,
                corrected_parsed_message: serde_json::to_value(corrected).unwrap_or(Value::Null),
                admin_user_id: UserId(ev.user_id.clone()),
                reason: reason.to_string(),
                occurred_at: now,
            }),
            status: EventStatus::Active,
        };
        let value = serde_json::to_value(&event)?;
        self.messages
            .append_raw_event(&cfg.round.round_id, &value)
            .await
    }
}

/// 取歧义片段的「首 token」（到首个分隔符为止），如 `克莱门莎（可能为：…）` → `克莱门莎`。
fn leading_token(part: &str) -> Option<&str> {
    let end = part
        .find(|c: char| {
            matches!(
                c,
                '（' | '(' | '：' | ':' | '，' | ',' | '、' | '。' | ' ' | '\t'
            )
        })
        .unwrap_or(part.len());
    let tok = part[..end].trim();
    (!tok.is_empty()).then_some(tok)
}

/// §U7 与 LLM 输出调和：LLM 常把「只报角色名、命中多个商品」判为歧义；此处对每个歧义片段
/// 取首 token 做 first-match（命中**可拼团**商品即采用），从而把伪歧义消化为认领；只有
/// 确实无解（如错别字）才保留为真歧义。
fn absorb_ambiguous_bare_subitems(parsed: &mut ParsedMessage, active_rounds: &[RoundContext]) {
    if parsed.intent != ParsedIntent::Claim || parsed.ambiguous_parts.is_empty() {
        return;
    }
    let parts = std::mem::take(&mut parsed.ambiguous_parts);
    let total = parts.len();
    let mut kept: Vec<String> = Vec::new();
    for part in parts {
        let Some(token) = leading_token(&part) else {
            kept.push(part);
            continue;
        };
        let Some((rid, iid, vid)) = alias_match::first_match(token, active_rounds) else {
            kept.push(part);
            continue;
        };
        // 优先复用同名的未解析项，避免重复计数；否则新增。
        if let Some(pi) = parsed
            .items
            .iter_mut()
            .find(|pi| pi.resolved_item_id.is_none() && pi.name.trim() == token)
        {
            pi.resolved_round_id = Some(rid.0);
            pi.resolved_item_id = Some(iid.0);
            pi.resolved_subitem_id = vid;
        } else if !parsed.items.iter().any(|pi| {
            pi.resolved_item_id.as_deref() == Some(iid.0.as_str()) && pi.resolved_subitem_id == vid
        }) {
            parsed.items.push(ParsedClaimItem {
                name: token.to_string(),
                category_hint: None,
                quantity: 1,
                claim_type: None,
                is_proxy_card: None,
                slot_policy: None,
                notes: None,
                resolved_item_id: Some(iid.0),
                resolved_subitem_id: vid,
                resolved_round_id: Some(rid.0),
            });
        }
    }
    let kept_len = kept.len();
    parsed.ambiguous_parts = kept;
    // 有片段经 first-match 消解 → 视为确定性解析（提升置信度，避免误判"置信度不足"）。
    if kept_len < total && parsed.confidence < 0.9 {
        parsed.confidence = 0.9;
    }
}

fn fallback(fallback_to_rules: bool) -> BareSubitemOutcome {
    if fallback_to_rules {
        BareSubitemOutcome::FallbackIgnore
    } else {
        BareSubitemOutcome::FallbackReject
    }
}

/// 在可拼团目录内解析澄清结果：优先精确商品+子项，退化为按子项 first-match。
fn resolve_clarified(
    item_tok: Option<&str>,
    subitem_tok: Option<&str>,
    active_rounds: &[RoundContext],
) -> Option<(RoundId, crate::domain::ids::ItemId, Option<String>)> {
    let item_tok = item_tok.map(str::trim).filter(|s| !s.is_empty());
    let subitem_tok = subitem_tok.map(str::trim).filter(|s| !s.is_empty());
    for round in active_rounds {
        for item in &round.items {
            if !alias_match::is_splittable(item) {
                continue;
            }
            let item_ok = match item_tok {
                Some(t) => item.exact_matches_name(t) || item.item_id.0 == t,
                None => true,
            };
            if !item_ok {
                continue;
            }
            match subitem_tok {
                Some(v) => {
                    if let Some(found) = item.find_subitem_by_name(v) {
                        return Some((
                            round.round_id.clone(),
                            item.item_id.clone(),
                            Some(found.subitem_id.clone()),
                        ));
                    }
                }
                None => return Some((round.round_id.clone(), item.item_id.clone(), None)),
            }
        }
    }
    if let Some(v) = subitem_tok {
        return alias_match::first_match(v, active_rounds);
    }
    None
}

#[derive(Deserialize)]
struct LlmClarify {
    #[serde(default)]
    matches: Vec<LlmClarifyMatch>,
    #[serde(default)]
    item: Option<String>,
    #[serde(default)]
    subitem: Option<String>,
}

#[derive(Deserialize)]
struct LlmClarifyMatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    item: Option<String>,
    #[serde(default)]
    subitem: Option<String>,
}

impl LlmClarify {
    fn answer_for(&self, index: usize, name: &str) -> Option<(Option<String>, Option<String>)> {
        if let Some(m) = self
            .matches
            .iter()
            .find(|m| m.name.as_deref() == Some(name))
        {
            return Some((m.item.clone(), m.subitem.clone()));
        }
        if let Some(m) = self.matches.get(index) {
            return Some((m.item.clone(), m.subitem.clone()));
        }
        if index == 0 && (self.item.is_some() || self.subitem.is_some()) {
            return Some((self.item.clone(), self.subitem.clone()));
        }
        None
    }
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
    subitem: Option<String>,
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
    let json_text = crate::llm::extract_json(raw);
    let out: LlmOut = serde_json::from_str(&json_text).map_err(|e| {
        anyhow::anyhow!(
            "LLM JSON 解析失败: {e}; raw={}",
            crate::llm::truncate(raw, 200)
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

fn resolve_llm_item(
    item: &LlmItem,
    items: &[Item],
    round_id: &str,
) -> (ParsedClaimItem, Option<String>) {
    let item_tok = item
        .item
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            item.name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("");
    let subitem_tok = item
        .subitem
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
        match subitem_tok {
            None => candidates.push((candidate, None)),
            Some(subitem) => {
                if let Some(found) = candidate.find_subitem_by_name(subitem) {
                    candidates.push((candidate, Some(found.subitem_id.clone())));
                }
            }
        }
    }

    if candidates.is_empty() {
        if let Some(subitem) = subitem_tok {
            for candidate in items {
                if let Some(found) = candidate.find_subitem_by_name(subitem) {
                    candidates.push((candidate, Some(found.subitem_id.clone())));
                }
            }
        }
    }

    let name = subitem_tok.unwrap_or(item_tok).to_string();
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
        resolved_subitem_id: None,
        resolved_round_id: None,
    };

    if candidates.len() == 1 {
        let (candidate, subitem_id) = &candidates[0];
        parsed.resolved_item_id = Some(candidate.item_id.0.clone());
        parsed.resolved_subitem_id = subitem_id.clone();
        parsed.resolved_round_id = Some(round_id.to_string());
        (parsed, None)
    } else if candidates.is_empty() {
        (parsed, None)
    } else if item_tok.is_empty() {
        // §U7：只报角色名/子项名（未报大类）→ 按目录顺序 first-match 到第一个可拼团商品。
        match candidates
            .iter()
            .find(|(candidate, _)| alias_match::is_splittable(candidate))
        {
            Some((candidate, subitem_id)) => {
                parsed.resolved_item_id = Some(candidate.item_id.0.clone());
                parsed.resolved_subitem_id = subitem_id.clone();
                parsed.resolved_round_id = Some(round_id.to_string());
                (parsed, None)
            }
            None => {
                let names: Vec<String> = candidates.iter().map(|(c, _)| c.name.clone()).collect();
                (parsed, Some(format!("商品歧义：{}", names.join("、"))))
            }
        }
    } else {
        let names: Vec<String> = candidates.iter().map(|(c, _)| c.name.clone()).collect();
        (parsed, Some(format!("商品歧义：{}", names.join("、"))))
    }
}
