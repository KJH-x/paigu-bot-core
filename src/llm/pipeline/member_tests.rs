use super::automatic_tests::fixture;
use crate::services::{
    members::{self, MemberContext},
    ServiceError,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

async fn context(state: &crate::api::ApiState) -> MemberContext {
    let view = members::get_members(state).await;
    MemberContext {
        round_id: view["round_id"].as_str().unwrap().into(),
        config_revision: view["config_revision"].as_u64().unwrap(),
        members_fingerprint: view["members_fingerprint"].as_str().unwrap().into(),
    }
}
async fn cache(state: &crate::api::ApiState) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(state.cfg.get().await.members.cache_path);
    std::fs::write(&path, br#"[{"user_id":1,"card":"member-one note","nickname":"unused-nickname","raw_private":"must-not-send"}]"#).unwrap();
    path
}

#[tokio::test]
async fn member_suggestion_is_guarded_explicit_minimal_and_never_writes_cache_or_choices() {
    let (state, model) = fixture(true, r#"{"items":[{"user_id":"1","cn":"成员01"}]}"#).await;
    let path = cache(&state).await;
    let before = std::fs::read(&path).unwrap();
    let cfg = state.cfg.get().await;
    let guard = context(&state).await;
    let suggested = members::suggest_cn(&state, guard.clone()).await.unwrap();
    assert_eq!(suggested["suggestions"][0]["cn"], "成员01");
    assert_eq!(suggested["executed"], false);
    assert_eq!(suggested["members_fingerprint"], guard.members_fingerprint);
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(state
        .messages
        .read_events_strict(&cfg.round.round_id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    let prompts = model.prompts.lock().unwrap();
    assert!(prompts[0].1.contains("member-one note"));
    for private in [
        "raw_private",
        "must-not-send",
        "unused-nickname",
        "123456789",
        "cn_overrides",
    ] {
        assert!(!prompts[0].1.contains(private));
    }
}

#[tokio::test]
async fn disabled_stale_or_foreign_member_request_never_calls_model() {
    let (state, model) = fixture(false, r#"{"items":[]}"#).await;
    cache(&state).await;
    let guard = context(&state).await;
    assert!(matches!(
        members::suggest_cn(&state, guard.clone()).await,
        Err(ServiceError::Unprocessable(_))
    ));
    let mut wrong = guard.clone();
    wrong.round_id = "other-round".into();
    assert!(matches!(
        members::suggest_cn(&state, wrong).await,
        Err(ServiceError::Conflict(_))
    ));
    let mut old = guard.clone();
    old.config_revision += 1;
    assert!(matches!(
        members::suggest_cn(&state, old).await,
        Err(ServiceError::StaleRevision(_))
    ));
    let mut old = guard;
    old.members_fingerprint = "stale".into();
    assert!(matches!(
        members::suggest_cn(&state, old).await,
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_atomic_member_cache_install_keeps_previous_file_and_target_directory() {
    let (state, _) = fixture(false, "").await;
    let path = cache(&state).await;
    let before = std::fs::read(&path).unwrap();
    let target = path.parent().unwrap();
    let mut cfg = state.cfg.get().await;
    let revision = cfg.revision;
    cfg.members.cache_path = target.display().to_string();
    state.cfg.put(cfg, revision).await.unwrap();
    let result =
        members::install_member_cache(&state, context(&state).await, json!([{"user_id":1}])).await;
    assert!(matches!(result, Err(ServiceError::Internal(_))));
    assert!(target.is_dir());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[tokio::test]
async fn malformed_unknown_or_duplicate_cn_suggestions_cannot_mutate_members() {
    for reply in [
        "not json",
        r#"{"items":[{"user_id":"unknown","cn":"成员02"}]}"#,
        r#"{"items":[{"user_id":"1","cn":"成员01"},{"user_id":"1","cn":"成员02"}]}"#,
        r#"{"items":[],"execute":true}"#,
        r#"{"items":[{"user_id":1,"cn":"成员01"}]}"#,
        r#"{"items":[{"user_id":"1","cn":"\n"}]}"#,
    ] {
        let (state, model) = fixture(true, reply).await;
        let path = cache(&state).await;
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            members::suggest_cn(&state, context(&state).await).await,
            Err(ServiceError::Unprocessable(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    }
}

struct ChangingModel {
    config: Arc<crate::settings::ConfigStore>,
    messages: Arc<crate::messages::MessageLog>,
    cache: std::path::PathBuf,
    change_config: bool,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl crate::llm::client::LlmClient for ChangingModel {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _writer = self.messages.write_guard().await;
        if self.change_config {
            let mut cfg = self.config.get().await;
            let revision = cfg.revision;
            cfg.display.refresh_ms += 1;
            self.config.put(cfg, revision).await?;
        } else {
            std::fs::write(&self.cache, br#"[{"user_id":1,"card":"changed-card"}]"#)?;
        }
        Ok(r#"{"items":[{"user_id":"1","cn":"成员01"}]}"#.into())
    }
}
#[tokio::test]
async fn model_wait_does_not_hold_writer_and_changed_config_or_cache_discards_suggestion() {
    for change_config in [false, true] {
        let (mut state, _) = fixture(true, "").await;
        let path = cache(&state).await;
        let model = Arc::new(ChangingModel {
            config: state.cfg.clone(),
            messages: state.messages.clone(),
            cache: path.clone(),
            change_config,
            calls: AtomicUsize::new(0),
        });
        state.pipeline = super::Pipeline::new_with_messages(
            state.cfg.clone(),
            model.clone(),
            state.messages.clone(),
        );
        let guard = context(&state).await;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            members::suggest_cn(&state, guard),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Err(ServiceError::Conflict(_)) | Err(ServiceError::StaleRevision(_))
        ));
        assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn member_cache_install_is_guarded_and_atomic_with_group_provenance() {
    let (state, model) = fixture(false, "").await;
    let path = cache(&state).await;
    let original = std::fs::read(&path).unwrap();
    let guard = context(&state).await;
    for invalid in [
        Value::Null,
        json!([{"nickname":"成员01"}]),
        json!([{"user_id":1},{"user_id":"1"}]),
    ] {
        assert!(
            members::install_member_cache(&state, guard.clone(), invalid)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let mut old = guard.clone();
    old.config_revision += 1;
    assert!(matches!(
        members::install_member_cache(&state, old, json!([])).await,
        Err(ServiceError::StaleRevision(_))
    ));
    let result = members::install_member_cache(
        &state,
        guard.clone(),
        json!([{"user_id":1,"card":"成员01"}]),
    )
    .await
    .unwrap();
    assert_eq!(result["cached_group_id"], "123456789");
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["group_id"], "123456789");
    assert!(saved["refreshed_at_ms"].as_i64().unwrap() > 0);
    assert_eq!(
        members::get_members(&state).await["members"][0]["user_id"],
        "1"
    );
    assert!(matches!(
        members::install_member_cache(&state, guard, json!([])).await,
        Err(ServiceError::Conflict(_))
    ));
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}
