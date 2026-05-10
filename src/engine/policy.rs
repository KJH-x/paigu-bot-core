use crate::domain::round::RoundStatus;

pub fn can_claim_on_round(status: &RoundStatus) -> bool {
    matches!(status, RoundStatus::Active)
}

pub fn can_cancel_on_round(status: &RoundStatus, allow_cancel: bool) -> bool {
    allow_cancel && matches!(status, RoundStatus::Active)
}

pub fn can_admin_modify_on_round(status: &RoundStatus) -> bool {
    matches!(status, RoundStatus::Draft | RoundStatus::Scheduled | RoundStatus::Active | RoundStatus::Settling)
}
