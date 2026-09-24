use super::*;

#[test]
fn priority_window_is_end_exclusive() {
    assert!(in_priority_window(Some((100, 200)), 100));
    assert!(in_priority_window(Some((100, 200)), 199));
    assert!(!in_priority_window(Some((100, 200)), 200));
    assert!(!in_priority_window(Some((100, 200)), 99));
    assert!(!in_priority_window(None, 150));
}

#[test]
fn priority_user_matches_any_candidate() {
    let users = vec!["user_a".to_string(), "user_b".to_string()];
    assert!(is_priority_user(&users, &["user_b"]));
    assert!(is_priority_user(&users, &["x", "user_a"]));
    assert!(!is_priority_user(&users, &["user_c"]));
    assert!(!is_priority_user(&[], &["user_a"]));
}

#[test]
fn item_class_reads_config_and_defaults_to_b() {
    let cfg = default_config();
    assert_eq!(cfg.round.item_class("pass_sp"), ItemClass::A);
    assert_eq!(cfg.round.item_class("gift_card"), ItemClass::B);
    assert_eq!(cfg.round.item_class("does_not_exist"), ItemClass::B);
}

#[test]
fn item_class_prefers_derivation_with_explicit_override() {
    let mut cfg = default_config();
    // 清空显式 class：有变体的商品应推导为 A（阶段受限）。
    for it in &mut cfg.round.items {
        it.class = None;
    }
    assert_eq!(cfg.round.item_class("pass_sp"), ItemClass::A);
    assert_eq!(cfg.round.item_class("gift_card"), ItemClass::A);

    // 显式 class 可覆盖推导。
    for it in &mut cfg.round.items {
        if it.item_id == "pass_sp" {
            it.class = Some("B".to_string());
        }
    }
    assert_eq!(cfg.round.item_class("pass_sp"), ItemClass::B);

    // 无变体 ⇒ B。
    cfg.round.items.push(ItemConfig {
        item_id: "single_only".to_string(),
        name: "单领".to_string(),
        kind: "单领".to_string(),
        class: None,
        aliases: vec![],
        unit_price_cents: 100,
        max_quantity: Some(3),
        variants: vec![],
    });
    assert_eq!(cfg.round.item_class("single_only"), ItemClass::B);
}

#[test]
fn variant_final_price_is_base_plus_adjust() {
    let mk = |unit_price_cents: i64, adjust_cents: i64| VariantConfig {
        variant_id: "v".to_string(),
        name: "变体".to_string(),
        unit_price_cents,
        adjust_cents,
        capacity: None,
        aliases: vec![],
    };
    assert_eq!(mk(5000, 0).final_price_cents(), 5000);
    assert_eq!(mk(5000, -500).final_price_cents(), 4500);
    assert_eq!(mk(2500, 300).final_price_cents(), 2800);
    assert_eq!(
        mk(i64::MAX, 10).final_price_cents(),
        i64::MAX,
        "加价溢出应饱和"
    );
}

#[test]
fn item_category_parses_ui_canonical_kinds() {
    // UI「种类」写入 `kind` 的规范值：group|single|box|gift。
    assert_eq!(ItemCategory::parse("group"), Some(ItemCategory::Group));
    assert_eq!(ItemCategory::parse("single"), Some(ItemCategory::Single));
    assert_eq!(ItemCategory::parse("box"), Some(ItemCategory::Box));
    assert_eq!(ItemCategory::parse("gift"), Some(ItemCategory::Gift));
    assert_eq!(ItemCategory::parse("bogus"), None);
}

#[test]
fn to_items_maps_whole_box_and_gift_kinds() {
    let mk_variant = || VariantConfig {
        variant_id: "v1".to_string(),
        name: "甲".to_string(),
        unit_price_cents: 100,
        adjust_cents: 0,
        capacity: None,
        aliases: vec![],
    };
    let mk = |id: &str, kind: &str, with_variant: bool| ItemConfig {
        item_id: id.to_string(),
        name: id.to_string(),
        kind: kind.to_string(),
        class: None,
        aliases: vec![],
        unit_price_cents: 100,
        max_quantity: None,
        variants: if with_variant {
            vec![mk_variant()]
        } else {
            vec![]
        },
    };
    let round = RoundSettings {
        round_id: "r".to_string(),
        title: "t".to_string(),
        group_id: "g".to_string(),
        priority_users: vec![],
        priority_window: None,
        phases: vec![],
        items: vec![
            mk("box_zh", "整盒", false),
            mk("box_en", "box", false),
            mk("box_wb", "whole_box", false),
            mk("gift_zh", "特典", true),
            mk("gift_en", "gift", true),
            mk("split_en", "split", true),
            mk("single_en", "single", false),
        ],
    };
    let kinds: std::collections::HashMap<String, ItemKind> = round
        .to_items()
        .into_iter()
        .map(|i| (i.item_id.0, i.kind))
        .collect();
    assert_eq!(kinds["box_zh"], ItemKind::WholeBox);
    assert_eq!(kinds["box_en"], ItemKind::WholeBox);
    assert_eq!(kinds["box_wb"], ItemKind::WholeBox);
    assert_eq!(kinds["gift_zh"], ItemKind::Gift);
    assert_eq!(kinds["gift_en"], ItemKind::Gift);
    assert_eq!(kinds["split_en"], ItemKind::Split);
    assert_eq!(kinds["single_en"], ItemKind::Single);
}

