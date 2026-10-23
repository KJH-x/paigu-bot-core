use super::action_tests::opened;
use super::automatic_tests::{fixture, incoming};
use crate::parser::action_override::ActionSelection;
use crate::services::{message_actions, snapshot::transfer};
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

struct Server {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(state: crate::api::ApiState) -> (Arc<crate::api::ApiState>, Server) {
    let state = Arc::new(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = crate::api::build_router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (state, Server { base, task })
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
}
async fn context(state: &crate::api::ApiState) -> Value {
    let cfg = state.cfg.get().await;
    json!({"round_id":cfg.round.round_id,"config_revision":cfg.revision,"private_ack":true})
}
async fn download(server: &Server, body: &Value) -> reqwest::Response {
    client()
        .post(format!("{}/api/snapshot/download", server.base))
        .json(body)
        .send()
        .await
        .unwrap()
}
async fn upload(
    server: &Server,
    ctx: &Value,
    bytes: &[u8],
    token: Option<&str>,
) -> reqwest::Response {
    let mut query = vec![
        ("round_id", ctx["round_id"].as_str().unwrap().to_string()),
        ("revision", ctx["config_revision"].to_string()),
        ("apply", token.is_some().to_string()),
    ];
    if let Some(token) = token {
        query.push(("preflight_token", token.into()));
    }
    client()
        .post(format!("{}/api/snapshot/import-file", server.base))
        .query(&query)
        .header("Content-Type", "application/json")
        .body(bytes.to_vec())
        .send()
        .await
        .unwrap()
}
fn private_headers(response: &reqwest::Response) {
    assert_eq!(response.headers()["cache-control"], "no-store, private");
    assert_eq!(response.headers()["pragma"], "no-cache");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn private_browser_download_is_a_no_store_attachment_not_a_server_path() {
    let (state, _) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let response = download(&server, &context(&state).await).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    private_headers(&response);
    let name = response.headers()["content-disposition"].to_str().unwrap();
    let hash = name
        .strip_prefix("attachment; filename=\"paigu-private-")
        .unwrap()
        .strip_suffix(".snapshot.json\"")
        .unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    let data: serde_json::Value = response.json().await.unwrap();
    assert_eq!(data["format"], "paigu-snapshot/1");
    assert!(data.get("path").is_none());
}

#[tokio::test]
async fn download_waits_for_the_common_writer_then_rechecks_observed_context() {
    let (state, llm) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let original = context(&state).await;
    let guard = state.pipeline.message_write_guard().await;
    let url = format!("{}/api/snapshot/download", server.base);
    let task =
        tokio::spawn(async move { client().post(url).json(&original).send().await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        !task.is_finished(),
        "a second state cut cannot bypass the shared writer"
    );
    let mut cfg = state.cfg.get().await;
    cfg.display.refresh_ms += 1;
    let revision = cfg.revision;
    state.cfg.put(cfg, revision).await.unwrap();
    drop(guard);
    let response = task.await.unwrap();
    assert_eq!(response.status().as_u16(), 409);
    private_headers(&response);
    assert!(!response.headers().contains_key("content-disposition"));
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn private_transfer_origin_and_context_fail_before_export_and_errors_are_not_cacheable() {
    let (state, llm) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let original = context(&state).await;
    for (field, value, expected) in [
        ("private_ack", json!(false), 400),
        ("round_id", json!("other"), 409),
        ("config_revision", json!(999), 409),
        ("unexpected", json!(true), 422),
    ] {
        let mut body = original.clone();
        body[field] = value;
        let response = download(&server, &body).await;
        assert_eq!(response.status().as_u16(), expected);
        private_headers(&response);
    }
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("private_ack");
    assert_eq!(download(&server, &missing).await.status().as_u16(), 422);
    for origin in [
        "null",
        "https://example.com",
        "http://localhost.example.com",
        "http://user@localhost",
        "http://localhost/?query=1",
        "http://localhost/#part",
    ] {
        for endpoint in ["download", "import-file"] {
            let response = client()
                .post(format!("{}/api/snapshot/{endpoint}", server.base))
                .header("Origin", origin)
                .header("Content-Type", "application/json")
                .body("not json")
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status().as_u16(),
                403,
                "origin before malformed payload: {origin}"
            );
            private_headers(&response);
        }
    }
    for origin in ["http://127.0.0.1:1234", "http://localhost:1234"] {
        let response = client()
            .post(format!("{}/api/snapshot/download", server.base))
            .header("Origin", origin)
            .json(&original)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        private_headers(&response);
        assert_eq!(response.headers()["access-control-allow-origin"], origin);
        assert!(response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .contains("content-disposition"));
    }
    let response = client()
        .post(format!("{}/api/snapshot/download", server.base))
        .header("Origin", "http://localhost")
        .header("Origin", "http://127.0.0.1")
        .json(&original)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        client()
            .get(format!("{}/api/snapshot/download", server.base))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        405
    );
    assert!(state.pipeline.messages_since(0).await.is_empty());
    assert_eq!(context(&state).await, original);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn raw_file_round_trip_keeps_large_integers_original_understanding_and_human_audit() {
    let (state, llm) = fixture(false, "").await;
    state.pipeline.process(incoming("排 徽章 甲 1", 1000)).await;
    let cfg = state.cfg.get().await;
    state
        .messages
        .append_raw_event(
            &cfg.round.round_id,
            &json!({"opaque_id":9007199254740993_u64}),
        )
        .await
        .unwrap();
    let original = state.messages.read_only(&cfg.round.round_id).await.unwrap()[0].clone();
    let (id, mut body) = opened(&state).await;
    body.selection = ActionSelection::Ignore {
        note: "占位管理解释".into(),
    };
    message_actions::save(&state, &id, body).await.unwrap();
    let (state, server) = server(state).await;
    let ctx = context(&state).await;
    let bytes = download(&server, &ctx).await.bytes().await.unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("9007199254740993"));
    let file: crate::snapshot_bundle::SnapshotFile = serde_json::from_slice(&bytes).unwrap();
    let bundle = crate::snapshot_bundle::SnapshotBundle::from_file(file).unwrap();
    assert_eq!(
        bundle.messages[0]["processing"],
        serde_json::to_value(&original.processing).unwrap()
    );
    let response = upload(&server, &ctx, &bytes, None).await;
    assert_eq!(response.status().as_u16(), 200);
    private_headers(&response);
    let prep: Value = response.json().await.unwrap();
    assert_eq!(prep["applied"], false);
    state.pipeline.process(incoming("排 徽章 甲 2", 2000)).await;
    assert_eq!(
        upload(&server, &ctx, &bytes, prep["preflight_token"].as_str())
            .await
            .status()
            .as_u16(),
        409
    );
    assert_eq!(state.pipeline.messages_since(0).await.len(), 2);
    let prep: Value = upload(&server, &ctx, &bytes, None)
        .await
        .json()
        .await
        .unwrap();
    let response = upload(&server, &ctx, &bytes, prep["preflight_token"].as_str()).await;
    assert_eq!(response.status().as_u16(), 200);
    let applied: Value = response.json().await.unwrap();
    assert_eq!(applied["applied"], true);
    assert!(std::path::Path::new(applied["backup_path"].as_str().unwrap()).is_file());
    let visible = state.pipeline.messages_since(0).await;
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0]["status"], "Ignored");
    assert_eq!(visible[0]["manual_note"], "占位管理解释");
    let restored = state.messages.read_only(&cfg.round.round_id).await.unwrap();
    assert_eq!(restored[0].record_id, original.record_id);
    assert_eq!(
        serde_json::to_value(&restored[0].processing).unwrap(),
        serde_json::to_value(&original.processing).unwrap()
    );
    let events = state
        .messages
        .read_events_strict(&cfg.round.round_id)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event["opaque_id"].as_u64() == Some(9007199254740993)));
    assert_eq!(state.cfg.get().await.revision, cfg.revision);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn raw_file_rejects_tamper_unknown_format_malformed_query_stale_context_and_locked_apply() {
    let (state, _) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let ctx = context(&state).await;
    let bytes = download(&server, &ctx).await.bytes().await.unwrap();
    let file: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, replacement) in [("format", json!("unknown")), ("config", json!({}))] {
        let mut bad = file.clone();
        bad[field] = replacement;
        let response = upload(&server, &ctx, &serde_json::to_vec(&bad).unwrap(), None).await;
        assert_eq!(response.status().as_u16(), 400);
        private_headers(&response);
    }
    for bytes in [b"not json".as_slice(), b"{}".as_slice()] {
        let response = upload(&server, &ctx, bytes, None).await;
        assert!(matches!(response.status().as_u16(), 400 | 422));
        private_headers(&response);
    }
    let response = client()
        .post(format!(
            "{}/api/snapshot/import-file?round_id=x&revision=0&surprise=1",
            server.base
        ))
        .header("Content-Type", "application/json")
        .body(bytes.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 400);
    for (field, replacement) in [
        ("round_id", json!("other")),
        ("config_revision", json!(999)),
    ] {
        let mut stale = ctx.clone();
        stale[field] = replacement;
        assert_eq!(
            upload(&server, &stale, &bytes, None)
                .await
                .status()
                .as_u16(),
            409
        );
    }
    assert_eq!(
        upload(&server, &ctx, &bytes, Some("wrong-token"))
            .await
            .status()
            .as_u16(),
        409
    );
    state.pipeline.set_locked(true).await.unwrap();
    let prep: Value = upload(&server, &ctx, &bytes, None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        upload(&server, &ctx, &bytes, prep["preflight_token"].as_str())
            .await
            .status()
            .as_u16(),
        423
    );
    assert!(state.pipeline.messages_since(0).await.is_empty());
}

