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
