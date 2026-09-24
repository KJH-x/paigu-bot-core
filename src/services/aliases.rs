//! §U6 别名「LLM 建议」：**所有商品一次请求**；解析失败/超时降级为 HTTP 200。
//!
//! 契约（前端已按此实现）：
//! req  `{ items: [{item_id, name, aliases: string[]}], mode: "aliases" }`
//! resp `{ suggestions: [{item_id, aliases: string[], verdict: "filled"|"best"}] }`
//! 降级 `{ suggestions: [], degraded: true, message: "..." }`（仍 200）。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::ApiState;
use crate::llm::client::LlmClient;
use crate::llm::prompt::{self, AliasPromptItem};
use crate::settings::LlmSettings;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AliasSuggestItem {
    pub item_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SuggestAliasesBody {
    #[serde(default)]
    pub items: Vec<AliasSuggestItem>,
    /// 目前仅 `"aliases"`；保留字段以匹配前端契约。
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct AliasSuggestion {
    item_id: String,
    aliases: Vec<String>,
    verdict: &'static str,
}

#[derive(Debug, Default, Deserialize)]
struct LlmAliasOut {
    #[serde(default)]
    suggestions: Vec<LlmAliasSuggestion>,
}

#[derive(Debug, Default, Deserialize)]
struct LlmAliasSuggestion {
    #[serde(default, alias = "id")]
    item_id: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    verdict: Option<String>,
}

/// `POST /api/items/suggest-aliases`：调用共享注入的 LLM 客户端；任何失败都降级、不 5xx。
pub async fn suggest_aliases(state: &ApiState, body: SuggestAliasesBody) -> Value {
    let cfg = state.cfg.get().await;
    if !cfg.llm.enabled {
        return degraded("LLM 未启用");
    }
    let client = state.pipeline.llm_client();
    suggest_aliases_with(client.as_ref(), &cfg.llm, body).await
}

/// 可注入客户端版本（单测用 MockClient，不联网）。
pub async fn suggest_aliases_with(
    client: &dyn LlmClient,
    settings: &LlmSettings,
    body: SuggestAliasesBody,
) -> Value {
    if let Some(mode) = body.mode.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        if !mode.eq_ignore_ascii_case("aliases") {
            return degraded(format!("不支持的 mode：{mode}"));
        }
    }
    if body.items.is_empty() {
        return json!({ "suggestions": [] });
    }
    let prompt_items: Vec<AliasPromptItem<'_>> = body
        .items
        .iter()
        .map(|it| AliasPromptItem {
            item_id: it.item_id.as_str(),
            name: it.name.as_str(),
            aliases: &it.aliases,
        })
        .collect();
    let system = prompt::build_alias_system_prompt();
    let user = prompt::build_alias_user_prompt(&prompt_items);

    let timeout_secs = settings.timeout_secs.max(1);
    let raw = match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        client.complete(settings, &system, &user),
    )
    .await
    {
        Ok(Ok(raw)) => raw,
        Ok(Err(error)) => return degraded(format!("LLM 调用失败：{error}")),
        Err(_) => return degraded("LLM 请求超时"),
    };

    match parse_suggestions(&raw, &body.items) {
        Some(suggestions) => json!({ "suggestions": suggestions }),
        None => degraded("LLM 返回无法解析为建议 JSON"),
    }
}

fn degraded(message: impl Into<String>) -> Value {
    json!({ "suggestions": [], "degraded": true, "message": message.into() })
}

/// 解析模型输出；只保留请求中出现的 `item_id`（按请求顺序）。
fn parse_suggestions(raw: &str, items: &[AliasSuggestItem]) -> Option<Vec<AliasSuggestion>> {
    let out: LlmAliasOut = serde_json::from_str(&crate::llm::extract_json(raw)).ok()?;
    let mut result = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let target = item.item_id.trim();
        let found = out
            .suggestions
            .iter()
            .find(|s| s.item_id.as_deref().map(str::trim) == Some(target))
            .or_else(|| out.suggestions.get(idx).filter(|s| s.item_id.is_none()));
        let Some(found) = found else { continue };

        let verdict = match found
            .verdict
            .as_deref()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("best") => "best",
            _ => "filled",
        };
        let source = if found.aliases.is_empty() {
            item.aliases.as_slice()
        } else {
            found.aliases.as_slice()
        };
        let aliases = dedupe_aliases(source);
        if verdict == "filled" && aliases.is_empty() {
            continue;
        }
        result.push(AliasSuggestion {
            item_id: item.item_id.clone(),
            aliases,
            verdict,
        });
    }
    (!result.is_empty()).then_some(result)
}

