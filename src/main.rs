mod config;
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
