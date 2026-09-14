mod config;
mod settings;
mod bus;
mod error;
mod app_state;
mod domain;
mod inbound;
mod parser;
mod engine;
mod repo;
mod services;
mod api;
mod publisher;
mod replay;
mod simulation;
mod ws;
mod gateway;
mod llm;
mod messages;
mod round;
mod settlement;
mod snapshot_bundle;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use tracing_subscriber;

use crate::ws::ws_server::WsServer;

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
    if args.len() > 1 && args[1] == "serve" {
        simulation::chat_server::run_cli(&args[2..]).await?;
        return Ok(());
    }
    if args.len() <= 1 || args[1] == "run" {
        return run_gateway_stack().await;
    }

    tracing::warn!(
        "旧栈已弃用：未识别的子命令 `{}` 落入旧栈（Postgres/旧 WS），请使用 `run`/`simulate`/`serve`",
        args[1]
    );

    let config = config::Config::from_env()?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;

    let app_state = app_state::AppState::build(config.clone(), pool).await?;

    // Spawn WS reverse server on port 3001
    if config.app.ws.enabled {
        let ws_config = config.app.ws.clone();
        let message_service = app_state.services.message.clone();
        let ws_server = Arc::new(WsServer::new(ws_config, message_service));

        tokio::spawn(async move {
            info!("Starting WebSocket server...");
            ws_server.run_forever().await;
        });

        info!("WebSocket server spawned on {}:{}", config.app.ws.host, config.app.ws.port);
    }

    // Start HTTP API on port 8080
    let app = api::routes::build_router(app_state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

    info!("HTTP API listening on 0.0.0.0:8080");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_gateway_stack() -> Result<()> {
    let cfg_path = std::env::var("PAIGU_CONFIG_PATH").unwrap_or_else(|_| "config/app.json".to_string());
    let store = Arc::new(settings::ConfigStore::load(&cfg_path)?);
    store.spawn_watch();
    let cfg = store.get().await;

    let pipeline = llm::Pipeline::new(store.clone());
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
            let wait_ms = (next_daily_ms(&at) - chrono::Utc::now().timestamp_millis()).max(1_000) as u64;
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
