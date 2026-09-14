use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::settings::LlmSettings;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(
        &self,
        settings: &LlmSettings,
        system_prompt: &str,
        user_prompt: &str,
    ) -> anyhow::Result<String>;
}

pub struct OpenAiClient {
    http: reqwest::Client,
}

impl OpenAiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }
}

impl Default for OpenAiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
struct ChatBody<'a> {
    model: &'a str,
    messages: Vec<ChatMsg<'a>>,
    temperature: f32,
    max_tokens: u32,
    response_format: ResponseFormat,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: Option<ChatChoiceMessage>,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: Option<String>,
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(
        &self,
        settings: &LlmSettings,
        system_prompt: &str,
        user_prompt: &str,
    ) -> anyhow::Result<String> {
        let api_key = std::env::var(&settings.api_key_env)
            .map_err(|_| anyhow::anyhow!("环境变量 {} 未设置", settings.api_key_env))?;
        let url = format!(
            "{}/chat/completions",
            settings.base_url.trim_end_matches('/')
        );
        let body = ChatBody {
            model: &settings.model,
            messages: vec![
                ChatMsg {
                    role: "system",
                    content: system_prompt,
                },
                ChatMsg {
                    role: "user",
                    content: user_prompt,
                },
            ],
            temperature: settings.temperature,
            max_tokens: settings.max_tokens,
            response_format: ResponseFormat {
                kind: "json_object",
            },
            stream: false,
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(settings.timeout_secs.max(1)))
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("LLM HTTP {}: {}", status.as_u16(), super::truncate(&text, 200));
        }

        let parsed: ChatResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("LLM 响应解析失败: {e}; raw={}", super::truncate(&text, 200)))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .and_then(|m| m.content)
            .unwrap_or_default();
        if content.trim().is_empty() {
            anyhow::bail!("LLM 空响应");
        }
        Ok(content)
    }
}
