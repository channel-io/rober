mod config;
mod channeltalk;
mod filter;
mod webhook;
mod ws;

use std::sync::Arc;

use tracing_subscriber;

use crate::config::Config;
use crate::ws::ConnectionRegistry;

pub struct AppState {
    pub config: Config,
    pub registry: ConnectionRegistry,
    pub http_client: reqwest::Client,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::load("configs/messenger/config.toml").unwrap_or_else(|e| {
        eprintln!("Failed to load config: {e}");
        std::process::exit(1);
    });

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!("Starting messenger-server on {bind_addr}");

    let state = Arc::new(AppState {
        config,
        registry: ConnectionRegistry::new(),
        http_client: reqwest::Client::new(),
    });

    let app = axum::Router::new()
        .route("/webhook/channeltalk", axum::routing::post(webhook::handle_webhook))
        .route("/ws", axum::routing::get(ws::handle_ws_upgrade))
        .route("/health", axum::routing::get(health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> &'static str {
    "ok"
}
