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
#[cfg(test)]
mod tests;

use anyhow::Result;
use tracing_subscriber;

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

    let app_state = app_state::AppState::build(config, pool).await?;
    let app = api::routes::build_router(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
