use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rover_core::{
    BrowserRequest, EvidenceItem, FileRequest, OutputValue, ProbeError, ProbeResult, Status,
};

pub struct ZeroClawBridge<R> {
    runner: R,
    binary_path: PathBuf,
    _config_dir: PathBuf,
}

impl<R: ProcessRunner> ZeroClawBridge<R> {
    pub fn from_system(runner: R) -> Result<Self, ProbeError> {
        let binary_path = discover_binary_from_system()?.ok_or_else(|| {
            ProbeError::new("binary_not_found", "zeroclaw binary was not found")
                .with_details("set ZEROCLAW_BIN or add zeroclaw to PATH")
        })?;

        Ok(Self {
            runner,
            binary_path,
            _config_dir: default_config_dir(),
        })
    }

    pub fn tool_subcommand_supported_from_system(runner: R) -> Result<bool, ProbeError> {
        Self::from_system(runner)?.supports_tool_subcommand()
    }

    pub fn browser(&self, request: BrowserRequest) -> Result<ProbeResult, ProbeError> {
        let action_name = request.action_name().to_string();
        let args = browser_args(&request);
        let started = Instant::now();
        let output = self.runner.run(&self.binary_path, &args)?;
        let latency_ms = started.elapsed().as_millis();

        map_command_output("zeroclaw-browser", &action_name, output, latency_ms)
    }

    pub fn file(&self, request: FileRequest) -> Result<ProbeResult, ProbeError> {
        let action_name = request.action_name().to_string();
        let args = file_args(&request);
        let started = Instant::now();
        let output = self.runner.run(&self.binary_path, &args)?;
        let latency_ms = started.elapsed().as_millis();

        map_command_output("zeroclaw-file", &action_name, output, latency_ms)
    }

    pub fn supports_tool_subcommand(&self) -> Result<bool, ProbeError> {
        let output = self
            .runner
            .run(&self.binary_path, &["tool".into(), "--help".into()])?;

        if output.exit_code == 0 {
            return Ok(true);
        }

        if contains_unrecognized_tool_subcommand(&output.stdout)
            || contains_unrecognized_tool_subcommand(&output.stderr)
        {
            return Ok(false);
        }

        Err(
            ProbeError::new(
                "tool_subcommand_check_failed",
                "failed to determine zeroclaw CLI compatibility",
            )
            .with_details(format!(
                "exit_code={} stdout={} stderr={}",
                output.exit_code, output.stdout, output.stderr
            )),
        )
    }
}

