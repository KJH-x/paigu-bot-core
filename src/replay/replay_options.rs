use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParserMode {
    LiveLlm,
    CachedParse { cache_path: String },
    RuleOnly,
    HybridCachedThenLlm { cache_path: String },
}

impl Default for ParserMode {
    fn default() -> Self {
        ParserMode::LiveLlm
    }
}
