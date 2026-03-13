use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::mpsc;

use rover_channel::channeltalk::{ChannelTalkChannel, ChannelTalkConfig};
use rover_channel::ChannelMessage;

#[derive(Debug, Deserialize)]
struct Config {
    server: ServerConfig,
    channels: ChannelsConfig,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct ChannelsConfig {
    channeltalk: ChannelTalkConfig,
}

impl Config {
    fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config_path = std::env::var("GATEWAY_CONFIG")
        .unwrap_or_else(|_| "configs/gateway/config.toml".to_string());

    let config = Config::load(&config_path).unwrap_or_else(|e| {
        eprintln!("Failed to load config: {e}");
        std::process::exit(1);
    });

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!("Starting gateway on {bind_addr}");

    let channel = Arc::new(ChannelTalkChannel::new(config.channels.channeltalk));

    let (tx, rx) = mpsc::channel::<ChannelMessage>(256);

    let webhook_routes = channel.webhook_route(tx);

    let app = webhook_routes
        .route("/health", axum::routing::get(health));

    tokio::spawn(message_handler(rx, Arc::clone(&channel)));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn message_handler(mut rx: mpsc::Receiver<ChannelMessage>, _channel: Arc<ChannelTalkChannel>) {
    while let Some(message) = rx.recv().await {
        tracing::info!(
            channel_id = %message.channel_id,
            sender = %message.sender_name,
            text = %message.text,
            "Processing message"
        );

        // TODO: forward to agent pipeline
        // For now, log the received message
        tracing::info!("Message from {}: {}", message.sender_name, message.text);
    }
}

async fn health() -> &'static str {
    "ok"
}
