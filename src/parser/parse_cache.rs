use std::collections::HashMap;
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

use crate::parser::parsed_event::ParsedMessage;
use crate::error::ParseError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseCacheEntry {
    pub raw_message_id: String,
    pub input_hash: String,
    pub parsed_message: ParsedMessage,
    pub parser_version: String,
    pub model_name: String,
}

pub struct ParseCache {
    entries: HashMap<String, ParseCacheEntry>,
}

impl ParseCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, raw_message_id: &str, input_hash: &str) -> Option<&ParseCacheEntry> {
        self.entries.get(raw_message_id).filter(|e| e.input_hash == input_hash)
    }

    pub fn insert(&mut self, entry: ParseCacheEntry) {
        self.entries.insert(entry.raw_message_id.clone(), entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn compute_input_hash(text: &str, group_id: &str, user_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(group_id.as_bytes());
    hasher.update(user_id.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub struct ParseContext {
    pub text: String,
    pub group_id: String,
    pub user_id: String,
    pub message_id: String,
}

impl ParseCacheEntry {
    pub fn new(
        raw_message_id: String,
        text: &str,
        group_id: &str,
        user_id: &str,
        parsed_message: ParsedMessage,
        parser_version: String,
        model_name: String,
    ) -> Self {
        Self {
            raw_message_id: raw_message_id.clone(),
            input_hash: compute_input_hash(text, group_id, user_id),
            parsed_message,
            parser_version,
            model_name,
        }
    }
}

pub async fn parse_with_mode(
    mode: &crate::replay::replay_options::ParserMode,
    llm_parser: &dyn crate::parser::llm_client::LlmClient,
    cache: &mut ParseCache,
    round_contexts: &[crate::domain::item::RoundContext],
    ctx: &ParseContext,
) -> Result<ParsedMessage, ParseError> {
    match mode {
        crate::replay::replay_options::ParserMode::LiveLlm => {
            let context = crate::parser::llm_client::ParseRequestContext {
                group_id: ctx.group_id.clone(),
                user_id: ctx.user_id.clone(),
                nickname: String::new(),
                message: ctx.text.clone(),
                active_rounds: round_contexts.to_vec(),
            };

            let request = crate::parser::llm_client::LlmParseRequest {
                system_prompt: crate::parser::parsed_event::SYSTEM_PROMPT.to_string(),
                user_payload: serde_json::to_value(&context).map_err(|e| ParseError::InvalidJson(e))?,
                temperature: 0.0,
                max_tokens: 2048,
            };

            let response = llm_parser.parse_message(request).await
                .map_err(|e| ParseError::Ambiguous(e.to_string()))?;
            Ok(response.parsed)
        }
        crate::replay::replay_options::ParserMode::CachedParse { .. } => {
            let input_hash = compute_input_hash(&ctx.text, &ctx.group_id, &ctx.user_id);
            cache.get(&ctx.message_id, &input_hash)
                .map(|e| e.parsed_message.clone())
                .ok_or(ParseError::CacheMiss)
        }
        crate::replay::replay_options::ParserMode::RuleOnly => {
            Err(ParseError::Ambiguous("RuleOnly parser not implemented".to_string()))
        }
        crate::replay::replay_options::ParserMode::HybridCachedThenLlm { cache_path: _ } => {
            let input_hash = compute_input_hash(&ctx.text, &ctx.group_id, &ctx.user_id);
            if let Some(entry) = cache.get(&ctx.message_id, &input_hash) {
                return Ok(entry.parsed_message.clone());
            }
            // Cache miss, fall back to LLM
            let context = crate::parser::llm_client::ParseRequestContext {
                group_id: ctx.group_id.clone(),
                user_id: ctx.user_id.clone(),
                nickname: String::new(),
                message: ctx.text.clone(),
                active_rounds: round_contexts.to_vec(),
            };

            let request = crate::parser::llm_client::LlmParseRequest {
                system_prompt: crate::parser::parsed_event::SYSTEM_PROMPT.to_string(),
                user_payload: serde_json::to_value(&context).map_err(|e| ParseError::InvalidJson(e))?,
                temperature: 0.0,
                max_tokens: 2048,
            };

            let response = llm_parser.parse_message(request).await
                .map_err(|e| ParseError::Ambiguous(e.to_string()))?;

            let entry = ParseCacheEntry::new(
                ctx.message_id.clone(),
                &ctx.text,
                &ctx.group_id,
                &ctx.user_id,
                response.parsed.clone(),
                String::new(),
                response.model.clone(),
            );
            cache.insert(entry);

            Ok(response.parsed)
        }
    }
}
