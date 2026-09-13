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
mod audit;
mod storage;
mod ws;
mod gateway;
mod llm;
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
    api::serve(state, port).await
}
