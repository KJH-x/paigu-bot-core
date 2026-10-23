use super::*;
use crate::llm::{client::LlmClient, Pipeline};
use crate::settings::{default_config, ItemConfig, SubitemConfig};
use crate::snapshot_bundle::SnapshotBundle;
use async_trait::async_trait;
use std::path::PathBuf;

struct NoModel;
#[async_trait]
impl LlmClient for NoModel {
    async fn complete(
        &self,
        _: &crate::settings::LlmSettings,
        _: &str,
        _: &str,
    ) -> anyhow::Result<String> {
        panic!("a real-time control export must not request model understanding");
    }
}

struct Owned {
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
    gateway: Arc<Gateway>,
    directory: PathBuf,
}
impl Drop for Owned {
    fn drop(&mut self) {
        self.server.abort();
        if let Some(worker) = self.gateway.worker.lock().unwrap().take() {
            worker.abort();
        }
        if self.directory.parent() == Some(std::env::temp_dir().as_path())
            && self
                .directory
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("paigu-export-ws-"))
        {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}
fn frame(id: u64, text: &str) -> WsMessage {
    WsMessage::Text(
        json!({"post_type":"message","message_type":"group","group_id":"123456789","user_id":"u1",
        "message_id":id,"message":[{"type":"text","data":{"text":text}}],"time":1000,
        "sender":{"card":"成员01","role":"admin"}})
        .to_string()
        .into(),
    )
}
async fn settled(gateway: &Gateway, pipeline: &Pipeline, count: usize) {
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            if pipeline.messages_since(0).await.len()==count && gateway.status().await["intake"]["pending"]==0 {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("owned intake must commit the control frame instead of waiting forever on its own pending receipt");
}
fn exports(messages: &MessageLog, round: &str) -> Vec<PathBuf> {
    std::fs::read_dir(messages.snapshot_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(round)
        })
        .map(|path| path.join(format!("{round}.snapshot.json")))
        .collect()
}

#[tokio::test]
async fn actual_owned_ws_control_exports_completed_actions_before_its_receipt_and_sends_no_application_frames(
) {
    let directory = std::env::temp_dir().join(format!("paigu-export-ws-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut cfg = default_config();
    cfg.active_round_id = None;
    cfg.round.round_id = format!("ws_export_{}", uuid::Uuid::new_v4().simple());
    cfg.round.category = crate::round::RoundCategory::Test;
    cfg.round.group_id = "123456789".into();
    cfg.round.priority_window = None;
    cfg.round.priority_users.clear();
    cfg.round.phases.clear();
    cfg.gateway.test_group_id = Some(cfg.round.group_id.clone());
    cfg.gateway.whitelist_groups = vec![cfg.round.group_id.clone()];
    cfg.gateway.whitelist_members.clear();
    cfg.gateway.reply_enabled = false;
    cfg.gateway.admin_commands_enabled = true;
    cfg.gateway.accept_forged_messages = false;
    cfg.gateway.allowed_actions.clear();
    cfg.gateway.react_emoji_id = None;
    cfg.members.cache_path = directory.join("members.json").display().to_string();
    cfg.members.cn_overrides.clear();
    cfg.llm.enabled = false;
    cfg.llm.fallback_to_rules = true;
    cfg.merge.enabled = false;
    cfg.round.items = vec![ItemConfig {
        item_id: "badge".into(),
        name: "徽章".into(),
        kind: "拼团".into(),
        class: None,
        aliases: vec![],
        unit_price_cents: 1000,
        max_quantity: None,
        box_item_id: None,
        subitems: vec![SubitemConfig {
            subitem_id: "a".into(),
            name: "款式甲".into(),
            aliases: vec![],
            unit_price_cents: 1000,
            adjust_cents: 0,
            capacity: Some(2),
        }],
    }];
    let path = directory.join("config.json");
    std::fs::write(&path, serde_json::to_vec(&cfg).unwrap()).unwrap();
    let store = Arc::new(ConfigStore::load(path).unwrap());
    let pipeline =
        Pipeline::new_with_client_dir(store.clone(), Arc::new(NoModel), directory.join("messages"));
    let messages = pipeline.messages();
    let gateway = Gateway::new(store, pipeline.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let gw = gateway.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        gw.handle_connection(socket, 1).await
    });
    let _owned = Owned {
        server,
        gateway: gateway.clone(),
        directory,
    };
    let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    client.send(frame(101, "排 徽章 款式甲 1")).await.unwrap();
    settled(&gateway, &pipeline, 1).await;
    assert_eq!(pipeline.messages_since(0).await[0]["status"], "Applied");
    let before = pipeline.board().await;
    let control = frame(9007199254740993, "/导出");
    client.send(control.clone()).await.unwrap();
    settled(&gateway, &pipeline, 2).await;
    let view = pipeline.messages_since(0).await;
    assert_eq!(view[1]["status"], "Applied");
    assert!(view[1]["detail"].as_str().unwrap().contains("本指令前"));
    assert_eq!(
        pipeline.board().await,
        before,
        "read-only export cannot change live business projection"
    );
    let files = exports(&messages, &cfg.round.round_id);
    assert_eq!(files.len(), 1);
    let bundle = SnapshotBundle::import_file(&files[0]).unwrap();
    assert_eq!(bundle.messages.as_array().unwrap().len(), 1);
    assert!(bundle
        .events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["message_id"].as_u64() == Some(9007199254740993)));
    assert_eq!(bundle.settlement["draft"]["round_id"], cfg.round.round_id);
    client.send(control).await.unwrap();
    settled(&gateway, &pipeline, 3).await;
    assert_eq!(pipeline.messages_since(0).await[2]["status"], "Duplicate");
    assert_eq!(
        exports(&messages, &cfg.round.round_id),
        files,
        "upstream duplicate cannot export another file"
    );
    let mut application_frames = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        while let Some(message) = client.next().await {
            if let WsMessage::Text(text) = message.unwrap() {
                application_frames.push(text);
            }
        }
    })
    .await;
    assert!(
        application_frames.is_empty(),
        "no sends, member pulls, model calls or reactions on the fake receiver"
    );
    let _ = client.close(None).await;
    let worker = gateway.worker.lock().unwrap().take();
    if let Some(worker) = worker {
        worker.abort();
        let _ = worker.await;
    }
}