fn dedupe_aliases(aliases: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for alias in aliases {
        let trimmed = alias.trim();
        if trimmed.is_empty() || out.iter().any(|a| a == trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockClient {
        reply: Result<String, String>,
    }

    #[async_trait]
    impl LlmClient for MockClient {
        async fn complete(
            &self,
            _settings: &LlmSettings,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> anyhow::Result<String> {
            match &self.reply {
                Ok(raw) => Ok(raw.clone()),
                Err(message) => anyhow::bail!("{message}"),
            }
        }
    }

    struct HangingClient;

    #[async_trait]
    impl LlmClient for HangingClient {
        async fn complete(
            &self,
            _settings: &LlmSettings,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> anyhow::Result<String> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(String::new())
        }
    }

    fn settings() -> LlmSettings {
        let mut llm = crate::settings::default_config().llm;
        llm.timeout_secs = 1;
        llm
    }

    fn body() -> SuggestAliasesBody {
        SuggestAliasesBody {
            mode: Some("aliases".to_string()),
            items: vec![
                AliasSuggestItem {
                    item_id: "pass_sp".to_string(),
                    name: "通行认证SP-月行水上".to_string(),
                    aliases: vec!["通行证".to_string()],
                },
                AliasSuggestItem {
                    item_id: "gift_card".to_string(),
                    name: "特典卡组-校园凭证".to_string(),
                    aliases: vec!["特典".to_string()],
                },
            ],
        }
    }

    #[tokio::test]
    async fn returns_filled_and_best_verdicts() {
        let raw = r#"```json
        {"suggestions":[
          {"item_id":"pass_sp","aliases":["通行证","通行认证"],"verdict":"filled"},
          {"item_id":"gift_card","aliases":["特典","校园凭证"],"verdict":"best"}
        ]}```"#;
        let client = MockClient {
            reply: Ok(raw.to_string()),
        };
        let value = suggest_aliases_with(&client, &settings(), body()).await;
        let suggestions = value["suggestions"].as_array().unwrap();
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0]["item_id"], "pass_sp");
        assert_eq!(suggestions[0]["verdict"], "filled");
        assert_eq!(suggestions[0]["aliases"][1], "通行认证");
        assert_eq!(suggestions[1]["verdict"], "best");
        assert!(value.get("degraded").is_none());
    }

    #[tokio::test]
    async fn malformed_json_degrades_to_200_shape() {
        let client = MockClient {
            reply: Ok("抱歉，我无法处理".to_string()),
        };
        let value = suggest_aliases_with(&client, &settings(), body()).await;
        assert_eq!(value["suggestions"].as_array().unwrap().len(), 0);
        assert_eq!(value["degraded"], true);
        assert!(!value["message"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn client_error_degrades() {
        let client = MockClient {
            reply: Err("boom".to_string()),
        };
        let value = suggest_aliases_with(&client, &settings(), body()).await;
        assert_eq!(value["degraded"], true);
        assert!(value["message"].as_str().unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn timeout_degrades() {
        let value = suggest_aliases_with(&HangingClient, &settings(), body()).await;
        assert_eq!(value["degraded"], true);
        assert!(value["message"].as_str().unwrap().contains("超时"));
    }

    #[tokio::test]
    async fn unknown_item_ids_are_ignored() {
        let raw = r#"{"suggestions":[{"item_id":"nope","aliases":["x"],"verdict":"filled"}]}"#;
        let client = MockClient {
            reply: Ok(raw.to_string()),
        };
        let value = suggest_aliases_with(&client, &settings(), body()).await;
        assert_eq!(value["degraded"], true);
    }
}
