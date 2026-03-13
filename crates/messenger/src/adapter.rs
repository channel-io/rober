use std::sync::Mutex;
use std::time::Instant;

use rover_core::{
    MessengerRequest, OutputValue, ProbeAdapter, ProbeError, ProbeResult, Status,
};

use crate::connection::WsConnection;
use crate::protocol::{OutgoingMessage, WsFrame};

pub struct MessengerAdapter {
    ws_url: String,
    runtime: tokio::runtime::Runtime,
    connection: Mutex<Option<WsConnection>>,
}

impl MessengerAdapter {
    pub fn new(ws_url: String) -> Result<Self, ProbeError> {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            ProbeError::new("runtime_error", format!("failed to create tokio runtime: {e}"))
        })?;

        Ok(Self {
            ws_url,
            runtime,
            connection: Mutex::new(None),
        })
    }

    fn ensure_connected(&self) -> Result<(), ProbeError> {
        let mut conn_guard = self.connection.lock().unwrap();
        if conn_guard.is_none() {
            let conn = self.runtime.block_on(WsConnection::connect_with_retry(&self.ws_url, 3))
                .map_err(|e| ProbeError::new("connection_error", e.to_string()))?;
            *conn_guard = Some(conn);
        }
        Ok(())
    }

    fn execute(&self, request: MessengerRequest) -> Result<ProbeResult, ProbeError> {
        self.ensure_connected()?;
        let start = Instant::now();

        let result = match &request {
            MessengerRequest::Send { channel_id, message } => {
                self.send_message(channel_id, message, None)?
            }
            MessengerRequest::Reply { channel_id, parent_message_id, message } => {
                self.send_message(channel_id, message, Some(parent_message_id))?
            }
            MessengerRequest::Read { channel_id, limit } => {
                self.read_messages(channel_id, *limit)?
            }
        };

        let latency_ms = start.elapsed().as_millis();
        Ok(ProbeResult::with_output(
            "messenger",
            request.action_name(),
            Status::Success,
            latency_ms,
            result.0,
            result.1,
        ))
    }

    fn send_message(
        &self,
        channel_id: &str,
        text: &str,
        reply_to: Option<&String>,
    ) -> Result<(String, OutputValue), ProbeError> {
        let request_id = format!("req-{}", uuid_v4_simple());

        let frame = WsFrame::SendMessage {
            request_id: request_id.clone(),
            message: OutgoingMessage {
                channel_id: channel_id.to_string(),
                text: text.to_string(),
                reply_to: reply_to.cloned(),
            },
        };

        let conn_guard = self.connection.lock().unwrap();
        let conn = conn_guard.as_ref().unwrap();
        self.runtime
            .block_on(conn.send_frame(&frame))
            .map_err(|e| ProbeError::new("send_error", e.to_string()))?;

        Ok((
            format!("message sent to channel {channel_id}"),
            OutputValue::object(vec![
                ("channel_id", OutputValue::string(channel_id)),
                ("request_id", OutputValue::string(&request_id)),
            ]),
        ))
    }

    fn read_messages(
        &self,
        channel_id: &str,
        limit: Option<u32>,
    ) -> Result<(String, OutputValue), ProbeError> {
        let limit = limit.unwrap_or(10) as usize;
        let mut conn_guard = self.connection.lock().unwrap();
        let conn = conn_guard.as_mut().unwrap();

        let mut messages = Vec::new();
        while messages.len() < limit {
            match conn.try_recv_incoming() {
                Some(msg) if msg.channel_id == channel_id => {
                    messages.push(OutputValue::object(vec![
                        ("id", OutputValue::string(&msg.id)),
                        ("sender_name", OutputValue::string(&msg.sender_name)),
                        ("text", OutputValue::string(&msg.text)),
                        ("timestamp_ms", OutputValue::Number(msg.timestamp_ms as i64)),
                    ]));
                }
                Some(_) => continue,
                None => break,
            }
        }

        let count = messages.len();
        Ok((
            format!("read {count} messages from channel {channel_id}"),
            OutputValue::object(vec![
                ("channel_id", OutputValue::string(channel_id)),
                ("count", OutputValue::Number(count as i64)),
                ("messages", OutputValue::Array(messages)),
            ]),
        ))
    }
}

impl ProbeAdapter<MessengerRequest> for MessengerAdapter {
    fn adapter_name(&self) -> &'static str {
        "messenger"
    }

    fn run(&self, request: MessengerRequest) -> Result<ProbeResult, ProbeError> {
        self.execute(request)
    }
}

fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}-{:x}", nanos, std::process::id())
}
