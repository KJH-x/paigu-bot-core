use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemVariant, RoundContext};
use crate::parser::parsed_event::{ParsedClaimItem, ResolveResult};

pub fn resolve_item(parsed: &ParsedClaimItem, active_rounds: &[RoundContext]) -> ResolveResult {
    // §U7 first-match：只报角色名/变体名（未报大类）时，按目录顺序取第一个可拼团商品。
    if is_bare_variant_name(&parsed.name, active_rounds) {
        if let Some((round_id, item_id, variant_id)) = first_match(&parsed.name, active_rounds) {
            return match variant_id {
                Some(vid) => ResolveResult::resolved_variant(round_id, item_id, vid),
                None => ResolveResult::resolved(round_id, item_id),
            };
        }
    }
    let base = resolve_base_item(parsed, active_rounds);
    if base.resolved || base.ambiguity.is_some() {
        return base;
    }
    resolve_variant(parsed, active_rounds)
}

/// 可拼团商品：拼团类(`Split`) + 特典类(`Gift`)；单领/整盒/发货/调价不参与 first-match 候选。
pub fn is_splittable(item: &Item) -> bool {
    matches!(item.kind, ItemKind::Split | ItemKind::Gift)
}

fn item_name_matches(item: &Item, name: &str) -> bool {
    item.item_id.0 == name || item.name == name || item.aliases.iter().any(|a| a == name)
}

fn variant_name_matches(variant: &ItemVariant, name: &str) -> bool {
    variant.name == name
        || variant.aliases.iter().any(|a| a == name)
        || variant.name.contains(name)
        || name.contains(variant.name.as_str())
}

/// 是否「只报角色名/变体名、未报商品大类」：既非商品名/别名，又命中某可拼团商品的变体。
pub fn is_bare_variant_name(name: &str, active_rounds: &[RoundContext]) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let matches_category = active_rounds
        .iter()
        .flat_map(|r| &r.items)
        .any(|item| item_name_matches(item, name));
    if matches_category {
        return false;
    }
    active_rounds
        .iter()
        .flat_map(|r| &r.items)
        .filter(|item| is_splittable(item))
        .any(|item| item.variants.iter().any(|v| variant_name_matches(v, name)))
}

/// §U7 first-match：按商品目录顺序取第一个可拼团且含该名称的商品。
/// 命中商品名/别名 → `variant_id=None`；命中变体 → `Some(variant_id)`。
pub fn first_match(
    name: &str,
    active_rounds: &[RoundContext],
) -> Option<(RoundId, ItemId, Option<String>)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    for round in active_rounds {
        for item in &round.items {
            if !is_splittable(item) {
                continue;
            }
            if item_name_matches(item, name) {
                return Some((round.round_id.clone(), item.item_id.clone(), None));
            }
            if let Some(variant) = item.variants.iter().find(|v| variant_name_matches(v, name)) {
                return Some((
                    round.round_id.clone(),
                    item.item_id.clone(),
                    Some(variant.variant_id.clone()),
                ));
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::item::ItemVariant;
    use crate::domain::money::MoneyCents;

    fn variant(id: &str, name: &str) -> ItemVariant {
        ItemVariant {
            variant_id: id.to_string(),
            name: name.to_string(),
            unit_price: MoneyCents(0),
            capacity: None,
            aliases: vec![],
        }
    }

    fn item(id: &str, name: &str, kind: ItemKind, aliases: &[&str], variants: Vec<ItemVariant>) -> Item {
        Item {
            item_id: ItemId(id.to_string()),
            round_id: RoundId("r1".to_string()),
            name: name.to_string(),
            kind,
            unit_price: MoneyCents(0),
            box_size: None,
            max_quantity: None,
            is_blind: false,
            is_proxy_card: false,
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            sort_order: 0,
            metadata: serde_json::Value::Null,
            variants,
        }
    }

    fn ctx(items: Vec<Item>) -> Vec<RoundContext> {
        vec![RoundContext {
            round_id: RoundId("r1".to_string()),
            title: "t".to_string(),
            items,
        }]
    }

    fn claim(name: &str) -> ParsedClaimItem {
        ParsedClaimItem {
            name: name.to_string(),
            category_hint: None,
            quantity: 1,
            claim_type: Some("Split".to_string()),
            is_proxy_card: None,
            slot_policy: None,
            notes: None,
            resolved_item_id: None,
            resolved_variant_id: None,
            resolved_round_id: None,
        }
    }

    fn catalog() -> Vec<RoundContext> {
        ctx(vec![
            item(
                "pass_sp",
                "通行认证SP-月行水上",
                ItemKind::Split,
                &["通行证"],
                vec![variant("v_jcl", "结城理"), variant("v_hlw", "虎狼丸")],
            ),
            item(
                "hr_resume",
                "人事部简历SP-月行水上",
                ItemKind::Split,
                &["人事部简历"],
                vec![variant("v_jcl", "结城理")],
            ),
            item(
                "fashion",
                "风尚速递SP-月行水上",
                ItemKind::Split,
                &["风尚速递"],
                vec![variant("v_jcl", "结城理")],
            ),
        ])
    }

    #[test]
    fn first_match_picks_catalog_first_splittable_item() {
        let rounds = catalog();
        let m = first_match("结城理", &rounds).expect("match");
        assert_eq!(m.1 .0, "pass_sp", "应命中目录第一个（通行证），而非人事部简历");
        assert_eq!(m.2.as_deref(), Some("v_jcl"));
        assert!(is_bare_variant_name("结城理", &rounds));
    }

    #[test]
    fn first_match_ignores_non_splittable_candidates() {
        let rounds = ctx(vec![
            item(
                "single_card",
                "单领卡",
                ItemKind::Single,
                &[],
                vec![variant("v_jcl", "结城理")],
            ),
            item(
                "pass_sp",
                "通行认证SP-月行水上",
                ItemKind::Split,
                &["通行证"],
                vec![variant("v_jcl", "结城理")],
            ),
        ]);
        let m = first_match("结城理", &rounds).expect("match");
        assert_eq!(m.1 .0, "pass_sp", "单领商品不应成为 first-match 候选");
    }

    #[test]
    fn first_match_resolves_category_alias_without_variant() {
        let rounds = catalog();
        assert!(!is_bare_variant_name("通行证", &rounds));
        let m = first_match("通行证", &rounds).expect("match");
        assert_eq!(m.1 .0, "pass_sp");
        assert_eq!(m.2, None);
    }

    #[test]
    fn resolve_item_uses_first_match_for_bare_variant() {
        let rounds = catalog();
        let r = resolve_item(&claim("结城理"), &rounds);
        assert!(r.resolved);
        assert_eq!(r.item_id.unwrap().0, "pass_sp");
        assert_eq!(r.variant_id.as_deref(), Some("v_jcl"));
    }

    #[test]
    fn first_match_none_for_unknown_name() {
        let rounds = catalog();
        assert!(first_match("悠人", &rounds).is_none());
        assert!(!is_bare_variant_name("悠人", &rounds));
    }
}
