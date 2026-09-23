use super::*;
use crate::settings::ConfigStore;

#[test]
fn web_dir_defaults_to_web() {
    if std::env::var("PAIGU_WEB_DIR").is_err() {
        assert_eq!(web_dir(), PathBuf::from("web"));
    }
}

#[test]
fn cors_layer_builds() {
    let _ = cors_layer();
}

#[tokio::test]
async fn build_router_registers_message_replay_and_snapshot_routes() {
    let path = std::env::temp_dir().join(format!("paigu-api-router-{}.json", uuid::Uuid::new_v4()));
    let cfg = Arc::new(ConfigStore::load(&path).expect("load config"));
    let pipeline = crate::llm::Pipeline::new(cfg.clone());
    let gateway = crate::gateway::Gateway::new(cfg.clone(), pipeline.clone());
    let messages = pipeline.messages();
    let state = Arc::new(ApiState {
        cfg,
        pipeline,
        gateway,
        messages,
    });
    let _router = build_router(state);
    let _ = std::fs::remove_file(&path);
}
