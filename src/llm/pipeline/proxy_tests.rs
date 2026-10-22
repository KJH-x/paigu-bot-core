use super::automatic_tests::{fixture, incoming, quantity};
use crate::api::ApiState;
use crate::bus::DelegationKind;
use crate::round::{PhaseWindow, RoundPhase};
use crate::settings::{CnOverride, PriorityWindow};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

async fn configure(state: &ApiState, members: Value, priority: &[&str]) {
    let mut cfg = state.cfg.get().await;
    cfg.members.group_id = cfg.round.group_id.clone();
    std::fs::write(
        &cfg.members.cache_path,
        serde_json::to_vec(&members).unwrap(),
    )
    .unwrap();
    cfg.round.priority_users = priority.iter().map(|s| (*s).into()).collect();
    cfg.round.priority_window = Some(PriorityWindow {
        start_ms: 0,
        end_ms: 10_000,
    });
    cfg.gateway.admin_commands_enabled = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
}

async fn rows(state: &ApiState) -> Vec<crate::messages::MessageRecord> {
    state
        .messages
        .read_all(&state.cfg.get().await.round.round_id)
        .await
        .unwrap()
}

fn canonical(mut board: Value) -> Value {
    if let Some(object) = board.as_object_mut() {
        object.remove("generated_at");
    }
    board
}

async fn parity(state: &ApiState) {
    let before = state.pipeline.board().await.1;
    let replay = state.pipeline.recompute().await.unwrap();
    if before.is_null() {
        assert!(replay.events.is_empty());
        assert_eq!(quantity(&replay.board), 0);
        assert_eq!(
            canonical(state.pipeline.board().await.1),
            canonical(serde_json::to_value(replay.board).unwrap())
        );
        return;
    }
    assert_eq!(
        canonical(before),
        canonical(serde_json::to_value(replay.board).unwrap())
    );
}

