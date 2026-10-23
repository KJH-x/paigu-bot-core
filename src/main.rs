// 新栈（gateway + llm pipeline + api + planner/settlement）已完整接线；
// 为后续 Wave 保留的少量公共 API 在各自条目上按需加**精确** `#[allow(dead_code)]`。

mod api;
mod bus;
mod domain;
mod engine;
mod gateway;
mod llm;
mod messages;
mod parser;
mod persistence;
mod planner;
mod replay;
mod round;
mod services;
mod settings;
mod settlement;
mod simulation;
mod snapshot_bundle;
#[cfg(test)]
mod tests;

use anyhow::Result;
use std::io::IsTerminal;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        // 重定向到文件时不写 ANSI 颜色码（终端下保留颜色；需 use std::io::IsTerminal）
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "simulate" {
        simulation::verifier::run_cli(&args[2..]).await?;
        return Ok(());
    }
    if args.len() <= 1 || args[1] == "run" || args[1] == "serve" {
        return run_gateway_stack().await;
    }

    tracing::warn!(
        "未识别的子命令 `{}`：已统一到新栈（run/serve），按 `run` 启动",
        args[1]
    );
    run_gateway_stack().await
}

async fn run_gateway_stack() -> Result<()> {
    let cfg_path =
        std::env::var("PAIGU_CONFIG_PATH").unwrap_or_else(|_| "config/app.json".to_string());
    let store = Arc::new(settings::ConfigStore::load(&cfg_path)?);
    let cfg = store.get().await;

    let messages = messages::MessageLog::from_env();
    let pipeline = llm::Pipeline::new_with_messages(
        store.clone(),
        Arc::new(llm::client::OpenAiClient::new()),
        messages.clone(),
    );
    // Verify durable sources before accepting input; derived data is disposable.
    // A corrupt/torn journal stops startup instead of silently reviving old choices.
    pipeline.restore().await?;
    let gateway = gateway::Gateway::new(store.clone(), pipeline.clone());

    let port: u16 = std::env::var("PAIGU_HTTP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21081);

    info!(
        "gateway bind={} reply_enabled={} http=127.0.0.1:{}",
        cfg.gateway.bind, cfg.gateway.reply_enabled, port
    );

    let state = Arc::new(api::ApiState {
        members_pull: Default::default(),
        cfg: store.clone(),
        pipeline: pipeline.clone(),
        gateway: gateway.clone(),
        messages,
    });
    services::settlement::restore_persistent_lock(&state)
        .await
        .map_err(|error| anyhow::anyhow!("恢复结算锁定状态失败：{error:?}"))?;
    {
        let gw = gateway.clone();
        tokio::spawn(async move {
            if let Err(e) = gw.run().await {
                tracing::error!("gateway stopped: {e}");
            }
        });
    }
    let watcher_state = Arc::downgrade(&state);
    store.spawn_watch(move || {
        let weak = watcher_state.clone();
        async move {
            if let Some(state) = weak.upgrade() {
                if let Err(error) = services::configuration::reload(&state, false).await {
                    tracing::warn!("配置热载未应用：{error:?}");
                }
            }
        }
    });
    services::member_pull::spawn(state.clone());
    api::serve(state, port).await
}
