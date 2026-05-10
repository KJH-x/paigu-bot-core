use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTrace {
    pub rule_id: String,
    pub rule_name: String,
    pub applied_to: Vec<String>,
    pub result: String,
    pub explanation: String,
}
