use super::*;

#[test]
fn time_phase_key_changes_on_half_open_boundaries_without_a_config_revision() {
    let mut cfg = crate::settings::default_config();
    cfg.members.cache_path.clear();
    cfg.round.phases = vec![crate::round::PhaseWindow {
        phase: crate::round::RoundPhase::Locked,
        start_ms: 1000,
        end_ms: 2000,
    }];
    let before = context_at(&cfg, &[], &[], 999).unwrap();
    let start = context_at(&cfg, &[], &[], 1000).unwrap();
    let end = context_at(&cfg, &[], &[], 2000).unwrap();
    assert_ne!(before, start);
    assert_eq!(start, context_at(&cfg, &[], &[], 1999).unwrap());
    assert_eq!(before, end);
    assert_eq!(BUILD.len(), 64);
    assert!(BUILD.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[tokio::test]
async fn even_validly_resealed_foreign_identity_is_not_a_valid_projection_and_size_is_bounded() {
    let root =
        std::env::temp_dir().join(format!("paigu-check-cache-shape-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("derived.json");
    let mut cfg = crate::settings::default_config();
    cfg.members.cache_path.clear();
    cfg.round.phases.clear();
    let result = session::replay_from_journal(&cfg, &[], &[], ReplayOverrides::default())
        .await
        .unwrap();
    let key = context(&cfg, &[], &[]).unwrap();
    save(&path, &key, &result).unwrap();
    assert!(load(&path, &key, &cfg, &[]).is_some());
    for which in 0..3 {
        let mut payload = Payload::new(&result);
        match which {
            0 => payload.result.round_id = "foreign".into(),
            1 => payload.result.board.round_id.0 = "foreign".into(),
            _ => payload.result.version += 1,
        }
        let file = File {
            format: FORMAT,
            build: BUILD.into(),
            context: key.clone(),
            integrity: digest(&payload).unwrap(),
            payload,
        };
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(
            load(&path, &key, &cfg, &[]).is_none(),
            "有效hash仍须检查身份/版本 {which}"
        );
    }
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(load(&path, &key, &cfg, &[]).is_none());
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("paigu-check-cache-shape-"));
    std::fs::remove_dir_all(root).unwrap();
}
