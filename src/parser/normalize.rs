use crate::parser::parsed_event::ParsedClaimItem;

pub fn normalize_claim_item(item: &ParsedClaimItem) -> ParsedClaimItem {
    let mut normalized = item.clone();

    normalized.claim_type = normalized.claim_type.map(|ct| {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("single") || ct_lower.contains("单领") || ct_lower.contains("单") {
            "Single".to_string()
        } else if ct_lower.contains("gift") || ct_lower.contains("赠") || ct_lower.contains("特典") {
            "GiftClaim".to_string()
        } else {
            "Split".to_string()
        }
    });

    normalized.slot_policy = normalized.slot_policy.map(|sp| {
        let sp_lower = sp.to_lowercase();
        if sp_lower.contains("tail") || sp_lower.contains("包尾") || sp_lower.contains("端盒") {
            "TailLocked".to_string()
        } else if sp_lower.contains("column") || sp_lower.contains("锁列") {
            "ColumnLocked".to_string()
        } else if sp_lower.contains("admin") || sp_lower.contains("管理") {
            "AdminFixed".to_string()
        } else {
            "Normal".to_string()
        }
    });

    normalized
}
