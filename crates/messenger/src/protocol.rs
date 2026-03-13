use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum WsFrame {
    MessageReceived(IncomingMessage),
    Ack { request_id: String },
    Error { request_id: String, reason: String },
    SendMessage { request_id: String, message: OutgoingMessage },
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncomingMessage {
    pub id: String,
    pub channel_id: String,
    pub sender_name: String,
    pub text: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub channel_id: String,
    pub text: String,
    pub reply_to: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_incoming_message_frame() {
        let frame = WsFrame::MessageReceived(IncomingMessage {
            id: "msg-1".into(),
            channel_id: "ch-100".into(),
            sender_name: "Alice".into(),
            text: "Hello".into(),
            timestamp_ms: 1700000000000,
        });

        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"MessageReceived\""));
        assert!(json.contains("\"sender_name\":\"Alice\""));

        let deserialized: WsFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, frame);
    }

    #[test]
    fn serializes_send_message_frame() {
        let frame = WsFrame::SendMessage {
            request_id: "req-1".into(),
            message: OutgoingMessage {
                channel_id: "ch-100".into(),
                text: "Hi there".into(),
                reply_to: Some("msg-1".into()),
            },
        };

        let json = serde_json::to_string(&frame).unwrap();
        let deserialized: WsFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, frame);
    }

    #[test]
    fn serializes_ack_frame() {
        let frame = WsFrame::Ack { request_id: "req-1".into() };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"type\":\"Ack\""));

        let deserialized: WsFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, frame);
    }

    #[test]
    fn serializes_ping_pong() {
        let ping_json = serde_json::to_string(&WsFrame::Ping).unwrap();
        let pong_json = serde_json::to_string(&WsFrame::Pong).unwrap();

        assert_eq!(serde_json::from_str::<WsFrame>(&ping_json).unwrap(), WsFrame::Ping);
        assert_eq!(serde_json::from_str::<WsFrame>(&pong_json).unwrap(), WsFrame::Pong);
    }

    #[test]
    fn serializes_error_frame() {
        let frame = WsFrame::Error {
            request_id: "req-2".into(),
            reason: "channel not found".into(),
        };

        let json = serde_json::to_string(&frame).unwrap();
        let deserialized: WsFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, frame);
    }
}
