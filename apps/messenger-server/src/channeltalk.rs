use reqwest::Client;
use serde_json::json;

const CHANNELTALK_API_BASE: &str = "https://api.channel.io/open";

pub async fn send_message(
    client: &Client,
    access_token: &str,
    channel_id: &str,
    text: &str,
    reply_to: Option<&str>,
) -> Result<(), String> {
    let mut body = json!({
        "channelId": channel_id,
        "body": {
            "text": text
        }
    });

    if let Some(parent_id) = reply_to {
        body["parentMessageId"] = json!(parent_id);
    }

    let response = client
        .post(format!("{CHANNELTALK_API_BASE}/v5/user-chats/{channel_id}/messages"))
        .header("Authorization", format!("Bearer {access_token}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Channel Talk API error {status}: {body}"))
    }
}
