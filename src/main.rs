// 旧栈删除后，仍有少量「保留但未接线」的公共 API（如 domain 的 Public* 视图模型、
// parser 的部分条目、engine::event_store），待后续 Wave 接线；统一在此静音 dead_code。
#![allow(dead_code)]

mod api;
mod bus;
mod domain;
mod engine;
mod error;
mod gateway;
mod llm;
mod messages;
mod parser;
mod planner;
mod replay;
mod round;
mod settings;
mod settlement;
mod simulation;
mod snapshot_bundle;
#[cfg(test)]
mod tests;

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
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
    store.spawn_watch();
    let cfg = store.get().await;

    let messages = messages::MessageLog::from_env();
    let pipeline = llm::Pipeline::new_with_messages(
        store.clone(),
        Arc::new(llm::client::OpenAiClient::new()),
        messages.clone(),
    );
    let gateway = gateway::Gateway::new(store.clone(), pipeline.clone());

    {
        let gw = gateway.clone();
        tokio::spawn(async move {
            if let Err(e) = gw.run().await {
                tracing::error!("gateway stopped: {e}");
            }
        });
    }

    let port: u16 = std::env::var("PAIGU_HTTP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21081);

    info!(
        "gateway bind={} reply_enabled={} http=127.0.0.1:{}",
        cfg.gateway.bind, cfg.gateway.reply_enabled, port
    );

    let state = Arc::new(api::ApiState {
        cfg: store.clone(),
        pipeline: pipeline.clone(),
        gateway: gateway.clone(),
        messages,
    });
    spawn_members_scheduler(state.clone());
    api::serve(state, port).await
}

fn next_daily_ms(hhmm: &str) -> i64 {
    use chrono::{Duration, Local, TimeZone};
    let (h, m) = hhmm
        .split_once(':')
        .map(|(h, m)| {
            (
                h.trim().parse::<u32>().unwrap_or(19),
                m.trim().parse::<u32>().unwrap_or(0),
            )
        })
        .unwrap_or((19, 0));
    let now = Local::now();
    let naive = now
        .date_naive()
        .and_hms_opt(h, m, 0)
        .unwrap_or_else(|| now.naive_local());
    let mut dt = Local.from_local_datetime(&naive).single().unwrap_or(now);
    if dt <= now {
        dt += Duration::hours(24);
    }
    dt.timestamp_millis()
}

/// 每日在 `members.daily_pull_at`（默认 19:00）尝试拉取群成员并缓存；失败只告警。
fn spawn_members_scheduler(state: Arc<api::ApiState>) {
    tokio::spawn(async move {
        loop {
            let at = state.cfg.get().await.members.daily_pull_at.clone();
            let wait_ms =
                (next_daily_ms(&at) - chrono::Utc::now().timestamp_millis()).max(1_000) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            match api::member_routes::refresh_members_from_gateway(&state).await {
                Ok(v) => info!(
                    "members refreshed: {} entries",
                    v.as_array().map(|a| a.len()).unwrap_or(0)
                ),
                Err(e) => tracing::warn!("members refresh failed: {e}"),
            }
        }
    });
}
