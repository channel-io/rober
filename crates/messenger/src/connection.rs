use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tracing;

use crate::protocol::{IncomingMessage, WsFrame};

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

pub struct WsConnection {
    write: Arc<Mutex<WsSink>>,
    incoming_rx: mpsc::Receiver<IncomingMessage>,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl WsConnection {
    pub async fn connect(url: &str) -> Result<Self, ConnectionError> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| ConnectionError::Connect(e.to_string()))?;

        let (write, read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));
        let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingMessage>(256);

        let writer_for_pong = Arc::clone(&write);
        let reader_handle = tokio::spawn(async move {
            Self::reader_loop(read, incoming_tx, writer_for_pong).await;
        });

        Ok(Self {
            write,
            incoming_rx,
            _reader_handle: reader_handle,
        })
    }

    pub async fn connect_with_retry(url: &str, max_attempts: u32) -> Result<Self, ConnectionError> {
        let mut delay = Duration::from_millis(100);
        for attempt in 1..=max_attempts {
            match Self::connect(url).await {
                Ok(conn) => return Ok(conn),
                Err(e) if attempt == max_attempts => return Err(e),
                Err(_) => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(10));
                }
            }
        }
        Err(ConnectionError::Connect("max attempts exceeded".into()))
    }

    pub async fn send_frame(&self, frame: &WsFrame) -> Result<(), ConnectionError> {
        let json = serde_json::to_string(frame)
            .map_err(|e| ConnectionError::Serialize(e.to_string()))?;
        let mut write = self.write.lock().await;
        write
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| ConnectionError::Send(e.to_string()))
    }

    pub async fn recv_incoming(&mut self) -> Option<IncomingMessage> {
        self.incoming_rx.recv().await
    }

    pub fn try_recv_incoming(&mut self) -> Option<IncomingMessage> {
        self.incoming_rx.try_recv().ok()
    }

    async fn reader_loop(
        mut read: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        incoming_tx: mpsc::Sender<IncomingMessage>,
        write: Arc<Mutex<WsSink>>,
    ) {
        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("WebSocket read error: {e}");
                    break;
                }
            };

            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };

            let frame: WsFrame = match serde_json::from_str(&text) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to parse WsFrame: {e}");
                    continue;
                }
            };

            match frame {
                WsFrame::MessageReceived(msg) => {
                    let _ = incoming_tx.send(msg).await;
                }
                WsFrame::Ping => {
                    let pong = serde_json::to_string(&WsFrame::Pong).unwrap();
                    let mut w = write.lock().await;
                    let _ = w.send(Message::Text(pong.into())).await;
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug)]
pub enum ConnectionError {
    Connect(String),
    Serialize(String),
    Send(String),
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionError::Connect(e) => write!(f, "connection failed: {e}"),
            ConnectionError::Serialize(e) => write!(f, "serialization failed: {e}"),
            ConnectionError::Send(e) => write!(f, "send failed: {e}"),
        }
    }
}

impl std::error::Error for ConnectionError {}
