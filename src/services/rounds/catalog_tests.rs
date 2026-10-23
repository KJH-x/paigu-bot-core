use super::check::check_issues;
use crate::domain::{ids::RoundId, item::RoundContext};
use crate::parser::alias_match::first_match;
use crate::settings::{default_config, ItemConfig, RoundSettings, SubitemConfig};

#[derive(Clone, Copy, Debug)]
enum Token {
    ProductName,
    ProductAlias,
    VariantName,
    VariantAlias,
}
impl Token {
    fn is_product(self) -> bool {
        matches!(self, Self::ProductName | Self::ProductAlias)
    }
    fn assign(self, item: &mut ItemConfig, value: &str) {
        match self {
            Self::ProductName => item.name = value.into(),
            Self::ProductAlias => item.aliases.push(value.into()),
            Self::VariantName => item.subitems[0].name = value.into(),
            Self::VariantAlias => item.subitems[0].aliases.push(value.into()),
        }
    }
}
const TOKENS: [Token; 4] = [
    Token::ProductName,
    Token::ProductAlias,
    Token::VariantName,
    Token::VariantAlias,
];
fn catalog() -> RoundSettings {
    let mut round = default_config().round;
    round.items = [("badge", "徽章", "款式甲"), ("stand", "立牌", "款式乙")]
        .into_iter()
        .map(|(id, name, variant)| ItemConfig {
            item_id: id.into(),
            name: name.into(),
            kind: "拼团".into(),
            class: None,
            aliases: vec![],
            unit_price_cents: 1000,
            max_quantity: None,
            box_item_id: None,
            subitems: vec![SubitemConfig {
                subitem_id: "a".into(),
                name: variant.into(),
                aliases: vec![],
                unit_price_cents: 1000,
                adjust_cents: 0,
                capacity: Some(2),
            }],
        })
        .collect();
    round
}
fn conflicts(round: &RoundSettings) -> Vec<serde_json::Value> {
    check_issues(round)
        .into_iter()
        .filter(|issue| {
            matches!(
                issue["code"].as_str(),
                Some("name_conflict" | "alias_conflict")
            )
        })
        .collect()
}

#[test]
fn all_sixteen_same_owner_token_pairs_never_create_self_conflicts_or_rewrite_catalog() {
    for left in TOKENS {
        for right in TOKENS {
            let mut round = catalog();
            left.assign(&mut round.items[0], "共同标记");
            right.assign(&mut round.items[0], "共同标记");
            let before = serde_json::to_value(&round).unwrap();
            let issues = check_issues(&round);
            assert!(
                !issues.iter().any(|issue| issue["level"] == "error"),
                "same owner {left:?}/{right:?}: {issues:?}"
            );
            assert!(conflicts(&round).is_empty());
            assert_eq!(serde_json::to_value(&round).unwrap(), before);
        }
    }
}

#[test]
fn all_sixteen_cross_owner_pairs_distinguish_legal_role_reuse_from_product_collisions() {
    for left in TOKENS {
        for right in TOKENS {
            let mut round = catalog();
            left.assign(&mut round.items[0], "共同标记");
            right.assign(&mut round.items[1], "共同标记");
            let before = serde_json::to_value(&round).unwrap();
            let hits = conflicts(&round);
            if left.is_product() || right.is_product() {
                assert!(
                    !hits.is_empty(),
                    "real product collision {left:?}/{right:?}"
                );
                assert!(hits.iter().all(|issue| issue["level"] == "error"));
            } else {
                assert!(
                    hits.is_empty(),
                    "legal cross-product role reuse {left:?}/{right:?}: {hits:?}"
                );
                assert!(
                    check_issues(&round).is_empty(),
                    "do not add a noisy warning for a supported role reuse"
                );
            }
            assert_eq!(serde_json::to_value(&round).unwrap(), before);
        }
    }
}

#[test]
fn legal_shared_roles_keep_the_real_first_match_catalog_order_without_mutation() {
    let mut round = catalog();
    for item in &mut round.items {
        item.subitems[0].name = "共同款式".into();
        item.subitems[0].aliases = vec!["款式简称".into()];
    }
    for (expected, reverse) in [("badge", false), ("stand", true)] {
        if reverse {
            round.items.reverse();
        }
        let before = serde_json::to_value(&round).unwrap();
        assert!(check_issues(&round).is_empty());
        let context = RoundContext {
            round_id: RoundId(round.round_id.clone()),
            title: round.title.clone(),
            items: round.to_items(),
        };
        for name in ["共同款式", "款式简称"] {
            let (_, item, variant) = first_match(name, std::slice::from_ref(&context)).unwrap();
            assert_eq!(item.0, expected);
            assert_eq!(variant.as_deref(), Some("a"));
        }
        assert_eq!(serde_json::to_value(&round).unwrap(), before);
    }
}

#[test]
fn identity_scope_and_required_fields_stay_errors_without_banning_cross_product_variant_ids() {
    let round = catalog(); // Both products deliberately use subitem_id=a.
    assert!(check_issues(&round).is_empty());
    let mut duplicate = round.clone();
    duplicate.items[1].item_id = duplicate.items[0].item_id.clone();
    assert!(check_issues(&duplicate)
        .iter()
        .any(|issue| issue["code"] == "duplicate_item_id" && issue["level"] == "error"));
    let mut duplicate = round.clone();
    let repeated = duplicate.items[0].subitems[0].clone();
    duplicate.items[0].subitems.push(repeated);
    assert!(check_issues(&duplicate)
        .iter()
        .any(|issue| issue["code"] == "duplicate_subitem_id" && issue["level"] == "error"));
    for code in [
        "empty_item_id",
        "empty_name",
        "empty_subitem_id",
        "empty_subitem_name",
    ] {
        let mut invalid = round.clone();
        match code {
            "empty_item_id" => invalid.items[0].item_id.clear(),
            "empty_name" => invalid.items[0].name.clear(),
            "empty_subitem_id" => invalid.items[0].subitems[0].subitem_id.clear(),
            _ => invalid.items[0].subitems[0].name.clear(),
        }
        assert!(check_issues(&invalid)
            .iter()
            .any(|issue| issue["code"] == code && issue["level"] == "error"));
    }
}

#[test]
fn explicit_class_and_missing_price_are_advisories_not_an_auto_fix_or_false_block() {
    let mut round = catalog();
    round.items[0].class = Some("B".into());
    round.items[0].subitems[0].unit_price_cents = 0;
    round.items[1].kind = "单领".into();
    round.items[1].subitems.clear();
    round.items[1].class = Some("A".into());
    round.items[1].unit_price_cents = 0;
    let before = serde_json::to_value(&round).unwrap();
    let issues = check_issues(&round);
    assert!(!issues.iter().any(|issue| issue["level"] == "error"));
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue["code"] == "class_derived_mismatch" && issue["level"] == "warn")
            .count(),
        2
    );
    for code in ["missing_price", "missing_subitem_price"] {
        assert!(issues
            .iter()
            .any(|issue| issue["code"] == code && issue["level"] == "warn"));
    }
    assert_eq!(serde_json::to_value(&round).unwrap(), before);
}
