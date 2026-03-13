use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;

const CHANNELTALK_API_BASE: &str = "https://api.channel.io/open";

/// Channel Talk channel in webhook mode.
///
/// Incoming messages are received by the gateway endpoint `/channeltalk`.
/// Outbound replies are sent through Channel Talk REST API.
pub struct ChannelTalkChannel {
    access_key: String,
    access_secret: String,
    ignore_senders: Vec<String>,
    client: reqwest::Client,
}

impl ChannelTalkChannel {
    pub fn new(access_key: String, access_secret: String, ignore_senders: Vec<String>) -> Self {
        Self {
            access_key,
            access_secret,
            ignore_senders,
            client: reqwest::Client::new(),
        }
    }

    fn is_sender_ignored(&self, sender: &str) -> bool {
        self.ignore_senders.iter().any(|s| s == sender)
    }

    fn now_unix_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn parse_timestamp(value: Option<&serde_json::Value>) -> u64 {
        let raw = match value {
            Some(serde_json::Value::Number(num)) => num.as_u64(),
            Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
        .unwrap_or_else(Self::now_unix_secs);

        // Channel Talk uses milliseconds.
        if raw > 1_000_000_000_000 {
            raw / 1000
        } else {
            raw
        }
    }

    /// Parse a Channel Talk webhook payload into channel messages.
    ///
    /// Filters:
    /// - `type` must be `"message"`
    /// - `entity.chatType` must be `"group"`
    /// - `entity.plainText` must start with `@rover` (case-insensitive)
    /// - sender must not be in `ignore_senders`
    pub fn parse_webhook_payload(&self, payload: &serde_json::Value) -> Vec<ChannelMessage> {
        let mut messages = Vec::new();

        let msg_type = payload
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if msg_type != "message" {
            tracing::debug!("Channel Talk: skipping non-message type: {msg_type}");
            return messages;
        }

        let Some(entity) = payload.get("entity") else {
            return messages;
        };

        let chat_type = entity
            .get("chatType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if chat_type != "group" {
            tracing::debug!("Channel Talk: skipping non-group chat: {chat_type}");
            return messages;
        }

        let plain_text = entity
            .get("plainText")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let trimmed = plain_text.trim();
        if !trimmed.to_lowercase().starts_with("@rover") {
            tracing::debug!("Channel Talk: message does not start with @rover");
            return messages;
        }

        // Extract sender name from refers.manager.name, fallback to entity.personId
        let sender_name = payload
            .get("refers")
            .and_then(|r| r.get("manager"))
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                entity
                    .get("personId")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("unknown");

        if self.is_sender_ignored(sender_name) {
            tracing::debug!("Channel Talk: ignoring sender: {sender_name}");
            return messages;
        }

        let message_id = entity
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let chat_id = entity
            .get("chatId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let timestamp = Self::parse_timestamp(entity.get("createdAt"));

        messages.push(ChannelMessage {
            id: message_id,
            reply_target: chat_id,
            sender: sender_name.to_string(),
            content: plain_text.to_string(),
            channel: "channeltalk".to_string(),
            timestamp,
            thread_ts: None,
        });

        messages
    }

    async fn send_to_chat(&self, chat_id: &str, content: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "channelId": chat_id,
            "body": {
                "text": content
            }
        });

        let response = self
            .client
            .post(format!(
                "{CHANNELTALK_API_BASE}/v5/user-chats/{chat_id}/messages"
            ))
            .header("x-access-key", &self.access_key)
            .header("x-access-secret", &self.access_secret)
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        tracing::error!("Channel Talk send failed: {status} — {body}");
        anyhow::bail!("Channel Talk API error: {status}");
    }
}

