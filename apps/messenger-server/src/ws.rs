use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use tokio::sync::{mpsc, Mutex};

use rover_messenger::protocol::{OutgoingMessage, WsFrame};

use crate::channeltalk;
use crate::AppState;

type WsSender = mpsc::Sender<String>;

pub struct ConnectionRegistry {
    connections: Mutex<Vec<WsSender>>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(Vec::new()),
        }
    }

    pub async fn add(&self, sender: WsSender) {
        self.connections.lock().await.push(sender);
    }

    pub async fn broadcast(&self, frame: &WsFrame) {
        let json = match serde_json::to_string(frame) {
            Ok(j) => j,
            Err(_) => return,
        };

        let mut conns = self.connections.lock().await;
        conns.retain(|sender| !sender.is_closed());
        for sender in conns.iter() {
            let _ = sender.try_send(json.clone());
        }
    }
}

pub async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(socket: WebSocket, state: Arc<AppState>) {
    use futures_util::SinkExt;
    use futures_util::StreamExt;

    let (ws_sink, mut ws_stream) = socket.split();
    let mut ws_sink: futures_util::stream::SplitSink<WebSocket, Message> = ws_sink;

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<String>(256);
    state.registry.add(outgoing_tx).await;
    tracing::info!("WS client connected");

    let send_task = tokio::spawn(async move {
        while let Some(msg) = outgoing_rx.recv().await {
            if ws_sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let state_clone = Arc::clone(&state);
    let recv_task = tokio::spawn(async move {
        while let Some(msg_result) = ws_stream.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(_) => break,
            };
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };

            let frame: WsFrame = match serde_json::from_str(&text) {
                Ok(f) => f,
                Err(_) => continue,
            };

            match frame {
                WsFrame::SendMessage { request_id, message } => {
                    tracing::info!(request_id = %request_id, channel_id = %message.channel_id, "Sending message via Channel Talk API");
                    handle_send_message(&state_clone, &request_id, &message).await;
                }
                WsFrame::Ping => {
                    // Registry will send pong back through broadcast if needed
                }
                _ => {}
            }
        }
    });

    let _ = tokio::join!(send_task, recv_task);
}

async fn handle_send_message(state: &AppState, request_id: &str, message: &OutgoingMessage) {
    match channeltalk::send_message(
        &state.http_client,
        &state.config.channeltalk.access_token,
        &message.channel_id,
        &message.text,
        message.reply_to.as_deref(),
    )
    .await
    {
        Ok(()) => {
            let ack = WsFrame::Ack {
                request_id: request_id.to_string(),
            };
            state.registry.broadcast(&ack).await;
        }
        Err(reason) => {
            let error = WsFrame::Error {
                request_id: request_id.to_string(),
                reason,
            };
            state.registry.broadcast(&error).await;
        }
    }
}
