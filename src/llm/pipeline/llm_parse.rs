use serde::Deserialize;

use crate::bus::IncomingEvent;
use crate::domain::item::Item;
use crate::llm::prompt;
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