#[tokio::test]
async fn known_member_qualification_is_recipient_not_actor_and_board_has_no_proxy_label() {
    for (priority, expected) in [(vec!["u2"], "Applied"), (vec!["u1"], "Rejected")] {
        let (state, llm) = fixture(false, "").await;
        configure(
            &state,
            json!([{"user_id":"u2","card":"MemberBeta"}]),
            &priority,
        )
        .await;
        let out = state
            .pipeline
            .ingest(incoming("代MemberBeta排:排 徽章 甲 1", 1000))
            .await;
        assert_eq!(out.status, expected, "{}", out.detail);
        let record = rows(&state).await.remove(0);
        assert_eq!(record.user_id, "u2");
        assert_eq!(
            record.sender_user_id, None,
            "known member retains its own qualification"
        );
        let meta = record.delegation.unwrap();
        assert_eq!(meta.actor_user_id, "u1");
        assert!(matches!(meta.target, DelegationKind::Member));
        if expected == "Applied" {
            let board = state.pipeline.board().await.1.to_string();
            assert!(board.contains("MemberBeta"));
            assert!(!board.contains("(代"));
        }
        parity(&state).await;
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn guests_numeric_codes_and_anonymous_use_actor_permission_not_another_members_id() {
    for text in [
        "代1排:排 徽章 甲 1",
        "代X排:排 徽章 甲 1",
        "代群外排:排 徽章 甲 1",
        "代王哥排:排 徽章 甲 1",
        "排 徽章 甲 1(代)",
    ] {
        for allowed in [true, false] {
            let (state, llm) = fixture(false, "").await;
            configure(
                &state,
                json!([{"user_id":"1","card":"数字成员"}]),
                if allowed { &["u1"] } else { &["1"] },
            )
            .await;
            let out = state.pipeline.ingest(incoming(text, 1000)).await;
            assert_eq!(
                out.status,
                if allowed { "Applied" } else { "Rejected" },
                "{text}: {}",
                out.detail
            );
            let record = rows(&state).await.remove(0);
            assert!(record.user_id.starts_with("proxy:"));
            assert_eq!(record.sender_user_id.as_deref(), Some("u1"));
            assert!(matches!(
                record.delegation.as_ref().unwrap().target,
                DelegationKind::Guest { .. }
            ));
            assert_eq!(record.delegation.as_ref().unwrap().source_text, text);
            if allowed {
                assert!(state
                    .pipeline
                    .board()
                    .await
                    .1
                    .to_string()
                    .contains("成员01(代"));
            }
            parity(&state).await;
            assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn same_name_proxy_actors_never_share_orders_and_rename_keeps_the_same_guest_identity() {
    let (state, _) = fixture(false, "").await;
    configure(&state, json!([]), &["u1", "u2"]).await;
    let mut ids = vec![];
    for (uid, name, ts) in [
        ("u1", "同名", 1000),
        ("u2", "同名", 2000),
        ("u1", "新称呼", 3000),
    ] {
        let mut ev = incoming("代1排:排 徽章 甲 1", ts);
        ev.user_id = uid.into();
        ev.nickname = name.into();
        assert_eq!(state.pipeline.ingest(ev).await.status, "Applied");
        ids.push(rows(&state).await.last().unwrap().user_id.clone());
        parity(&state).await;
        if ts == 2000 {
            assert_eq!(
                state.pipeline.who_whats().await.len(),
                2,
                "same display names do not combine different real actors"
            );
        }
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[0], ids[2]);
    let mut cfg = state.cfg.get().await;
    cfg.members.cn_overrides.push(CnOverride {
        user_id: "u1".into(),
        cn: "统一新CN".into(),
        aliases: vec![],
    });
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 3);
    assert!(serde_json::to_string(&result.board)
        .unwrap()
        .contains("统一新CN(代1)"));
}

#[tokio::test]
async fn ambiguity_partial_source_mismatch_and_unknown_explicit_member_do_not_ask_model_or_guess() {
    for (cache, text) in [
        (
            json!([{"user_id":"u2","card":"同名"},{"user_id":"u3","card":"同名"}]),
            "代同名排:排 徽章 甲 1",
        ),
        (
            json!([{"user_id":"u2","card":"成员长名"}]),
            "代成员排:排 徽章 甲 1",
        ),
        (
            json!({"group_id":"987654321","members":[{"user_id":"u2","card":"MemberBeta"}]}),
            "代MemberBeta排:排 徽章 甲 1",
        ),
        (json!([]), "代@unknown排:排 徽章 甲 1"),
    ] {
        let (state, model) = fixture(true, "").await;
        configure(&state, cache, &["u1", "u2"]).await;
        let out = state.pipeline.ingest(incoming(text, 1000)).await;
        assert_eq!(out.status, "NeedConfirm", "{}", out.detail);
        let record = rows(&state).await.remove(0);
        assert_eq!(record.user_id, "u1");
        assert_eq!(record.text, text);
        assert!(record.processing.unwrap().results[0]
            .understanding
            .events
            .is_empty());
        parity(&state).await;
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn cn_alias_explicit_id_and_known_line_prefix_resolve_without_stealing_product_headings() {
    let (state, model) = fixture(false, "").await;
    configure(
        &state,
        json!([{"user_id":"1","card":"ＭｅｍｂｅｒＢｅｔａ"}]),
        &["1"],
    )
    .await;
    let mut cfg = state.cfg.get().await;
    cfg.members.cn_overrides.push(CnOverride {
        user_id: "1".into(),
        cn: "称呼乙".into(),
        aliases: vec!["别名乙".into()],
    });
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for (i, text) in [
        "代称呼乙排:排 徽章 甲 1",
        "代别名乙排:排 徽章 甲 1",
        "代@1排:排 徽章 甲 1",
        "ＭｅｍｂｅｒＢｅｔａ：排 徽章 甲 1",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            state
                .pipeline
                .ingest(incoming(text, 1000 + i as i64))
                .await
                .status,
            "Applied",
            "{text}"
        );
        assert_eq!(rows(&state).await.last().unwrap().user_id, "1");
        parity(&state).await;
    }
    assert!(super::proxy::segments(&cfg, &incoming("徽章：甲1", 3000)).is_none());
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"1","card":"MemberBeta"},{"user_id":"2","card":"称呼乙"}]"#,
    )
    .unwrap();
    let mut cfg = state.cfg.get().await;
    cfg.members.cn_overrides.push(CnOverride {
        user_id: "2".into(),
        cn: "显示丙".into(),
        aliases: vec![],
    });
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    state.pipeline.recompute().await.unwrap();
    let view = crate::services::display::display(&state, 0).await;
    assert_eq!(view["who_whats"][0]["user_id"], "1");
    assert_eq!(
        view["who_whats"][0]["display"], "称呼乙",
        "another member's card must not remap an already resolved CN"
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn saved_recipient_does_not_retarget_after_cache_and_alias_changes() {
    let (state, model) = fixture(false, "").await;
    configure(
        &state,
        json!([{"user_id":"u2","card":"MemberBeta"}]),
        &["u2", "u3"],
    )
    .await;
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("代MemberBeta排:排 徽章 甲 1", 1000))
            .await
            .status,
        "Applied"
    );
    let original = rows(&state).await;
    let cfg = state.cfg.get().await;
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"u3","card":"MemberBeta"}]"#,
    )
    .unwrap();
    state.pipeline.reset().await;
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(result.events[0].user_id.0, "u2");
    assert_eq!(
        serde_json::to_value(rows(&state).await[0].processing.clone()).unwrap(),
        serde_json::to_value(original[0].processing.clone()).unwrap()
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ordinary_proxy_cannot_cancel_or_modify_member_orders_or_execute_nested_control() {
    let (state, model) = fixture(false, "").await;
    configure(
        &state,
        json!([{"user_id":"u2","card":"MemberBeta"}]),
        &["u1", "u2"],
    )
    .await;
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("代MemberBeta排:排 徽章 甲 1", 1000))
            .await
            .status,
        "Applied"
    );
    for (i, text) in [
        "代MemberBeta排:撤 徽章 甲 1",
        "代MemberBeta排:改 徽章 甲 2",
        "代MemberBeta排:/锁位",
    ]
    .into_iter()
    .enumerate()
    {
        let mut ev = incoming(text, 2000 + i as i64);
        if text.ends_with("/锁位") {
            ev.is_admin = true;
        }
        let out = state.pipeline.ingest(ev).await;
        assert_eq!(out.status, "Rejected", "{text}: {}", out.detail);
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert!(!state.pipeline.is_locked().await);
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn known_recipient_phase_and_manual_lock_apply_without_admin_exemption() {
    let (state, _) = fixture(false, "").await;
    configure(
        &state,
        json!([{"user_id":"u2","card":"MemberBeta"}]),
        &["u2"],
    )
    .await;
    let mut cfg = state.cfg.get().await;
    cfg.round.phases = vec![PhaseWindow {
        phase: RoundPhase::Settling,
        start_ms: 0,
        end_ms: 5000,
    }];
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let mut ev = incoming("代MemberBeta排:排 徽章 甲 1", 1000);
    ev.is_admin = true;
    assert_eq!(state.pipeline.ingest(ev).await.status, "Rejected");
    parity(&state).await;
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        0
    );
}

