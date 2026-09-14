use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBundle {
    pub manifest: serde_json::Value,
    pub config: serde_json::Value,
    pub messages: serde_json::Value,
    pub events: serde_json::Value,
    pub snapshot: serde_json::Value,
    pub settlement: serde_json::Value,
}
