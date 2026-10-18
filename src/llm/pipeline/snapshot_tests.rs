use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming, quantity};
use crate::parser::action_override::ActionSelection;
use crate::services::{
    message_actions,
    snapshot::{self, ExportBody, ImportBody},
};
use std::sync::atomic::Ordering;

async fn export(state: &crate::api::ApiState) -> String {
    let path = std::env::temp_dir().join(format!(
        "paigu-private-test-{}.snapshot.json",
        uuid::Uuid::new_v4()
    ));
    snapshot::snapshot_export(
        state,
        ExportBody {
            overrides: Default::default(),
            out_dir: Some(path.to_string_lossy().into()),
            format: Some("file".into()),
        },
    )
    .await
    .unwrap()["path"]
        .as_str()
        .unwrap()
        .into()
}
async fn preflight(state: &crate::api::ApiState, path: &str) -> serde_json::Value {
    snapshot::snapshot_import(
        state,
        ImportBody {
            path: Some(path.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}
fn apply(path: &str, preflight: &serde_json::Value) -> ImportBody {
    ImportBody {
        path: Some(path.into()),
        apply: true,
        revision: preflight["config_revision"].as_u64(),
        round_id: preflight["target_round_id"].as_str().map(Into::into),
        preflight_token: preflight["preflight_token"].as_str().map(Into::into),
        ..Default::default()
    }
}

#[tokio::test]
async fn private_export_retains_audit_and_guarded_restore_uses_human_selection_without_nc_reinjection(
) {
    let (state, llm) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let (id, mut b) = opened(&state).await;
    b.selection = ActionSelection::Ignore {
        note: "原人工选择".into(),
    };
    message_actions::save(&state, &id, b).await.unwrap();
    let path = export(&state).await;
    let backup = crate::snapshot_bundle::SnapshotBundle::import_file(&path).unwrap();
    assert_eq!(
        backup.settlement["draft"]["round_id"],
        state.cfg.get().await.round.round_id
    );
    assert!(backup
        .events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["payload"]["event_type"] == "ParseOverride"));
    let (id, mut b) = opened(&state).await;
    b.selection = ActionSelection::Reject {
        note: "备份后选择".into(),
    };
    message_actions::save(&state, &id, b).await.unwrap();
    let old = preflight(&state, &path).await;
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    assert!(
        snapshot::snapshot_import(&state, apply(&path, &old))
            .await
            .is_err(),
        "new input invalidates import preflight"
    );
    assert_eq!(state.pipeline.messages_since(0).await.len(), 2);
    let next = preflight(&state, &path).await;
    let result = snapshot::snapshot_import(&state, apply(&path, &next))
        .await
        .unwrap();
    assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["status"],
        "Ignored"
    );
    assert_eq!(
        state.pipeline.messages_since(0).await[0]["manual_note"],
        "原人工选择"
    );
    assert_eq!(
        message_actions::undo_status(&state, next["target_round_id"].as_str().unwrap())
            .await
            .unwrap()["available"],
        false
    );
    let retained: serde_json::Value =
        serde_json::from_slice(&std::fs::read(result["backup_path"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(retained["before_records"].as_array().unwrap().len(), 2);
    assert_eq!(
        quantity(&state.pipeline.recompute().await.unwrap().board),
        0
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn private_import_is_recoverable_at_all_file_boundaries_and_blocks_interleaved_writes() {
    for fault in [4, 5, 6] {
        let (state, llm) = fixture(false, "{}").await;
        state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
        let path = export(&state).await;
        state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
        let prep = preflight(&state, &path).await;
        let round = prep["target_round_id"].as_str().unwrap();
        let record = state.messages.read_only(round).await.unwrap().remove(0);
        state.messages.inject_durable_fault(fault);
        assert!(snapshot::snapshot_import(&state, apply(&path, &prep))
            .await
            .is_err());
        assert_eq!(
            state.pipeline.messages_since(0).await.len(),
            2,
            "failed request did not publish half view"
        );
        assert!(
            state.messages.append(round, &record).await.is_err(),
            "pending replacement blocks new writes"
        );
        state.pipeline.reset().await;
        state.pipeline.recompute().await.unwrap();
        assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
        assert_eq!(
            quantity(&state.pipeline.recompute().await.unwrap().board),
            1
        );
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn private_import_rejects_missing_guard_cross_round_and_locked_target() {
    let (state, _) = fixture(false, "{}").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let path = export(&state).await;
    let prep = preflight(&state, &path).await;
    let mut missing = apply(&path, &prep);
    missing.preflight_token = None;
    assert!(snapshot::snapshot_import(&state, missing).await.is_err());
    let mut cross = apply(&path, &prep);
    cross.round_id = Some("another-round".into());
    assert!(snapshot::snapshot_import(&state, cross).await.is_err());
    state.pipeline.set_locked(true).await.unwrap();
    let prep = preflight(&state, &path).await;
    assert!(snapshot::snapshot_import(&state, apply(&path, &prep))
        .await
        .is_err());
    assert_eq!(state.pipeline.messages_since(0).await.len(), 1);
}
