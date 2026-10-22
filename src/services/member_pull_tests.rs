use super::*;
use crate::services::members::{self, MemberContext};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Owned(PathBuf);
impl Drop for Owned {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("paigu-member-pull-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct NoModel;
#[async_trait::async_trait]
impl crate::llm::client::LlmClient for NoModel {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        panic!("member pull must not call a model")
    }
}

async fn fixture() -> (Owned, Arc<ApiState>) {
    let directory =
        Owned(std::env::temp_dir().join(format!("paigu-member-pull-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&directory.0).unwrap();
    let mut cfg = crate::settings::default_config();
    cfg.active_round_id = None;
    cfg.round.round_id = format!("pull_{}", uuid::Uuid::new_v4().simple());
    cfg.round.group_id = "123456789".into();
    cfg.gateway.whitelist_groups = vec![cfg.round.group_id.clone()];
    cfg.gateway.reply_enabled = false;
    cfg.llm.enabled = false;
    cfg.gateway.react_emoji_id = None;
    cfg.gateway.allowed_actions = vec!["get_group_member_list".into()];
    cfg.members.group_id = cfg.round.group_id.clone();
    cfg.members.cache_path = directory.0.join("members.json").display().to_string();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    cfg.gateway.bind = socket.local_addr().unwrap().to_string();
    drop(socket);
    let path = directory.0.join("app.json");
    std::fs::write(&path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    std::fs::write(
        &cfg.members.cache_path,
        r#"[{"user_id":"user01","card":"成员01"}]"#,
    )
    .unwrap();
    let store = Arc::new(crate::settings::ConfigStore::load(path).unwrap());
    let pipeline = crate::llm::Pipeline::new_with_client_dir(
        store.clone(),
        Arc::new(NoModel),
        directory.0.join("messages"),
    );
    let state = Arc::new(ApiState {
        members_pull: Default::default(),
        cfg: store.clone(),
        gateway: crate::gateway::Gateway::new(store, pipeline.clone()),
        messages: pipeline.messages(),
        pipeline,
    });
    (directory, state)
}

async fn context(state: &ApiState) -> MemberContext {
    let value = members::get_members(state).await;
    MemberContext {
        round_id: value["round_id"].as_str().unwrap().into(),
        config_revision: value["config_revision"].as_u64().unwrap(),
        members_fingerprint: value["members_fingerprint"].as_str().unwrap().into(),
    }
}

#[test]
fn next_local_day_is_calendar_based_and_bad_times_are_not_repaired() {
    let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let before = zone.with_ymd_and_hms(2024, 2, 28, 18, 59, 59).unwrap();
    let due = zone.with_ymd_and_hms(2024, 2, 28, 19, 0, 0).unwrap();
    assert_eq!(next_at(before, "19:00"), Some(due.timestamp_millis()));
    assert_eq!(
        next_at(due, "19:00"),
        Some(
            zone.with_ymd_and_hms(2024, 2, 29, 19, 0, 0)
                .unwrap()
                .timestamp_millis()
        )
    );
    let month_end = zone.with_ymd_and_hms(2024, 2, 29, 23, 59, 59).unwrap();
    assert_eq!(
        next_at(month_end, "00:00"),
        Some(
            zone.with_ymd_and_hms(2024, 3, 1, 0, 0, 0)
                .unwrap()
                .timestamp_millis()
        )
    );
    for bad in [
        "",
        "9:00",
        "24:00",
        "19:60",
        "１９:００",
        "19:00:00",
        " 19:00",
    ] {
        assert_eq!(next_at(due, bad), None, "{bad}");
    }
}

#[test]
fn schedule_changes_replan_without_catchup_and_unrelated_revisions_do_not_postpone() {
    let mut cfg = crate::settings::default_config();
    cfg.members.daily_pull_at = "19:00".into();
    let now = chrono::Utc
        .with_ymd_and_hms(2026, 10, 22, 18, 59, 0)
        .unwrap();
    let due = chrono::Utc
        .with_ymd_and_hms(2026, 10, 22, 19, 0, 0)
        .unwrap();
    let mut schedule = Schedule::default();
    assert!(!schedule.tick(&cfg, now));
    let original = schedule.next();
    cfg.revision += 1;
    cfg.llm.model = "unrelated".into();
    assert!(!schedule.tick(&cfg, now));
    assert_eq!(schedule.next(), original);
    assert!(schedule.tick(&cfg, due));
    assert!(!schedule.tick(&cfg, due));
    assert!(schedule.next().unwrap() > due.timestamp_millis());
    cfg.members.daily_pull_at = "19:05".into();
    assert!(!schedule.tick(&cfg, due));
    assert_eq!(schedule.next(), next_at(due, "19:05"));
    cfg.members.cache_path = "replacement.json".into();
    assert!(!schedule.tick(
        &cfg,
        chrono::Utc
            .with_ymd_and_hms(2026, 10, 22, 19, 5, 0)
            .unwrap()
    ));
    cfg.members.daily_pull_at = "bad".into();
    assert!(!schedule.tick(&cfg, due));
    assert_eq!(schedule.next(), None);
}

#[test]
fn health_is_bounded_private_source_scoped_and_cancellation_releases_busy() {
    let cfg = crate::settings::default_config();
    let monitor = Arc::new(Monitor::default());
    let attempt = monitor.begin(&cfg, Origin::Manual).unwrap();
    assert!(monitor.begin(&cfg, Origin::Scheduled).is_err());
    assert_eq!(monitor.snapshot(&cfg)["running"], true);
    drop(attempt);
    assert_eq!(
        monitor.snapshot(&cfg)["last_attempt"]["status"],
        "cancelled"
    );
    let attempt = monitor.begin(&cfg, Origin::Manual).unwrap();
    attempt.finish(&Err(ServiceError::Upstream(
        "private-payload-must-not-be-published".into(),
    )));
    let status = monitor.snapshot(&cfg);
    assert_eq!(status["consecutive_failures"], 2);
    assert!(!status.to_string().contains("private-payload"));
    assert!(!status.to_string().contains(&cfg.members.cache_path));
    monitor
        .begin(&cfg, Origin::Manual)
        .unwrap()
        .finish(&Ok(json!({"members":[{},{}]})));
    assert_eq!(monitor.snapshot(&cfg)["consecutive_failures"], 0);
    assert_eq!(monitor.snapshot(&cfg)["last_attempt"]["member_count"], 2);
    let mut changed = cfg.clone();
    changed.members.group_id = "987654321".into();
    assert!(monitor.snapshot(&changed)["last_attempt"].is_null());
    assert!(monitor.snapshot(&changed)["last_success_at_ms"].is_null());
}

#[tokio::test]
async fn scheduled_failure_is_observable_preserves_cache_and_does_not_loop_retry() {
    let (_owned, state) = fixture().await;
    let cfg = state.cfg.get().await;
    let before = std::fs::read(&cfg.members.cache_path).unwrap();
    let mut schedule = Schedule::default();
    let start = Local.with_ymd_and_hms(2026, 10, 22, 18, 0, 0).unwrap();
    let due = Local.with_ymd_and_hms(2026, 10, 22, 19, 0, 0).unwrap();
    tick(&state, &mut schedule, start).await;
    tick(&state, &mut schedule, due).await;
    let status = state.members_pull.snapshot(&cfg);
    assert_eq!(status["last_attempt"]["origin"], "scheduled");
    assert_eq!(status["last_attempt"]["reason"], "receiver_error");
    assert_eq!(status["consecutive_failures"], 1);
    assert!(status["next_attempt_at_ms"].as_i64().unwrap() > due.timestamp_millis());
    tick(&state, &mut schedule, due + chrono::Duration::seconds(1)).await;
    assert_eq!(state.members_pull.snapshot(&cfg)["consecutive_failures"], 1);
    assert_eq!(std::fs::read(&cfg.members.cache_path).unwrap(), before);
    let mut invalid = cfg.clone();
    invalid.members.daily_pull_at = "25:00".into();
    state.cfg.put(invalid, cfg.revision).await.unwrap();
    tick(&state, &mut schedule, due).await;
    assert_eq!(
        state.members_pull.snapshot(&state.cfg.get().await)["schedule_state"],
        "invalid_time"
    );
}

struct Server(tokio::task::JoinHandle<anyhow::Result<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn receiver(
    state: &Arc<ApiState>,
    hold: Arc<tokio::sync::Semaphore>,
    entered: Arc<tokio::sync::Notify>,
    count: Arc<AtomicUsize>,
) -> (Server, tokio::task::JoinHandle<()>) {
    let gateway = state.gateway.clone();
    let server = Server(tokio::spawn(gateway.run()));
    let bind = state.cfg.get().await.gateway.bind;
    let (mut socket, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let Ok(socket) = tokio_tungstenite::connect_async(format!("ws://{bind}")).await {
                break socket;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let client = tokio::spawn(async move {
        while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
            socket.next().await
        {
            let request: Value = serde_json::from_str(&text).unwrap();
            if request.get("echo").is_none() {
                continue;
            }
            assert_eq!(request["action"], "get_group_member_list");
            assert_eq!(request["params"]["group_id"], "123456789");
            count.fetch_add(1, Ordering::SeqCst);
            entered.notify_one();
            let Ok(permit) = hold.acquire().await else {
                break;
            };
            permit.forget();
            let response = json!({"status":"ok","retcode":0,"echo":request["echo"],"data":[{"user_id":"user01","card":"成员01"},{"user_id":"user02","card":"成员02"}]});
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    response.to_string().into(),
                ))
                .await
                .unwrap();
        }
    });
    (server, client)
}

#[tokio::test]
async fn real_local_echo_recovers_failure_and_busy_does_not_send_a_second_request() {
    let (_owned, state) = fixture().await;
    let cfg = state.cfg.get().await;
    assert!(members::refresh_members(&state, context(&state).await)
        .await
        .is_err());
    let permits = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let count = Arc::new(AtomicUsize::new(0));
    let (_server, client) = receiver(&state, permits.clone(), entered.clone(), count.clone()).await;
    let body = context(&state).await;
    let copy = state.clone();
    let request = body.clone();
    let first = tokio::spawn(async move { members::refresh_members(&copy, request).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert!(
        matches!(members::refresh_members(&state,body).await,Err(ServiceError::Conflict(ref value)) if value["error"]=="members_pull_busy")
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    permits.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap()["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let status = state.members_pull.snapshot(&cfg);
    assert_eq!(status["last_attempt"]["status"], "succeeded");
    assert_eq!(status["consecutive_failures"], 0);
    assert_eq!(members::get_members(&state).await["pull_status"], status);
    client.abort();
    let _ = client.await;
}

#[tokio::test]
async fn stale_schedule_and_cancelled_pull_cannot_install_a_new_source_cache() {
    let (_owned, state) = fixture().await;
    let original = state.cfg.get().await;
    let mut next = original.clone();
    next.members.group_id = "987654321".into();
    state.cfg.put(next, original.revision).await.unwrap();
    assert!(members::refresh_members_from_gateway(&state, &original)
        .await
        .is_err());
    assert!(
        state.members_pull.snapshot(&state.cfg.get().await)["last_attempt"].is_null(),
        "a stale request must not become the replacement source's failed pull"
    );
    let current = state.cfg.get().await;
    let mut restored = original.clone();
    restored.revision = current.revision;
    state.cfg.put(restored, current.revision).await.unwrap();
    let cfg = state.cfg.get().await;
    let before = std::fs::read(&cfg.members.cache_path).unwrap();
    let permits = Arc::new(tokio::sync::Semaphore::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let count = Arc::new(AtomicUsize::new(0));
    let (_server, client) = receiver(&state, permits.clone(), entered.clone(), count.clone()).await;
    let body = context(&state).await;
    let copy = state.clone();
    let task = tokio::spawn(async move { members::refresh_members(&copy, body).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        state.members_pull.snapshot(&cfg)["last_attempt"]["status"],
        "cancelled"
    );
    assert_eq!(std::fs::read(&cfg.members.cache_path).unwrap(), before);
    permits.add_permits(2);
    members::refresh_members(&state, context(&state).await)
        .await
        .unwrap();
    assert_eq!(
        state.members_pull.snapshot(&cfg)["last_attempt"]["status"],
        "succeeded"
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
    client.abort();
    let _ = client.await;
}
