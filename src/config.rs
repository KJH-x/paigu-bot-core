use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub r2: R2Config,
    pub llm: LlmConfig,
    pub app: AppConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct R2Config {
    pub bucket: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub confidence_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_timezone: String,
    pub host: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            database: DatabaseConfig {
                url: std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgres://localhost:5432/paigu".to_string()),
                max_connections: std::env::var("DATABASE_MAX_CONNECTIONS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
            },
            r2: R2Config {
                bucket: std::env::var("R2_BUCKET").unwrap_or_default(),
                endpoint: std::env::var("R2_ENDPOINT").unwrap_or_default(),
                access_key_id: std::env::var("R2_ACCESS_KEY_ID").unwrap_or_default(),
                secret_access_key: std::env::var("R2_SECRET_ACCESS_KEY").unwrap_or_default(),
            },
            llm: LlmConfig {
                api_base: std::env::var("LLM_API_BASE").unwrap_or_default(),
                api_key: std::env::var("LLM_API_KEY").unwrap_or_default(),
                model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4".to_string()),
                max_tokens: std::env::var("LLM_MAX_TOKENS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2048),
                temperature: std::env::var("LLM_TEMPERATURE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0),
                confidence_threshold: std::env::var("LLM_CONFIDENCE_THRESHOLD")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.65),
            },
            app: AppConfig {
                default_timezone: std::env::var("DEFAULT_TIMEZONE")
                    .unwrap_or_else(|_| "Asia/Shanghai".to_string()),
                host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: std::env::var("PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8080),
            },
        })
    }
}
