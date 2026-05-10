use serde::{Deserialize, Serialize};

pub const SYSTEM_PROMPT: &str = r#"你是排谷系统的自然语言解析器。你的任务是把用户消息解析成 JSON。
不要判断能不能排上，不要计算价格，不要生成回复。
只抽取意图、商品名、数量、拼团/单领、是否代牌、是否包尾/端盒/锁列、撤销对象、管理员命令。
若不确定，填写 ambiguous_parts。
输出必须是合法 JSON，不要包含任何解释文字。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParsedIntent {
    Claim,
    Cancel,
    Modify,
    ConfirmAmbiguous,
    AdminCommand,
    Unknown,
}

impl ParsedIntent {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParsedIntent::Claim => "Claim",
            ParsedIntent::Cancel => "Cancel",
            ParsedIntent::Modify => "Modify",
            ParsedIntent::ConfirmAmbiguous => "ConfirmAmbiguous",
            ParsedIntent::AdminCommand => "AdminCommand",
            ParsedIntent::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMessage {
    pub intent: ParsedIntent,
    pub round_hint: Option<String>,
    pub items: Vec<ParsedClaimItem>,
    pub cancel_target_hint: Option<String>,
    pub admin_command: Option<ParsedAdminCommand>,
    pub confidence: f32,
    pub ambiguous_parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedClaimItem {
    pub name: String,
    pub category_hint: Option<String>,
    pub quantity: u32,
    pub claim_type: Option<String>,
    pub is_proxy_card: Option<bool>,
    pub slot_policy: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ParsedAdminCommand {
    CreateRound {
        title: String,
        start_at: Option<String>,
        end_at: Option<String>,
    },
    AddItem {
        name: String,
        kind: String,
        unit_price_cents: i64,
        box_size: Option<u32>,
        max_quantity: Option<u32>,
        aliases: Option<Vec<String>>,
    },
    SetDiscountRules {
        rules: Vec<serde_json::Value>,
    },
    CloseRound {},
    ExportRound {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResult {
    pub round_id: Option<crate::domain::ids::RoundId>,
    pub item_id: Option<crate::domain::ids::ItemId>,
    pub candidates: Vec<(crate::domain::ids::RoundId, crate::domain::ids::ItemId, i32)>,
    pub resolved: bool,
    pub ambiguity: Option<String>,
}

impl ResolveResult {
    pub fn not_found() -> Self {
        Self {
            round_id: None,
            item_id: None,
            candidates: vec![],
            resolved: false,
            ambiguity: None,
        }
    }

    pub fn resolved(round_id: crate::domain::ids::RoundId, item_id: crate::domain::ids::ItemId) -> Self {
        Self {
            round_id: Some(round_id),
            item_id: Some(item_id),
            candidates: vec![],
            resolved: true,
            ambiguity: None,
        }
    }

    pub fn ambiguous(candidates: Vec<(crate::domain::ids::RoundId, crate::domain::ids::ItemId, i32)>, msg: String) -> Self {
        Self {
            round_id: None,
            item_id: None,
            candidates,
            resolved: false,
            ambiguity: Some(msg),
        }
    }
}

pub struct MessageParser {
    pub llm_client: Option<Box<dyn super::llm_client::LlmClient>>,
}

impl std::fmt::Debug for MessageParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageParser")
            .field("llm_client", &self.llm_client.is_some())
            .finish()
    }
}

impl MessageParser {
    pub fn new(llm_client: Option<Box<dyn super::llm_client::LlmClient>>) -> Self {
        Self { llm_client }
    }

    pub async fn parse_member_message(
        &self,
        msg: &super::llm_client::ParseRequestContext,
    ) -> Result<ParsedMessage, crate::error::ParseError> {
        if let Some(ref client) = self.llm_client {
            let prompt = SYSTEM_PROMPT.to_string();
            let payload = serde_json::to_value(msg).map_err(|e| crate::error::ParseError::InvalidJson(e))?;

            let response = client
                .parse_message(crate::parser::llm_client::LlmParseRequest {
                    system_prompt: prompt,
                    user_payload: payload,
                    temperature: 0.0,
                    max_tokens: 2048,
                })
                .await
                .map_err(|e| crate::error::ParseError::Ambiguous(e.to_string()))?;

            Ok(response.parsed)
        } else {
            Err(crate::error::ParseError::CacheMiss)
        }
    }
}