#[tokio::test]
async fn file_size_limit_refuses_whole_oversize_bodies_and_pending_ingress_or_corrupt_logs_never_export_empty(
) {
    assert!(transfer::check_size(transfer::MAX_FILE_BYTES).is_ok());
    assert!(matches!(
        transfer::check_size(transfer::MAX_FILE_BYTES + 1),
        Err(crate::services::ServiceError::PayloadTooLarge(_))
    ));
    let (state, _) = fixture(false, "").await;
    let (state, server) = server(state).await;
    let ctx = context(&state).await;
    let response = upload(
        &server,
        &ctx,
        &vec![b' '; transfer::MAX_FILE_BYTES + 1],
        None,
    )
    .await;
    assert_eq!(response.status().as_u16(), 413);
    private_headers(&response);
    let round = ctx["round_id"].as_str().unwrap();
    let pending = state
        .messages
        .ingress_dir()
        .join(round)
        .join("00000000000000000001.json");
    std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
    std::fs::write(&pending, b"pending fixture").unwrap();
    let response = download(&server, &ctx).await;
    assert_eq!(response.status().as_u16(), 409);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"],
        "pending_ingress"
    );
    std::fs::remove_file(pending).unwrap();
    let cfg = state.cfg.get().await;
    let path = std::path::Path::new(&cfg.members.cache_path)
        .parent()
        .unwrap()
        .join("messages")
        .join(format!("{round}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"corrupt message line\n").unwrap();
    let response = download(&server, &ctx).await;
    assert_eq!(response.status().as_u16(), 500);
    private_headers(&response);
    assert!(!response.headers().contains_key("content-disposition"));
}
