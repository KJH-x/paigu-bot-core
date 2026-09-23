use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::RoundContext;
use crate::parser::parsed_event::{ParsedClaimItem, ResolveResult};

pub fn resolve_item(parsed: &ParsedClaimItem, active_rounds: &[RoundContext]) -> ResolveResult {
    let base = resolve_base_item(parsed, active_rounds);
    if base.resolved || base.ambiguity.is_some() {
        return base;
    }
    resolve_variant(parsed, active_rounds)
}

fn resolve_base_item(parsed: &ParsedClaimItem, active_rounds: &[RoundContext]) -> ResolveResult {
    let mut candidates: Vec<(RoundId, ItemId, i32)> = Vec::new();

    for round in active_rounds {
        for item in &round.items {
            let mut name_score = 0;

            if item.item_id.0 == parsed.name {
                name_score += 1000;
            }
            if item.name == parsed.name {
                name_score += 900;
            }
            if item.aliases.iter().any(|a| a == &parsed.name) {
                name_score += 800;
            }
            if item.name.contains(&parsed.name) {
                name_score += 400;
            }
            if parsed.name.contains(&item.name) {
                name_score += 350;
            }

            if name_score <= 0 {
                continue;
            }

            let mut score = name_score;

            if let Some(ref hint) = parsed.category_hint {
                if item.name.contains(hint) || item.aliases.iter().any(|a| a.contains(hint)) {
                    score += 150;
                }
            }

            if let Some(ref claim_type_str) = parsed.claim_type {
                if let Some(claim_type) = crate::domain::claim::ClaimType::from_str(claim_type_str)
                {
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
        let top = candidates
            .into_iter()
            .filter(|c| c.2 >= threshold)
            .collect::<Vec<_>>();
        let names: Vec<String> = top
            .iter()
            .map(|c| format!("({},{})", c.0 .0, c.1 .0))
            .collect();
        ResolveResult::ambiguous(top, format!("匹配到多个商品：{}", names.join(", ")))
    }
}

fn resolve_variant(parsed: &ParsedClaimItem, active_rounds: &[RoundContext]) -> ResolveResult {
    let mut candidates: Vec<(RoundId, ItemId, String, i32)> = Vec::new();

    for round in active_rounds {
        for item in &round.items {
            for variant in &item.variants {
                let mut score = 0;

                if variant.name == parsed.name {
                    score += 900;
                }
                if variant.aliases.iter().any(|a| a == &parsed.name) {
                    score += 800;
                }
                if variant.name.contains(&parsed.name) {
                    score += 400;
                }
                if parsed.name.contains(&variant.name) {
                    score += 350;
                }

                if let Some(ref hint) = parsed.category_hint {
                    if variant.name.contains(hint) {
                        score += 150;
                    }
                }

                if let Some(ref claim_type_str) = parsed.claim_type {
                    if let Some(claim_type) =
                        crate::domain::claim::ClaimType::from_str(claim_type_str)
                    {
                        if item.kind.compatible_with(&claim_type) {
                            score += 100;
                        } else {
                            score -= 500;
                        }
                    }
                }

                if score > 0 {
                    candidates.push((
                        round.round_id.clone(),
                        item.item_id.clone(),
                        variant.variant_id.clone(),
                        score,
                    ));
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.3.cmp(&a.3));

    if candidates.is_empty() {
        ResolveResult::not_found()
    } else if candidates.len() == 1 || candidates[0].3 >= candidates[1].3 + 300 {
        let best = &candidates[0];
        ResolveResult::resolved_variant(best.0.clone(), best.1.clone(), best.2.clone())
    } else {
        let threshold = candidates[0].3 - 200;
        let top = candidates
            .into_iter()
            .filter(|c| c.3 >= threshold)
            .collect::<Vec<_>>();
        let names: Vec<String> = top
            .iter()
            .map(|c| format!("({},{},{})", c.0 .0, c.1 .0, c.2))
            .collect();
        let plain: Vec<(RoundId, ItemId, i32)> = top
            .iter()
            .map(|c| (c.0.clone(), c.1.clone(), c.3))
            .collect();
        ResolveResult::ambiguous(plain, format!("匹配到多个变体：{}", names.join(", ")))
    }
}

pub fn resolve_multiple_items(
    parsed_items: &[ParsedClaimItem],
    active_rounds: &[RoundContext],
) -> Vec<ResolveResult> {
    parsed_items
        .iter()
        .map(|item| resolve_item(item, active_rounds))
        .collect()
}
