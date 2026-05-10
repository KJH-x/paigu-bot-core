use crate::domain::claim::{EffectiveClaimLine, Eligibility};

pub fn compute_priority_level(
    line: &EffectiveClaimLine,
    eligibilities: &[Eligibility],
) -> i32 {
    eligibilities
        .iter()
        .filter(|e| e.user_id == line.user_id)
        .filter(|e| e.applies_to_item(&line.item_id))
        .filter(|e| e.applies_at(line.effective_at))
        .filter(|e| e.has_uses_remaining())
        .map(|e| e.priority_level)
        .max()
        .unwrap_or(0)
}

pub fn sort_claim_lines(lines: &mut [EffectiveClaimLine]) {
    lines.sort_by(|a, b| {
        b.priority_level
            .cmp(&a.priority_level)
            .then_with(|| a.effective_at.cmp(&b.effective_at))
            .then_with(|| a.sequence.cmp(&b.sequence))
            .then_with(|| a.line_index.cmp(&b.line_index))
    });
}
