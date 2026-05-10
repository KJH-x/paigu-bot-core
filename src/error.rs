use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    #[error("Allocation error: {0}")]
    Allocation(#[from] AllocationError),

    #[error("Settlement error: {0}")]
    Settlement(#[from] SettlementError),

    #[error("Event error: {0}")]
    Event(#[from] EventError),

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

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON deserialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Empty response")]
    EmptyResponse,

    #[error("Rate limited")]
    RateLimited,

    #[error("Token limit exceeded")]
    TokenLimitExceeded,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid JSON from LLM: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Unknown intent: {0}")]
    UnknownIntent(String),

    #[error("Ambiguous parse: {0}")]
    Ambiguous(String),

    #[error("Parse confidence too low: {0}")]
    LowConfidence(f32),

    #[error("Cache miss")]
    CacheMiss,

    #[error("Unknown item: {0}")]
    UnknownItem(String),

    #[error("Ambiguous item: {0}")]
    AmbiguousItem(String),

    #[error("Admin command parse error: {0}")]
    AdminCommandParse(String),
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Round not active: {0}")]
    RoundNotActive(String),

    #[error("Round not found: {0}")]
    RoundNotFound(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Item not found: {0}")]
    ItemNotFound(String),

    #[error("Exceeds max quantity: item={0}, max={1}, requested={2}")]
    ExceedsMaxQuantity(String, u32, u32),

    #[error("Duplicate claim: {0}")]
    DuplicateClaim(String),

    #[error("Invalid cancel target: {0}")]
    InvalidCancelTarget(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Time window not open")]
    TimeWindowNotOpen,

    #[error("Need confirmation: {0}")]
    NeedConfirmation(String),
}

#[derive(Debug, Error)]
pub enum AllocationError {
    #[error("No available slots for item {0}")]
    NoAvailableSlots(String),

    #[error("Slot already filled: item={0}, box={1}, slot={2}")]
    SlotAlreadyFilled(String, u32, u32),

    #[error("Slot locked: item={0}, box={1}, slot={2}")]
    SlotLocked(String, u32, u32),

    #[error("Quantity exceeds remaining: {0}")]
    ExceedsRemaining(u32),

    #[error("Segment conflict: {0}")]
    SegmentConflict(String),
}

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("Discount exceeds gross: gross={0}, discount={1}")]
    DiscountExceedsGross(i64, i64),

    #[error("Negative payable for user {0}: {1}")]
    NegativePayable(String, i64),

    #[error("Gift item not found: {0}")]
    GiftItemNotFound(String),

    #[error("Invalid discount rule: {0}")]
    InvalidDiscountRule(String),

    #[error("Rounding error: {0}")]
    RoundingError(String),
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("Event not found: {0}")]
    EventNotFound(String),

    #[error("Event sequence conflict: {0}")]
    SequenceConflict(String),

    #[error("Event already exists: {0}")]
    EventAlreadyExists(String),

    #[error("Invalid event type: {0}")]
    InvalidEventType(String),
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("Replay not found: {0}")]
    ReplayNotFound(String),

    #[error("Config hash mismatch: {0}")]
    ConfigHashMismatch(String),

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
    #[error("R2 put error: {0}")]
    R2Put(String),

    #[error("S3 error: {0}")]
    S3(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type AppResult<T> = Result<T, AppError>;
