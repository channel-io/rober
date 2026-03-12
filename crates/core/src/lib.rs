use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Success,
    Error,
    NotImplemented,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Success => "success",
            Status::Error => "error",
            Status::NotImplemented => "not_implemented",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceItem {
    pub kind: String,
    pub value: String,
}

impl EvidenceItem {
    pub fn new(kind: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputValue {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<OutputValue>),
    Object(BTreeMap<String, OutputValue>),
}

impl OutputValue {
    pub fn string(value: impl Into<String>) -> Self {
        OutputValue::String(value.into())
    }

    pub fn object(entries: Vec<(&str, OutputValue)>) -> Self {
        let mut map = BTreeMap::new();
        for (key, value) in entries {
            map.insert(key.to_string(), value);
        }
        OutputValue::Object(map)
    }

    pub fn to_json(&self) -> String {
        match self {
            OutputValue::Null => "null".to_string(),
            OutputValue::Bool(value) => value.to_string(),
            OutputValue::Number(value) => value.to_string(),
            OutputValue::String(value) => format!("\"{}\"", escape_json(value)),
            OutputValue::Array(values) => {
                let rendered = values.iter().map(OutputValue::to_json).collect::<Vec<_>>();
                format!("[{}]", rendered.join(","))
            }
            OutputValue::Object(entries) => {
                let rendered = entries
                    .iter()
                    .map(|(key, value)| format!("\"{}\":{}", escape_json(key), value.to_json()))
                    .collect::<Vec<_>>();
                format!("{{{}}}", rendered.join(","))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub adapter: String,
    pub action: String,
    pub status: Status,
    pub latency_ms: u128,
    pub summary: String,
    pub structured_output: OutputValue,
    pub evidence: Vec<EvidenceItem>,
}

impl ProbeResult {
    pub fn success(
        adapter: impl Into<String>,
        action: impl Into<String>,
        latency_ms: u128,
        summary: impl Into<String>,
    ) -> Self {
        Self::with_output(
            adapter,
            action,
            Status::Success,
            latency_ms,
            summary,
            OutputValue::Null,
        )
    }

    pub fn not_implemented(
        adapter: impl Into<String>,
        action: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self::with_output(
            adapter,
            action,
            Status::NotImplemented,
            0,
            summary,
            OutputValue::Null,
        )
    }

    pub fn with_output(
        adapter: impl Into<String>,
        action: impl Into<String>,
        status: Status,
        latency_ms: u128,
        summary: impl Into<String>,
        structured_output: OutputValue,
    ) -> Self {
        Self {
            adapter: adapter.into(),
            action: action.into(),
            status,
            latency_ms,
            summary: summary.into(),
            structured_output,
            evidence: Vec::new(),
        }
    }

    pub fn with_evidence(mut self, evidence: Vec<EvidenceItem>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn exit_code(&self) -> i32 {
        match self.status {
            Status::Success => 0,
            Status::Error => 1,
            Status::NotImplemented => 2,
        }
    }

    pub fn render(&self, mode: RenderMode) -> String {
        match mode {
            RenderMode::Human => self.render_human(),
            RenderMode::Json => self.render_json(),
        }
    }

    fn render_human(&self) -> String {
        let mut lines = vec![
            format!("adapter: {}", self.adapter),
            format!("action: {}", self.action),
            format!("status: {}", self.status.as_str()),
            format!("latency_ms: {}", self.latency_ms),
            format!("summary: {}", self.summary),
        ];

        if self.structured_output != OutputValue::Null {
            lines.push(format!("structured_output: {}", self.structured_output.to_json()));
        }

        if !self.evidence.is_empty() {
            let evidence = self
                .evidence
                .iter()
                .map(|item| format!("{}={}", item.kind, item.value))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("evidence: {}", evidence));
        }

        lines.join("\n")
    }

    fn render_json(&self) -> String {
        let evidence = OutputValue::Array(
            self.evidence
                .iter()
                .map(|item| {
                    OutputValue::object(vec![
                        ("kind", OutputValue::string(item.kind.clone())),
                        ("value", OutputValue::string(item.value.clone())),
                    ])
                })
                .collect(),
        );

        OutputValue::object(vec![
            ("adapter", OutputValue::string(self.adapter.clone())),
            ("action", OutputValue::string(self.action.clone())),
            ("status", OutputValue::string(self.status.as_str())),
            ("latency_ms", OutputValue::Number(self.latency_ms as i64)),
            ("summary", OutputValue::string(self.summary.clone())),
            ("structured_output", self.structured_output.clone()),
            ("evidence", evidence),
        ])
        .to_json()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

impl ProbeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn usage(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self::new("usage_error", message).with_details(details)
    }

    pub fn render(&self, mode: RenderMode) -> String {
        match mode {
            RenderMode::Human => self.render_human(),
            RenderMode::Json => self.render_json(),
        }
    }

    fn render_human(&self) -> String {
        match &self.details {
            Some(details) => format!("error[{}]: {}\n{}", self.code, self.message, details),
            None => format!("error[{}]: {}", self.code, self.message),
        }
    }

    fn render_json(&self) -> String {
        OutputValue::object(vec![
            ("code", OutputValue::string(self.code.clone())),
            ("message", OutputValue::string(self.message.clone())),
            (
                "details",
                match &self.details {
                    Some(details) => OutputValue::string(details.clone()),
                    None => OutputValue::Null,
                },
            ),
        ])
        .to_json()
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render_human())
    }
}

impl std::error::Error for ProbeError {}

pub trait ProbeAdapter<Request> {
    fn adapter_name(&self) -> &'static str;
    fn run(&self, request: Request) -> Result<ProbeResult, ProbeError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserRequest {
    Open { url: String },
    Read { target: Option<String> },
    Click { target: String },
    Fill { target: String, value: String },
    Download { url: String, destination: Option<String> },
}

impl BrowserRequest {
    pub fn action_name(&self) -> &'static str {
        match self {
            BrowserRequest::Open { .. } => "open",
            BrowserRequest::Read { .. } => "read",
            BrowserRequest::Click { .. } => "click",
            BrowserRequest::Fill { .. } => "fill",
            BrowserRequest::Download { .. } => "download",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRequest {
    List { path: String },
    Stat { path: String },
    Copy { source: String, destination: String },
    Move { source: String, destination: String },
    Delete { path: String },
    Open { path: String },
}

impl FileRequest {
    pub fn action_name(&self) -> &'static str {
        match self {
            FileRequest::List { .. } => "list",
            FileRequest::Stat { .. } => "stat",
            FileRequest::Copy { .. } => "copy",
            FileRequest::Move { .. } => "move",
            FileRequest::Delete { .. } => "delete",
            FileRequest::Open { .. } => "open",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRequest {
    Inspect { target: Option<String> },
    Act {
        action: String,
        target: Option<String>,
        value: Option<String>,
    },
}

impl NativeRequest {
    pub fn action_name(&self) -> &str {
        match self {
            NativeRequest::Inspect { .. } => "inspect",
            NativeRequest::Act { action, .. } => action.as_str(),
        }
    }
}

fn escape_json(input: &str) -> String {
    let mut output = String::new();
    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c.is_control() => output.push_str(&format!("\\u{:04x}", c as u32)),
            c => output.push(c),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_probe_result_to_json() {
        let result = ProbeResult::with_output(
            "browser",
            "open",
            Status::Success,
            12,
            "ok",
            OutputValue::object(vec![("url", OutputValue::string("https://example.com"))]),
        )
        .with_evidence(vec![EvidenceItem::new("stdout", "done")]);

        let rendered = result.render(RenderMode::Json);
        assert!(rendered.contains("\"adapter\":\"browser\""));
        assert!(rendered.contains("\"latency_ms\":12"));
        assert!(rendered.contains("\"evidence\""));
    }

    #[test]
    fn renders_probe_error_in_both_modes() {
        let error = ProbeError::new("binary_not_found", "zeroclaw not found")
            .with_details("set ZEROCLAW_BIN or add zeroclaw to PATH");

        assert!(error.render(RenderMode::Human).contains("binary_not_found"));
        assert!(error.render(RenderMode::Json).contains("\"code\":\"binary_not_found\""));
    }
}
