use super::*;
use crate::settings::{ItemConfig, SubitemConfig};

fn subitem(subitem_id: &str, unit_price_cents: i64) -> SubitemConfig {
    SubitemConfig {
        subitem_id: subitem_id.to_string(),
        name: subitem_id.to_string(),
        unit_price_cents,
        adjust_cents: 0,
        capacity: None,
        aliases: vec![],
    }
}

fn item(item_id: &str, kind: &str, aliases: Vec<&str>, subitems: Vec<SubitemConfig>) -> ItemConfig {
    ItemConfig {
        item_id: item_id.to_string(),
        name: item_id.to_string(),
        kind: kind.to_string(),
        class: None,
        aliases: aliases.into_iter().map(str::to_string).collect(),
        unit_price_cents: 100,
        max_quantity: None,
        box_item_id: None,
        subitems,
    }
}

#[test]
fn check_flags_duplicates_aliases_and_category_mismatch() {
    let round = RoundSettings {
        round_id: "t".to_string(),
        title: "t".to_string(),
        group_id: "g".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![
            item("a", "拼团", vec!["通用"], vec![subitem("v1", 100)]),
            item("b", "单领", vec!["通用"], vec![]),
            item(
                "c",
                "单领",
                vec![],
                vec![subitem("v1", 0), subitem("v1", 0)],
            ),
        ],
        category: RoundCategory::Normal,
    };
    let issues = check_issues(&round);
    let codes: Vec<&str> = issues.iter().filter_map(|i| i["code"].as_str()).collect();
    assert!(codes.contains(&"alias_conflict"));
    assert!(codes.contains(&"duplicate_subitem_id"));
    assert!(codes.contains(&"category_subitem_mismatch"));
    assert!(codes.contains(&"missing_subitem_price"));
    assert!(!issues.iter().any(|i| i["level"] == "no_such_level"));
}

/// 真实 `config.example.json` 结构：多个商品复用 `v_jcl` 等 subitem_id、
/// 且角色名可在多商品重复，不应报 error（§U7/§U8 口径）。
#[test]
fn check_ok_for_example_config_structure() {
    let cfg = crate::settings::default_config();
    let issues = check_issues(&cfg.round);
    assert!(
        !issues.iter().any(|i| i["level"] == "error"),
        "真实配置不应有 error：{issues:?}"
    );
}

#[test]
fn check_flags_cross_item_name_and_alias_conflicts() {
    let round = RoundSettings {
        round_id: "t".to_string(),
        title: "t".to_string(),
        group_id: "g".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![
            item("a", "拼团", vec!["甲"], vec![subitem("v1", 100)]),
            item("b", "单领", vec!["乙"], vec![]),
        ],
        category: RoundCategory::Normal,
    };
    // 让 b 的别名与 a 的商品名冲突。
    let mut round = round;
    round.items[1].aliases = vec!["a".to_string()];
    let issues = check_issues(&round);
    // item() 的 name = item_id，此处应命中 name_conflict（"a" 同名）。
    assert!(
        issues.iter().any(|i| i["code"] == "name_conflict"),
        "{issues:?}"
    );
}

#[test]
fn check_ok_for_consistent_round() {
    let round = RoundSettings {
        round_id: "t".to_string(),
        title: "t".to_string(),
        group_id: "g".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![
            item("a", "拼团", vec!["甲"], vec![subitem("v1", 100)]),
            item("b", "单领", vec!["乙"], vec![]),
        ],
        category: RoundCategory::Normal,
    };
    let issues = check_issues(&round);
    assert!(!issues.iter().any(|i| i["level"] == "error"));
}

#[test]
fn setup_merge_keeps_absent_and_replaces_provided() {
    let base = RoundSettings {
        round_id: "r0".to_string(),
        title: "旧标题".to_string(),
        group_id: "123456789".to_string(),
        priority_users: vec!["u1".to_string()],
        priority_window: Some(PriorityWindow {
            start_ms: 1,
            end_ms: 2,
        }),
        phases: vec![],
        items: vec![item("a", "单领", vec![], vec![])],
        category: RoundCategory::Normal,
    };

    // 全缺省：仅 `round_id` 归一为路径 id，其余保留。
    let same = merge_setup("r1", base.clone(), &SetupBody::default());
    assert_eq!(same.round_id, "r1");
    assert_eq!(same.title, "旧标题");
    assert_eq!(same.group_id, "123456789");
    assert_eq!(same.priority_users, vec!["u1".to_string()]);
    assert!(same.priority_window.is_some());
    assert_eq!(same.items.len(), 1);

    // 提供即整体替换（列表可清空）。
    let body = SetupBody {
        title: Some(" 新标题 ".to_string()),
        priority_users: Some(vec![]),
        items: Some(vec![item(
            "b",
            "拼团",
            vec!["甲"],
            vec![subitem("v1", 100)],
        )]),
        ..SetupBody::default()
    };
    let changed = merge_setup("r1", base, &body);
    assert_eq!(changed.title, "新标题");
    assert!(changed.priority_users.is_empty());
    assert_eq!(changed.items.len(), 1);
    assert_eq!(changed.items[0].item_id, "b");
    // 未提供字段保留。
    assert_eq!(changed.group_id, "123456789");
}

#[test]
fn clean_opt_trims_and_drops_blank() {
    assert_eq!(clean_opt(None::<&str>), None);
    assert_eq!(clean_opt(Some("   ")), None);
    assert_eq!(clean_opt(Some(" 甲 ")), Some("甲".to_string()));
}

#[test]
fn category_binding_checks_test_and_normal() {
    let base = RoundSettings {
        round_id: "t".to_string(),
        title: "t".to_string(),
        group_id: "111".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![],
        category: RoundCategory::Test,
    };
    // 测试轮 + 匹配测试群 → 无 issue。
    assert!(category_issues(&base, Some("111")).is_empty());
    // 测试轮 + 不匹配 → error。
    assert!(category_issues(&base, Some("222"))
        .iter()
        .any(|i| i["code"] == "test_round_group_mismatch"));
    // 测试轮 + 未配置测试群 → error。
    assert!(category_issues(&base, None)
        .iter()
        .any(|i| i["level"] == "error"));

    let normal = RoundSettings {
        category: RoundCategory::Normal,
        ..base
    };
    // 正常轮 + 正常群 → 无 issue。
    assert!(category_issues(&normal, Some("222")).is_empty());
    // 正常轮 + 测试群 → error。
    assert!(category_issues(&normal, Some("111"))
        .iter()
        .any(|i| i["code"] == "normal_round_uses_test_group"));
}

#[test]
fn merge_setup_carries_category() {
    let base = RoundSettings {
        round_id: "r0".to_string(),
        title: "t".to_string(),
        group_id: "111".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![],
        category: RoundCategory::Test,
    };
    let same = merge_setup("r1", base.clone(), &SetupBody::default());
    assert_eq!(same.category, RoundCategory::Test);
    let body = SetupBody {
        category: Some(RoundCategory::Normal),
        ..SetupBody::default()
    };
    assert_eq!(
        merge_setup("r1", base, &body).category,
        RoundCategory::Normal
    );
}
