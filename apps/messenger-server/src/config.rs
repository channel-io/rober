use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub channeltalk: ChannelTalkConfig,
    pub filter: FilterConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ChannelTalkConfig {
    pub access_token: String,
    pub webhook_secret: String,
}

#[derive(Debug, Deserialize)]
pub struct FilterConfig {
    pub channel_ids: Vec<String>,
    pub ignore_senders: Vec<String>,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError(format!("cannot read {path}: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| ConfigError(format!("invalid config: {e}")))
    }
}

#[derive(Debug)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConfigError {}
