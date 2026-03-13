use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::{Channel, ChannelError, ChannelMessage, OutgoingMessage};

const CHANNELTALK_API_BASE: &str = "https://api.channel.io/open";

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelTalkConfig {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub ignore_senders: Vec<String>,
}

pub struct ChannelTalkChannel {
    config: ChannelTalkConfig,
    client: Client,
}

impl ChannelTalkChannel {
    pub fn new(config: ChannelTalkConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    pub fn webhook_route(&self, tx: mpsc::Sender<ChannelMessage>) -> axum::Router {
        let state = Arc::new(WebhookState {
            tx,
            ignore_senders: self.config.ignore_senders.clone(),
        });
        axum::Router::new()
            .route("/channeltalk", axum::routing::post(handle_webhook))
            .with_state(state)
    }
}

impl Channel for ChannelTalkChannel {
    fn name(&self) -> &'static str {
        "channeltalk"
    }

    async fn send(&self, message: OutgoingMessage) -> Result<(), ChannelError> {
        let mut body = json!({
            "channelId": message.channel_id,
            "body": {
                "text": message.text
            }
        });

        if let Some(parent_id) = &message.reply_to {
            body["parentMessageId"] = json!(parent_id);
        }

        let response = self
            .client
            .post(format!(
                "{CHANNELTALK_API_BASE}/v5/user-chats/{}/messages",
                message.channel_id
            ))
            .header(
                "Authorization",
                format!("Bearer {}", self.config.access_token),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError(format!("HTTP request failed: {e}")))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(ChannelError(format!(
                "Channel Talk API error {status}: {body}"
            )))
        }
    }
}

// --- Webhook types and handler ---

struct WebhookState {
    tx: mpsc::Sender<ChannelMessage>,
    ignore_senders: Vec<String>,
}

#[derive(Deserialize)]
struct WebhookPayload {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    event: String,
    entity: Option<WebhookEntity>,
    refers: Option<WebhookRefers>,
}

#[derive(Deserialize)]
struct WebhookEntity {
    #[serde(default)]
    id: String,
    #[serde(rename = "chatType", default)]
    chat_type: String,
    #[serde(rename = "chatId", default)]
    chat_id: String,
    #[serde(rename = "personId", default)]
    person_id: String,
    #[serde(rename = "plainText", default)]
    plain_text: String,
    #[serde(rename = "createdAt", default)]
    created_at: u64,
}

#[derive(Deserialize)]
struct WebhookRefers {
    manager: Option<ManagerInfo>,
}

#[derive(Deserialize)]
struct ManagerInfo {
    #[serde(default)]
    name: String,
}

impl WebhookPayload {
    fn sender_name(&self) -> String {
        if let Some(refers) = &self.refers {
            if let Some(manager) = &refers.manager {
                if !manager.name.is_empty() {
                    return manager.name.clone();
                }
            }
        }
        self.entity
            .as_ref()
            .map(|e| e.person_id.clone())
            .unwrap_or_default()
    }
}

async fn handle_webhook(
    State(state): State<Arc<WebhookState>>,
    body: Bytes,
) -> StatusCode {
    let payload: WebhookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse webhook payload: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    tracing::info!(event = %payload.event, r#type = %payload.r#type, "Webhook received");

    if payload.r#type != "message" {
        return StatusCode::OK;
    }

    let Some(entity) = &payload.entity else {
        return StatusCode::OK;
    };

    let sender_name = payload.sender_name();

    if !should_accept(
        &entity.chat_type,
        &entity.plain_text,
        &sender_name,
        &state.ignore_senders,
    ) {
        tracing::info!("Filtered out");
        return StatusCode::OK;
    }

    let entity = payload.entity.unwrap();

    let message = ChannelMessage {
        id: entity.id,
        channel_id: entity.chat_id,
        sender_name,
        text: entity.plain_text,
        timestamp_ms: entity.created_at,
    };

    tracing::info!(
        channel_id = %message.channel_id,
        sender = %message.sender_name,
        "Channel message received"
    );

    let _ = state.tx.send(message).await;

    StatusCode::OK
}

// --- Filter ---

fn should_accept(
    chat_type: &str,
    plain_text: &str,
    sender_name: &str,
    ignore_senders: &[String],
) -> bool {
    if chat_type != "group" {
        return false;
    }

    let trimmed = plain_text.trim();
    if !trimmed.to_lowercase().starts_with("@rover") {
        return false;
    }

    if ignore_senders.iter().any(|s| s == sender_name) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_rover_mention_in_group() {
        assert!(should_accept("group", "@rover 안녕", "Alice", &[]));
    }

    #[test]
    fn accepts_rover_mention_case_insensitive() {
        assert!(should_accept("group", "@Rover do something", "Alice", &[]));
        assert!(should_accept("group", "@ROVER help", "Alice", &[]));
    }

    #[test]
    fn accepts_with_leading_whitespace() {
        assert!(should_accept("group", "  @rover hello", "Alice", &[]));
    }

    #[test]
    fn rejects_non_group_chat() {
        assert!(!should_accept("direct", "@rover hello", "Alice", &[]));
    }

    #[test]
    fn rejects_message_without_rover_mention() {
        assert!(!should_accept("group", "일반 메시지입니다", "Alice", &[]));
    }

    #[test]
    fn rejects_rover_not_at_start() {
        assert!(!should_accept("group", "hello @rover", "Alice", &[]));
    }

    #[test]
    fn rejects_ignored_sender() {
        let ignore = vec!["Bot".to_string()];
        assert!(!should_accept("group", "@rover hi", "Bot", &ignore));
    }

    #[test]
    fn accepts_non_ignored_sender() {
        let ignore = vec!["Bot".to_string()];
        assert!(should_accept("group", "@rover hi", "Alice", &ignore));
    }
}