#[async_trait]
impl Channel for ChannelTalkChannel {
    fn name(&self) -> &str {
        "channeltalk"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        self.send_to_chat(&message.recipient, &message.content)
            .await
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!(
            "Channel Talk channel active (webhook mode). \
            Configure Channel Talk webhook to POST to your gateway's /channeltalk endpoint."
        );

        // Keep task alive; incoming events are handled by the gateway webhook handler.
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> ChannelTalkChannel {
        ChannelTalkChannel::new("test-key".into(), "test-secret".into(), vec![])
    }

    fn make_channel_with_ignored(senders: Vec<&str>) -> ChannelTalkChannel {
        ChannelTalkChannel::new(
            "test-key".into(),
            "test-secret".into(),
            senders.into_iter().map(String::from).collect(),
        )
    }

    fn make_payload(chat_type: &str, plain_text: &str, sender_name: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "message",
            "event": "message.created",
            "entity": {
                "id": "msg-1",
                "chatType": chat_type,
                "chatId": "chat-100",
                "personId": "person-1",
                "plainText": plain_text,
                "createdAt": 1700000000000_u64
            },
            "refers": {
                "manager": {
                    "name": sender_name
                }
            }
        })
    }

    #[test]
    fn channel_name() {
        assert_eq!(make_channel().name(), "channeltalk");
    }

    #[test]
    fn accepts_rover_mention_in_group() {
        let channel = make_channel();
        let payload = make_payload("group", "@rover 안녕", "Alice");
        let messages = channel.parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender, "Alice");
        assert_eq!(messages[0].content, "@rover 안녕");
        assert_eq!(messages[0].reply_target, "chat-100");
        assert_eq!(messages[0].channel, "channeltalk");
        assert_eq!(messages[0].timestamp, 1_700_000_000);
    }

    #[test]
    fn accepts_rover_mention_case_insensitive() {
        let channel = make_channel();
        assert_eq!(
            channel
                .parse_webhook_payload(&make_payload("group", "@Rover do something", "Alice"))
                .len(),
            1
        );
        assert_eq!(
            channel
                .parse_webhook_payload(&make_payload("group", "@ROVER help", "Alice"))
                .len(),
            1
        );
    }

    #[test]
    fn accepts_with_leading_whitespace() {
        let channel = make_channel();
        let messages =
            channel.parse_webhook_payload(&make_payload("group", "  @rover hello", "Alice"));
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn rejects_non_group_chat() {
        let channel = make_channel();
        let messages =
            channel.parse_webhook_payload(&make_payload("direct", "@rover hello", "Alice"));
        assert!(messages.is_empty());
    }

    #[test]
    fn rejects_message_without_rover_mention() {
        let channel = make_channel();
        let messages =
            channel.parse_webhook_payload(&make_payload("group", "일반 메시지입니다", "Alice"));
        assert!(messages.is_empty());
    }

    #[test]
    fn rejects_rover_not_at_start() {
        let channel = make_channel();
        let messages =
            channel.parse_webhook_payload(&make_payload("group", "hello @rover", "Alice"));
        assert!(messages.is_empty());
    }

    #[test]
    fn rejects_ignored_sender() {
        let channel = make_channel_with_ignored(vec!["Bot"]);
        let messages = channel.parse_webhook_payload(&make_payload("group", "@rover hi", "Bot"));
        assert!(messages.is_empty());
    }

    #[test]
    fn accepts_non_ignored_sender() {
        let channel = make_channel_with_ignored(vec!["Bot"]);
        let messages =
            channel.parse_webhook_payload(&make_payload("group", "@rover hi", "Alice"));
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn rejects_non_message_type() {
        let channel = make_channel();
        let payload = serde_json::json!({
            "type": "user",
            "entity": {
                "chatType": "group",
                "plainText": "@rover hello"
            }
        });
        assert!(channel.parse_webhook_payload(&payload).is_empty());
    }

    #[test]
    fn falls_back_to_person_id_when_manager_name_empty() {
        let channel = make_channel();
        let payload = serde_json::json!({
            "type": "message",
            "entity": {
                "id": "msg-2",
                "chatType": "group",
                "chatId": "chat-200",
                "personId": "person-42",
                "plainText": "@rover test",
                "createdAt": 1700000000
            },
            "refers": {
                "manager": {
                    "name": ""
                }
            }
        });
        let messages = channel.parse_webhook_payload(&payload);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender, "person-42");
    }
}
