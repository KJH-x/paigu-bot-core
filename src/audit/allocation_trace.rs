use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditAllocationTrace {
    pub claim_id: String,
    pub item_id: String,
    pub requested_quantity: u32,
    pub allocated_quantity: u32,
    pub slot_policy: String,
    pub final_slots: Vec<SlotDetail>,
    pub explanation: String,
    pub priority_level: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotDetail {
    pub box_index: u32,
    pub slot_index: u32,
    pub outcome: String,
}
