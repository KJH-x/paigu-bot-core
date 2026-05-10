use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditParseTrace {
    pub message_id: String,
    pub raw_text: String,
    pub parser_version: String,
    pub model: String,
    pub parsed_json: serde_json::Value,
    pub confidence: f32,
    pub ambiguous_parts: Vec<String>,
    pub status: String,
}
