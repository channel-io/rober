pub mod channeltalk;

#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub id: String,
    pub channel_id: String,
    pub sender_name: String,
    pub text: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub channel_id: String,
    pub text: String,
    pub reply_to: Option<String>,
}

#[derive(Debug)]
pub struct ChannelError(pub String);

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ChannelError {}

pub trait Channel: Send + Sync {
    fn name(&self) -> &'static str;
    fn send(
        &self,
        message: OutgoingMessage,
    ) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send;
}
