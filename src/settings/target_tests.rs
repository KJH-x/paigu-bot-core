use super::*;
use serde_json::{json, Value};

struct Owned(PathBuf);
impl Drop for Owned {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("paigu-target-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fixture(data: Value) -> (Owned, AppConfig) {
    let owned = Owned(std::env::temp_dir().join(format!("paigu-target-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&owned.0).unwrap();
    let path = owned.0.join("members.json");
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    let mut cfg = default_config();
    cfg.round.group_id = "123456789".into();
    cfg.members.group_id = cfg.round.group_id.clone();
    cfg.members.cache_path = path.display().to_string();
    cfg.members.cn_overrides.clear();
    (owned, cfg)
}

fn lookup(cfg: &AppConfig, name: &str) -> Option<String> {
    match targets::MemberTargets::load(cfg, &cfg.round.group_id).resolve(name) {
        targets::MemberTarget::Known(uid) => Some(uid),
        _ => None,
    }
}

#[test]
fn ambiguous_shared_card_never_selects_hashmap_first() {
    let (_owned, cfg) = fixture(json!([
        {"user_id":"member01","card":"同名成员"},
        {"user_id":"member02","card":"同名成员"}
    ]));
    assert_eq!(
        lookup(&cfg, "同名成员"),
        None,
        "same-name lookup must not silently choose either member"
    );
}

#[test]
fn explicit_id_cn_alias_and_nfkc_resolve_to_known_member() {
    let (_owned, mut cfg) = fixture(json!([
        {"user_id":"member01","card":"Ａｌｐｈａ （备注）","nickname":"ignored-name"},
        {"user_id":"member02","card":"成员乙"}
    ]));
    cfg.members.cn_overrides = vec![CnOverride {
        user_id: "member01".into(),
        cn: "称呼甲".into(),
        aliases: vec!["别名甲".into()],
    }];
    for token in ["member01", "Alpha", "Ａｌｐｈａ", "称呼甲", "别名甲"] {
        assert_eq!(lookup(&cfg, token), Some("member01".into()), "{token}");
    }
    assert_eq!(
        lookup(&cfg, "ignored-name"),
        None,
        "nonempty group card remains authoritative"
    );
}

#[test]
fn partial_name_never_changes_a_claim_owner() {
    let (_owned, cfg) = fixture(json!([
        {"user_id":"member01","card":"成员甲长名"}
    ]));
    assert_eq!(
        lookup(&cfg, "成员甲"),
        None,
        "a substring is not a stable member identity"
    );
}

#[test]
fn source_group_mismatch_never_resolves_members_from_another_group() {
    let (_owned, mut cfg) = fixture(json!({"group_id":"987654321","members":[
        {"user_id":"member01","card":"成员甲"}
    ]}));
    assert_eq!(
        lookup(&cfg, "成员甲"),
        None,
        "wrapped cache group must be checked"
    );
    assert_eq!(member_display_name(&cfg, "member01"), None);
    std::fs::write(
        &cfg.members.cache_path,
        serde_json::to_vec(&json!([
            {"user_id":"member01","card":"成员甲"}
        ]))
        .unwrap(),
    )
    .unwrap();
    cfg.members.group_id = "987654321".into();
    assert_eq!(
        lookup(&cfg, "成员甲"),
        None,
        "configured source group must also match"
    );
    assert_eq!(member_display_name(&cfg, "member01"), None);
}

#[test]
fn card_first_name_and_empty_card_nickname_fallback_stay_consistent() {
    let (_owned, cfg) = fixture(json!([
        {"user_id":"member01","card":"成员甲 空白备注","nickname":"discarded"},
        {"user_id":"member02","card":"","nickname":"成员乙(备注)"}
    ]));
    assert_eq!(lookup(&cfg, "成员甲"), Some("member01".into()));
    assert_eq!(lookup(&cfg, "成员乙"), Some("member02".into()));
    assert_eq!(lookup(&cfg, "discarded"), None);
}

#[test]
fn malformed_duplicate_and_missing_sources_have_distinct_safe_results() {
    use targets::{MemberTarget, MemberTargets};
    for data in [
        json!({"members":"bad"}),
        json!([{}]),
        json!([
            {"user_id":"member01","card":"甲"}, {"user_id":"member01","card":"乙"}
        ]),
    ] {
        let (_owned, cfg) = fixture(data);
        assert_eq!(
            MemberTargets::load(&cfg, &cfg.round.group_id).resolve("甲"),
            MemberTarget::Unavailable
        );
    }
    let (_owned, cfg) = fixture(json!([]));
    std::fs::remove_file(&cfg.members.cache_path).unwrap();
    assert_eq!(
        MemberTargets::load(&cfg, &cfg.round.group_id).resolve("群外"),
        MemberTarget::Unknown
    );
}

#[test]
fn candidate_order_and_snapshot_are_stable_while_cache_and_overrides_change() {
    use targets::{MemberTarget, MemberTargets};
    let (_owned, mut cfg) = fixture(json!([
        {"user_id":"member02","card":"称呼甲"}, {"user_id":"member01","card":"称呼甲"}
    ]));
    cfg.members.cn_overrides = vec![CnOverride {
        user_id: "not-in-cache".into(),
        cn: "不存在".into(),
        aliases: vec![],
    }];
    let snapshot = MemberTargets::load(&cfg, &cfg.round.group_id);
    assert_eq!(
        snapshot.resolve("称呼甲"),
        MemberTarget::Ambiguous(vec!["member01".into(), "member02".into()])
    );
    assert_eq!(
        snapshot.resolve("称呼"),
        MemberTarget::Partial(vec!["member01".into(), "member02".into()])
    );
    assert_eq!(snapshot.resolve("不存在"), MemberTarget::Unknown);
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"member03","card":"称呼甲"}]"#,
    )
    .unwrap();
    assert_eq!(
        MemberTargets::load(&cfg, &cfg.round.group_id).resolve("称呼甲"),
        MemberTarget::Known("member03".into())
    );
    assert!(matches!(
        snapshot.resolve("称呼甲"),
        MemberTarget::Ambiguous(_)
    ));
}
