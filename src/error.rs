use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Settlement error: {0}")]
    Settlement(#[from] SettlementError),

    #[error("Replay error: {0}")]
    Replay(#[from] ReplayError),

    #[error("Export error: {0}")]
    Export(#[from] ExportError),

    #[error("Publish error: {0}")]
    Publish(#[from] PublishError),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON deserialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid JSON from LLM: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Ambiguous parse: {0}")]
    Ambiguous(String),

    #[error("Cache miss")]
    CacheMiss,
}

#[derive(Debug, Error)]
pub enum SettlementError {}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("Snapshot restore failed: step={0}, error={1}")]
    SnapshotRestoreFailed(u64, String),
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("CSV write error: {0}")]
    Csv(#[from] csv::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("S3 error: {0}")]
    S3(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type AppResult<T> = Result<T, AppError>;