impl ZeroClawBridge<StdProcessRunner> {
    pub fn doctor_from_system(runner: StdProcessRunner) -> ProbeResult {
        let config_dir = default_config_dir();
        let config_exists = config_dir.exists();

        let Some(binary_path) = discover_binary_from_system().ok().flatten() else {
            return ProbeResult::with_output(
                "doctor",
                "check",
                Status::Error,
                0,
                "zeroclaw binary not found",
                OutputValue::object(vec![
                    ("binary_found", OutputValue::Bool(false)),
                    ("config_dir", OutputValue::string(config_dir.display().to_string())),
                    ("config_exists", OutputValue::Bool(config_exists)),
                    ("windows_native_available", OutputValue::Bool(cfg!(windows))),
                ]),
            );
        };

        let version_output = runner.run(&binary_path, &["--version".into()]);

        match version_output {
            Ok(output) if output.exit_code == 0 => {
                let version = parse_version(&output.stdout, &output.stderr);
                ProbeResult::with_output(
                    "doctor",
                    "check",
                    Status::Success,
                    0,
                    "zeroclaw binary and config detected",
                    OutputValue::object(vec![
                        ("binary_found", OutputValue::Bool(true)),
                        (
                            "binary_path",
                            OutputValue::string(binary_path.display().to_string()),
                        ),
                        ("version", OutputValue::string(version)),
                        ("config_dir", OutputValue::string(config_dir.display().to_string())),
                        ("config_exists", OutputValue::Bool(config_exists)),
                        ("windows_native_available", OutputValue::Bool(cfg!(windows))),
                    ]),
                )
            }
            Ok(output) => ProbeResult::with_output(
                "doctor",
                "check",
                Status::Error,
                0,
                "zeroclaw binary found but version check failed",
                OutputValue::object(vec![
                    ("binary_found", OutputValue::Bool(true)),
                    (
                        "binary_path",
                        OutputValue::string(binary_path.display().to_string()),
                    ),
                    ("config_dir", OutputValue::string(config_dir.display().to_string())),
                    ("config_exists", OutputValue::Bool(config_exists)),
                    ("version_stdout", OutputValue::string(output.stdout)),
                    ("version_stderr", OutputValue::string(output.stderr)),
                ]),
            ),
            Err(error) => ProbeResult::with_output(
                "doctor",
                "check",
                Status::Error,
                0,
                "zeroclaw binary found but version command could not be executed",
                OutputValue::object(vec![
                    ("binary_found", OutputValue::Bool(true)),
                    (
                        "binary_path",
                        OutputValue::string(binary_path.display().to_string()),
                    ),
                    ("config_dir", OutputValue::string(config_dir.display().to_string())),
                    ("config_exists", OutputValue::Bool(config_exists)),
                    ("error", OutputValue::string(error.message)),
                ]),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait ProcessRunner {
    fn run(&self, binary: &Path, args: &[String]) -> Result<ProcessOutput, ProbeError>;
}

#[derive(Clone, Copy, Default)]
pub struct StdProcessRunner;

impl ProcessRunner for StdProcessRunner {
    fn run(&self, binary: &Path, args: &[String]) -> Result<ProcessOutput, ProbeError> {
        let output = Command::new(binary)
            .args(args)
            .output()
            .map_err(|error| {
                ProbeError::new("process_spawn_failed", "failed to spawn zeroclaw command")
                    .with_details(error.to_string())
            })?;

        Ok(ProcessOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn map_command_output(
    adapter: &str,
    action: &str,
    output: ProcessOutput,
    latency_ms: u128,
) -> Result<ProbeResult, ProbeError> {
    if output.exit_code != 0 {
        return Err(
            ProbeError::new(
                "external_command_failed",
                format!("{adapter} command exited with {}", output.exit_code),
            )
            .with_details(format!("stderr: {}", output.stderr)),
        );
    }

    let evidence = vec![
        EvidenceItem::new("stdout", output.stdout.clone()),
        EvidenceItem::new("stderr", output.stderr.clone()),
    ];

    Ok(
        ProbeResult::with_output(
            adapter,
            action,
            Status::Success,
            latency_ms,
            format!("{adapter} `{action}` dispatched through zeroclaw"),
            OutputValue::object(vec![
                ("exit_code", OutputValue::Number(output.exit_code as i64)),
                ("stdout", OutputValue::string(output.stdout)),
                ("stderr", OutputValue::string(output.stderr)),
            ]),
        )
        .with_evidence(evidence),
    )
}

fn contains_unrecognized_tool_subcommand(output: &str) -> bool {
    output.contains("unrecognized subcommand 'tool'")
        || output.contains("unrecognized subcommand `tool`")
}

// Assumption: these argument shapes centralize our best-effort zeroclaw CLI mapping.
// If zeroclaw's public CLI changes, only these builders need to be updated.
fn browser_args(request: &BrowserRequest) -> Vec<String> {
    match request {
        BrowserRequest::Open { url } => vec![
            "tool".into(),
            "browser".into(),
            "open".into(),
            "--url".into(),
            url.clone(),
        ],
        BrowserRequest::Read { target } => {
            let mut args = vec!["tool".into(), "browser".into(), "read".into()];
            if let Some(target) = target {
                args.push("--target".into());
                args.push(target.clone());
            }
            args
        }
        BrowserRequest::Click { target } => vec![
            "tool".into(),
            "browser".into(),
            "click".into(),
            "--target".into(),
            target.clone(),
        ],
        BrowserRequest::Fill { target, value } => vec![
            "tool".into(),
            "browser".into(),
            "fill".into(),
            "--target".into(),
            target.clone(),
            "--value".into(),
            value.clone(),
        ],
        BrowserRequest::Download { url, destination } => {
            let mut args = vec![
                "tool".into(),
                "browser".into(),
                "download".into(),
                "--url".into(),
                url.clone(),
            ];
            if let Some(destination) = destination {
                args.push("--destination".into());
                args.push(destination.clone());
            }
            args
        }
    }
}

fn file_args(request: &FileRequest) -> Vec<String> {
    match request {
        FileRequest::List { path } => vec![
            "tool".into(),
            "file".into(),
            "list".into(),
            "--path".into(),
            path.clone(),
        ],
        FileRequest::Stat { path } => vec![
            "tool".into(),
            "file".into(),
            "stat".into(),
            "--path".into(),
            path.clone(),
        ],
        FileRequest::Copy { source, destination } => vec![
            "tool".into(),
            "file".into(),
            "copy".into(),
            "--source".into(),
            source.clone(),
            "--destination".into(),
            destination.clone(),
        ],
        FileRequest::Move { source, destination } => vec![
            "tool".into(),
            "file".into(),
            "move".into(),
            "--source".into(),
            source.clone(),
            "--destination".into(),
            destination.clone(),
        ],
        FileRequest::Delete { path } => vec![
            "tool".into(),
            "file".into(),
            "delete".into(),
            "--path".into(),
            path.clone(),
        ],
        FileRequest::Open { path } => vec![
            "tool".into(),
            "file".into(),
            "open".into(),
            "--path".into(),
            path.clone(),
        ],
    }
}

fn parse_version(stdout: &str, stderr: &str) -> String {
    stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn default_config_dir() -> PathBuf {
    PathBuf::from("configs/zeroclaw")
}

fn discover_binary_from_system() -> Result<Option<PathBuf>, ProbeError> {
    let env_override = env::var_os("ZEROCLAW_BIN").map(PathBuf::from);
    let path_entries = env::var_os("PATH")
        .as_deref()
        .map(env::split_paths)
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();

    discover_binary(env_override, &path_entries)
}

fn discover_binary(
    env_override: Option<PathBuf>,
    path_entries: &[PathBuf],
) -> Result<Option<PathBuf>, ProbeError> {
    if let Some(path) = env_override {
        if path.exists() {
            return Ok(Some(path));
        }
        return Err(
            ProbeError::new("binary_not_found", "ZEROCLAW_BIN points to a missing path")
                .with_details(path.display().to_string()),
        );
    }

    let binary_names = if cfg!(windows) {
        vec!["zeroclaw.exe", "zeroclaw"]
    } else {
        vec!["zeroclaw"]
    };

    for entry in path_entries {
        for name in &binary_names {
            let candidate = entry.join(name);
            if candidate.exists() {
                return Ok(Some(candidate));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeRunner {
        outputs: RefCell<Vec<Result<ProcessOutput, ProbeError>>>,
        calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
    }

    impl FakeRunner {
        fn with_outputs(outputs: Vec<Result<ProcessOutput, ProbeError>>) -> Self {
            Self {
                outputs: RefCell::new(outputs),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl ProcessRunner for FakeRunner {
        fn run(&self, binary: &Path, args: &[String]) -> Result<ProcessOutput, ProbeError> {
            self.calls
                .borrow_mut()
                .push((binary.to_path_buf(), args.to_vec()));
            self.outputs.borrow_mut().remove(0)
        }
    }

    #[test]
    fn env_override_wins_for_binary_discovery() {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fake_binary = env::temp_dir().join(format!(
            "zeroclaw-test-bin-{}-{}",
            std::process::id(),
            unique_suffix
        ));
        std::fs::write(&fake_binary, "stub").unwrap();

        let discovered = discover_binary(Some(fake_binary.clone()), &[]).unwrap();
        assert_eq!(discovered, Some(fake_binary.clone()));

        std::fs::remove_file(fake_binary).unwrap();
    }

    #[test]
    fn parses_version_from_stdout() {
        let version = parse_version("zeroclaw 0.4.2\n", "");
        assert_eq!(version, "zeroclaw 0.4.2");
    }

    #[test]
    fn maps_stdout_and_stderr_into_probe_result() {
        let runner = FakeRunner::with_outputs(vec![Ok(ProcessOutput {
            exit_code: 0,
            stdout: "done".into(),
            stderr: String::new(),
        })]);

        let bridge = ZeroClawBridge {
            runner,
            binary_path: PathBuf::from("/usr/bin/zeroclaw"),
            _config_dir: PathBuf::from("configs/zeroclaw"),
        };

        let result = bridge
            .browser(BrowserRequest::Open {
                url: "https://example.com".into(),
            })
            .unwrap();

        assert_eq!(result.adapter, "zeroclaw-browser");
        assert!(result.summary.contains("dispatched"));
    }

    #[test]
    fn reports_missing_tool_subcommand_as_incompatible() {
        let runner = FakeRunner::with_outputs(vec![Ok(ProcessOutput {
            exit_code: 2,
            stdout: String::new(),
            stderr: "error: unrecognized subcommand 'tool'".into(),
        })]);

        let bridge = ZeroClawBridge {
            runner,
            binary_path: PathBuf::from("/usr/bin/zeroclaw"),
            _config_dir: PathBuf::from("configs/zeroclaw"),
        };

        assert!(!bridge.supports_tool_subcommand().unwrap());
    }
}
