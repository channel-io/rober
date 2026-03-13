use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use rover_messenger::protocol::{IncomingMessage, WsFrame};

use crate::filter;
use crate::AppState;

type HmacSha256 = Hmac<Sha256>;

#[derive(serde::Deserialize)]
struct WebhookPayload {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    event: String,
    entity: Option<WebhookEntity>,
    refers: Option<WebhookRefers>,
}

#[derive(serde::Deserialize)]
struct WebhookEntity {
    #[serde(default)]
    id: String,
    #[serde(rename = "channelId", default)]
    _channel_id: String,
    #[serde(rename = "chatType", default)]
    chat_type: String,
    #[serde(rename = "chatId", default)]
    chat_id: String,
    #[serde(rename = "personType", default)]
    _person_type: String,
    #[serde(rename = "personId", default)]
    person_id: String,
    #[serde(rename = "plainText", default)]
    plain_text: String,
    #[serde(rename = "createdAt", default)]
    created_at: u64,
}

#[derive(serde::Deserialize)]
struct WebhookRefers {
    manager: Option<ManagerInfo>,
}

#[derive(serde::Deserialize)]
struct ManagerInfo {
    #[serde(default)]
    name: String,
}

impl WebhookPayload {
    fn sender_name(&self) -> String {
        if let Some(refers) = &self.refers {
            if let Some(manager) = &refers.manager {
                return manager.name.clone();
            }
        }

        self.entity
            .as_ref()
            .map(|e| e.person_id.clone())
            .unwrap_or_default()
    }
}

pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if !verify_signature(&state.config.channeltalk.webhook_secret, &headers, &body) {
        tracing::warn!("Webhook signature verification failed");
        return StatusCode::UNAUTHORIZED;
    }

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

    tracing::info!(
        chat_type = %entity.chat_type,
        plain_text = %truncate(&entity.plain_text, 10),
        sender = %sender_name,
        "Filter input"
    );

    if !filter::should_accept(&state.config.filter, &entity.chat_type, &entity.plain_text, &sender_name) {
        tracing::info!("Filtered out");
        return StatusCode::OK;
    }

    let entity = payload.entity.unwrap();

    let incoming = IncomingMessage {
        id: entity.id,
        channel_id: entity.chat_id,
        sender_name,
        text: entity.plain_text,
        timestamp_ms: entity.created_at,
    };

    tracing::info!(
        channel_id = %incoming.channel_id,
        sender = %incoming.sender_name,
        text_prefix = %truncate(&incoming.text, 30),
        "Broadcasting to WS clients"
    );

    let frame = WsFrame::MessageReceived(incoming);
    state.registry.broadcast(&frame).await;

    StatusCode::OK
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}...")
    }
}

fn verify_signature(secret: &str, headers: &HeaderMap, body: &Bytes) -> bool {
    if secret.is_empty() {
        return true;
    }

    let Some(sig_header) = headers.get("x-signature") else {
        return false;
    };

    let sig_str = match sig_header.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };

    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());

    constant_time_eq(sig_str.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
