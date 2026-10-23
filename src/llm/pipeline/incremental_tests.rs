use super::automatic_tests::{fixture, incoming, quantity};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn fresh_cut_survives_parked_switch_back_backdated_append_and_completed_ingress_retries() {
    use crate::services::rounds;
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    let mut old = incoming("排 徽章 甲 1", 1000);
    old.ingress = Some(crate::bus::IngressSource {
        id: "old-frame".into(),
        part: "0".into(),
    });
    assert_eq!(state.pipeline.ingest(old.clone()).await.status, "Applied");
    let cfg = state.cfg.get().await;
    rounds::activate_round(
        &state,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("fresh".into()),
        },
    )
    .await
    .unwrap();
    let mut new = incoming("排 徽章 甲 1", 2000);
    new.ingress = Some(crate::bus::IngressSource {
        id: "new-frame".into(),
        part: "0".into(),
    });
    assert_eq!(state.pipeline.ingest(new.clone()).await.status, "Applied");
    let before = state.pipeline.board().await;
    assert_eq!(state.pipeline.ingest(old).await.status, "AlreadyDelivered");
    assert_eq!(state.pipeline.ingest(new).await.status, "AlreadyDelivered");
    assert_eq!(state.pipeline.board().await, before);
    assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
    assert!(
        !state
            .messages
            .computed_path(&cfg.round.round_id)
            .unwrap()
            .exists(),
        "部分板不能冒充完整源检查点"
    );
    let other = format!("tail_{}", uuid::Uuid::new_v4().simple());
    rounds::create_round(
        &state,
        rounds::CreateRoundBody {
            round_id: other.clone(),
            title: None,
            copy_from: Some(cfg.round.round_id.clone()),
            category: None,
        },
    )
    .await
    .unwrap();
    rounds::activate_round(
        &state,
        &other,
        rounds::ActivateBody {
            mode: Some("fresh".into()),
        },
    )
    .await
    .unwrap();
    rounds::activate_round(
        &state,
        &cfg.round.round_id,
        rounds::ActivateBody {
            mode: Some("continue".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(state.pipeline.board().await, before);
    state.pipeline.ingest(incoming("排 徽章 甲 1", 500)).await;
    let board = serde_json::from_value(state.pipeline.board().await.1).unwrap();
    assert_eq!(
        quantity(&board),
        2,
        "按记录身份cut，不因新消息时间更早而丢弃它"
    );
    assert_eq!(state.pipeline.messages_since(0).await.len(), 2);
    let all = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(all.len(), 3);
    let current = state.cfg.get().await;
    let explicit = state.pipeline.recompute().await.unwrap();
    assert_eq!(explicit.messages.len(), 3, "显式全量重算保留原完整源合同");
    assert!(state.pipeline.state.lock().await.source_cut.is_none());
    assert!(state
        .messages
        .computed_path(&current.round.round_id)
        .unwrap()
        .is_file());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fresh_followed_by_append_does_not_revive_old_orders_or_merge_across_the_cut() {
    for merge in [false, true] {
        let (state, _) = fixture(false, "").await;
        let mut cfg = state.cfg.get().await;
        cfg.merge.enabled = merge;
        cfg.merge.same_sender = true;
        state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("排 徽章 甲 1", 1000))
                .await
                .status,
            "Applied"
        );
        let cfg = state.cfg.get().await;
        crate::services::rounds::activate_round(
            &state,
            &cfg.round.round_id,
            crate::services::rounds::ActivateBody {
                mode: Some("fresh".into()),
            },
        )
        .await
        .unwrap();
        assert!(state.pipeline.board().await.1.is_null());
        assert_eq!(
            state
                .pipeline
                .ingest(incoming("排 徽章 甲 1", 2000))
                .await
                .status,
            "Applied"
        );
        let board = serde_json::from_value(state.pipeline.board().await.1).unwrap();
        assert_eq!(quantity(&board), 1, "merge={merge}: fresh后不恢复原认购");
        assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
        assert_eq!(
            state
                .messages
                .read_only(&cfg.round.round_id)
                .await
                .unwrap()
                .len(),
            2,
            "cut不能删除原消息"
        );
    }
}

#[tokio::test]
async fn real_frozen_merge_ingress_uses_one_tail_evaluation_and_keeps_complete_replay_and_checkpoint(
) {
    let (state, llm) = fixture(false, "").await;
    let mut cfg = state.cfg.get().await;
    cfg.merge.enabled = true;
    cfg.merge.same_sender = true;
    state.cfg.put(cfg.clone(), cfg.revision).await.unwrap();
    for index in 0..8 {
        let mut ev = incoming("排 徽章 甲 1", 1000 + index);
        ev.user_id = format!("member{index}");
        ev.nickname = format!("成员{index}");
        assert_eq!(state.pipeline.ingest(ev).await.status, "Applied");
        let work = state.pipeline.state.lock().await.memo.work;
        assert_eq!(work.evaluated_groups, 1);
        assert_eq!(work.reused_groups, index as usize);
    }
    let mut ev = incoming("排 徽章 甲 2", 2000);
    ev.user_id = "member7".into();
    ev.nickname = "成员07新名".into();
    assert_eq!(state.pipeline.ingest(ev).await.status, "Applied");
    let work = state.pipeline.state.lock().await.memo.work;
    assert_eq!(work.evaluated_groups, 1);
    assert_eq!(work.reused_groups, 7);
    let cfg = state.cfg.get().await;
    let full = crate::replay::session::replay(&state.messages, &cfg, Default::default())
        .await
        .unwrap();
    assert_eq!(
        state.pipeline.board().await.1,
        serde_json::to_value(&full.board).unwrap()
    );
    assert_eq!(state.pipeline.messages_since(0).await.len(), 9);
    let view = state.pipeline.replay_view().await.unwrap();
    assert_eq!(
        view["steps"].as_array().unwrap().len(),
        9,
        "组内原消息仍有独立复盘步"
    );
    let _guard = state.pipeline.message_write_guard().await;
    let (cached, hit) = crate::replay::computed::replay(
        &state.messages,
        &cfg,
        crate::replay::computed::ReadPolicy::Prefer,
    )
    .await
    .unwrap();
    assert!(hit);
    assert_eq!(
        serde_json::to_value(&cached).unwrap(),
        serde_json::to_value(&full).unwrap()
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}