#[tokio::test]
async fn phase_ii_checks_the_recipient_and_not_the_priority_proxy_actor() {
    for (priority, expected) in [(vec!["u2"], "Applied"), (vec!["u1"], "Rejected")] {
        let (state, _) = fixture(false, "").await;
        configure(
            &state,
            json!([{"user_id":"u2","card":"MemberBeta"}]),
            &priority,
        )
        .await;
        let mut cfg = state.cfg.get().await;
        cfg.round.priority_window = None;
        cfg.round.items[0].class = None;
        cfg.round.phases = vec![PhaseWindow {
            phase: RoundPhase::PhaseII,
            start_ms: 0,
            end_ms: 5000,
        }];
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("代MemberBeta排:排 徽章 甲 1", 1000))
                .await
                .status,
            expected
        );
        parity(&state).await;
    }
}

#[tokio::test]
async fn actor_whitelist_cannot_be_borrowed_from_recipient_even_for_ambiguous_target() {
    for text in ["代MemberBeta排:排 徽章 甲 1", "代DK排:排 徽章 甲 1"] {
        let (state, model) = fixture(false, "").await;
        configure(
            &state,
            json!([{"user_id":"u2","card":"MemberBeta"}]),
            &["u2"],
        )
        .await;
        let mut cfg = state.cfg.get().await;
        cfg.gateway.whitelist_members = vec!["u2".into()];
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        assert_eq!(
            state.pipeline.ingest(incoming(text, 1000)).await.status,
            "Dropped"
        );
        parity(&state).await;
        assert_eq!(model.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn product_name_collision_and_empty_segment_are_visible_not_silent_partial_success() {
    let (state, model) = fixture(true, "").await;
    configure(
        &state,
        json!([{"user_id":"u2","card":"徽章"}]),
        &["u1", "u2"],
    )
    .await;
    for (ts, text) in [(1000, "徽章：排 徽章 甲 1"), (2000, "代@u2排:")] {
        let out = state.pipeline.ingest(incoming(text, ts)).await;
        assert_eq!(out.status, "NeedConfirm", "{}", out.detail);
        parity(&state).await;
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn guest_cancel_after_gap_preserves_other_actor_orders_and_uses_the_same_frozen_owner() {
    let (state, _) = fixture(false, "").await;
    configure(&state, json!([]), &["u1", "u2"]).await;
    for (ts, uid, text) in [
        (1000, "u1", "代1排:排 徽章 甲 1"),
        (2000, "u2", "代1排:排 徽章 甲 1"),
        (3000, "u1", "无关闲聊"),
        (4000, "u1", "代1排:撤 徽章 甲 1"),
    ] {
        let mut ev = incoming(text, ts);
        ev.user_id = uid.into();
        ev.nickname = "同名".into();
        state.pipeline.ingest(ev).await;
        parity(&state).await;
    }
    let records = rows(&state).await;
    assert_eq!(records[0].user_id, records[3].user_id);
    assert_ne!(records[0].user_id, records[1].user_id);
    let result = state.pipeline.recompute().await.unwrap();
    assert_eq!(quantity(&result.board), 1);
    assert_eq!(result.board.user_summaries[0].user_id.0, records[1].user_id);
}

#[tokio::test]
async fn anonymous_same_actor_merge_stays_enabled_but_different_proxy_authors_never_merge() {
    let (state, model) = fixture(false, "").await;
    configure(
        &state,
        json!([{"user_id":"u3","card":"MemberBeta"}]),
        &["u1", "u2", "u3"],
    )
    .await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for ts in [1000, 2000] {
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("排 徽章 甲 1(代)", ts))
                .await
                .status,
            "Applied"
        );
        parity(&state).await;
    }
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Merged"
    );
    for (i, uid) in ["u1", "u2"].into_iter().enumerate() {
        let mut ev = incoming("代MemberBeta排:排 徽章 甲 1", 3000 + i as i64);
        ev.user_id = uid.into();
        state.pipeline.ingest(ev).await;
        parity(&state).await;
    }
    let records = rows(&state).await;
    for record in &records[2..] {
        assert_eq!(
            record
                .processing
                .as_ref()
                .unwrap()
                .results
                .last()
                .unwrap()
                .members
                .len(),
            1
        );
    }
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn legacy_anonymous_order_keeps_its_owner_and_can_be_cancelled_without_migrating_old_hash() {
    let (state, _) = fixture(false, "").await;
    configure(&state, json!([]), &["u1"]).await;
    let mut old = incoming("排 徽章 甲 1(代)", 1000);
    old.user_id = "旧称呼(代)".into();
    old.nickname.clear();
    old.sender_user_id = Some("u1".into());
    old.sender_nickname = Some("旧称呼".into());
    assert_eq!(state.pipeline.ingest(old).await.status, "Applied");
    let first = rows(&state).await.remove(0);
    let hash = crate::messages::processing::ProcessingMember::from_record(&first);
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("排 徽章 甲 1(代)", 2000))
            .await
            .status,
        "Applied"
    );
    assert_eq!(rows(&state).await[1].user_id, first.user_id);
    assert_eq!(
        state
            .pipeline
            .ingest(incoming("撤 徽章 甲 2(代)", 3000))
            .await
            .status,
        "Applied"
    );
    let after = rows(&state).await;
    assert_eq!(after[2].user_id, first.user_id);
    assert_eq!(
        crate::messages::processing::ProcessingMember::from_record(&after[0]),
        hash
    );
    parity(&state).await;
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        0
    );
}

#[tokio::test]
async fn legacy_named_guest_without_real_author_needs_check_instead_of_silently_splitting_orders() {
    let (state, model) = fixture(false, "").await;
    configure(&state, json!([]), &["u1"]).await;
    let mut cfg = state.cfg.get().await;
    cfg.round.priority_window = None;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let mut old = incoming("排 徽章 甲 1", 1000);
    old.user_id = "旧称呼(代王哥)".into();
    old.nickname.clear();
    assert_eq!(state.pipeline.process(old).await.status, "Applied");
    let out = state
        .pipeline
        .ingest(incoming("代王哥排:排 徽章 甲 1", 2000))
        .await;
    assert_eq!(out.status, "NeedConfirm");
    assert!(out.detail.contains("旧代排"));
    parity(&state).await;
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        1
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