#[test]
fn empty_cn_overrides_match_legacy_behaviour() {
    let cfg = default_config();
    assert!(cfg.members.cn_overrides.is_empty());
    assert_eq!(
        resolve_cn(&cfg, "u1", "昵称（备注）").as_deref(),
        Some("昵称")
    );
    assert_eq!(resolve_cn(&cfg, "u2", "").as_deref(), Some("u2"));
    assert!(!is_priority_with_cn(&cfg, "u1", "昵称"));
}

#[test]
fn new_fields_default_to_unrestricted() {
    let cfg = default_config();
    assert!(cfg.gateway.whitelist_members.is_empty());
    assert!(cfg.round.phases.is_empty());
}

#[tokio::test]
async fn put_rejects_stale_revision() {
    let path =
        std::env::temp_dir().join(format!("paigu-settings-test-{}.json", uuid::Uuid::new_v4()));
    let store = ConfigStore::load(&path).expect("load config");
    let current = store.get().await.revision;

    let next = store
        .put(store.get().await, current)
        .await
        .expect("put with current revision");
    assert_eq!(next, current + 1);

    match store.put(store.get().await, current).await {
        Err(ConfigError::StaleRevision { expected, actual }) => {
            assert_eq!(expected, current);
            assert_eq!(actual, current + 1);
        }
        other => panic!("expected StaleRevision, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn resolve_cn_prefers_override_then_nickname_then_user_id() {
    let mut cfg = default_config();
    cfg.members.cn_overrides = vec![CnOverride {
        user_id: "u1".to_string(),
        cn: "甲子".to_string(),
        aliases: vec!["小甲".to_string()],
    }];
    assert_eq!(resolve_cn(&cfg, "u1", "任意昵称").as_deref(), Some("甲子"));
    assert_eq!(resolve_cn(&cfg, "u2", "乙（备注）").as_deref(), Some("乙"));
    assert_eq!(resolve_cn(&cfg, "u3", "").as_deref(), Some("u3"));
    assert_eq!(resolve_cn(&cfg, "", ""), None);
}

#[test]
fn priority_candidates_add_cn_and_aliases() {
    let mut cfg = default_config();
    cfg.members.cn_overrides = vec![CnOverride {
        user_id: "u1".to_string(),
        cn: "甲子".to_string(),
        aliases: vec!["小甲".to_string()],
    }];
    cfg.round.priority_users = vec!["甲子".to_string()];
    assert!(is_priority_with_cn(&cfg, "u1", "任意"));
    cfg.round.priority_users = vec!["小甲".to_string()];
    assert!(is_priority_with_cn(&cfg, "u1", "任意"));
    cfg.round.priority_users = vec!["别人".to_string()];
    assert!(!is_priority_with_cn(&cfg, "u1", "任意"));
}

#[test]
fn item_category_and_derived_class_follow_variants() {
    let cfg = default_config();
    let pass = cfg
        .round
        .items
        .iter()
        .find(|i| i.item_id == "pass_sp")
        .expect("pass_sp");
    assert_eq!(pass.category(), Some(ItemCategory::Group));
    assert_eq!(pass.derived_class(), ItemClass::A);
    assert!(pass.category().unwrap().requires_variants());

    let single = ItemConfig {
        item_id: "s".to_string(),
        name: "单领".to_string(),
        kind: "单领".to_string(),
        class: None,
        aliases: vec![],
        unit_price_cents: 100,
        max_quantity: Some(3),
        variants: vec![],
    };
    assert_eq!(single.category(), Some(ItemCategory::Single));
    assert_eq!(single.derived_class(), ItemClass::B);

    let boxed = ItemConfig {
        kind: "整盒".to_string(),
        ..single.clone()
    };
    assert_eq!(boxed.category(), Some(ItemCategory::Box));
    assert!(!boxed.category().unwrap().requires_variants());
}

#[test]
fn round_store_activation_round_trips() {
    let mut cfg = default_config();
    let id = format!("test-round-{}", uuid::Uuid::new_v4());
    cfg.round.round_id = id.clone();
    cfg.round.title = "原始".to_string();
    cfg.active_round_id = None;
    assert!(rounds::resolve_active(&mut cfg));
    assert_eq!(cfg.active_round_id.as_deref(), Some(id.as_str()));

    let mut reloaded = default_config();
    reloaded.round.round_id = id.clone();
    reloaded.round.title = "应被覆盖".to_string();
    reloaded.active_round_id = Some(id.clone());
    assert!(!rounds::resolve_active(&mut reloaded));
    assert_eq!(reloaded.round.title, "原始");

    assert!(rounds::read_round(&id).is_some());
    let _ = std::fs::remove_file(rounds::round_path(&id));
}

#[test]
fn round_id_validation_rejects_paths() {
    assert!(rounds::is_valid_round_id("月行水上"));
    assert!(rounds::is_valid_round_id("round-1_a"));
    assert!(!rounds::is_valid_round_id(""));
    assert!(!rounds::is_valid_round_id("../etc"));
    assert!(!rounds::is_valid_round_id("a/b"));
}

#[tokio::test]
async fn reload_bumps_revision() {
    let path = std::env::temp_dir().join(format!(
        "paigu-settings-reload-{}.json",
        uuid::Uuid::new_v4()
    ));
    let store = ConfigStore::load(&path).expect("load config");
    let before = store.get().await.revision;

    let after = store.reload().await.expect("reload");
    assert_eq!(after, before + 1);
    assert_eq!(store.get().await.revision, before + 1);
    let _ = std::fs::remove_file(&path);
}
