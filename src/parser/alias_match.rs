use crate::domain::ids::{RoundId, ItemId};
use crate::domain::item::RoundContext;
use crate::parser::parsed_event::{ParsedClaimItem, ResolveResult};

pub fn resolve_item(
    parsed: &ParsedClaimItem,
    active_rounds: &[RoundContext],
) -> ResolveResult {
    let mut candidates: Vec<(RoundId, ItemId, i32)> = Vec::new();

    for round in active_rounds {
        for item in &round.items {
            let mut score = 0;

            if item.item_id.0 == parsed.name {
                score += 1000;
            }
            if item.name == parsed.name {
                score += 900;
            }
            if item.aliases.iter().any(|a| a == &parsed.name) {
                score += 800;
            }
            if item.name.contains(&parsed.name) {
                score += 400;
            }
            if parsed.name.contains(&item.name) {
                score += 350;
            }

            if let Some(ref hint) = parsed.category_hint {
                if item.name.contains(hint) || item.aliases.iter().any(|a| a.contains(hint)) {
                    score += 150;
                }
            }

            if let Some(ref claim_type_str) = parsed.claim_type {
                if let Some(claim_type) = crate::domain::claim::ClaimType::from_str(claim_type_str) {
                    if item.kind.compatible_with(&claim_type) {
                        score += 100;
                    } else {
                        score -= 500;
                    }
                }
            }

            candidates.push((round.round_id.clone(), item.item_id.clone(), score));
        }
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2));

    if candidates.is_empty() || candidates[0].2 <= 0 {
        ResolveResult::not_found()
    } else if candidates.len() == 1 || candidates[0].2 >= candidates[1].2 + 300 {
        ResolveResult::resolved(candidates[0].0.clone(), candidates[0].1.clone())
    } else {
        let threshold = candidates[0].2 - 200;
        let top = candidates.into_iter().filter(|c| c.2 >= threshold).collect::<Vec<_>>();
        let names: Vec<String> = top.iter().map(|c| format!("({},{})", c.0.0, c.1.0)).collect();
        ResolveResult::ambiguous(top, format!("匹配到多个商品：{}", names.join(", ")))
    }
}

pub fn resolve_multiple_items(
    parsed_items: &[ParsedClaimItem],
    active_rounds: &[RoundContext],
) -> Vec<ResolveResult> {
    parsed_items.iter().map(|item| resolve_item(item, active_rounds)).collect()
}
