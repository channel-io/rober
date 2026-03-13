mod local_compat;

use std::fs;
use std::sync::OnceLock;

use rover_core::{
    BrowserRequest, FileRequest, NativeRequest, ProbeAdapter, ProbeError,
    ProbeResult, RenderMode,
};
use rover_zeroclaw_bridge::{StdProcessRunner, ZeroClawBridge};
use rover_windows_native::NativeAdapter;

use crate::local_compat::{LocalBrowserAdapter, LocalFileAdapter};

static ZEROCLAW_TOOL_SUPPORT: OnceLock<Option<bool>> = OnceLock::new();

pub struct AppOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait AppServices {
    fn doctor(&self) -> ProbeResult;
    fn browser(&self, request: BrowserRequest) -> Result<ProbeResult, ProbeError>;
    fn file(&self, request: FileRequest) -> Result<ProbeResult, ProbeError>;
    fn native(&self, request: NativeRequest) -> Result<ProbeResult, ProbeError>;
}

#[derive(Default)]
pub struct RealServices;

impl AppServices for RealServices {
    fn doctor(&self) -> ProbeResult {
        ZeroClawBridge::doctor_from_system(StdProcessRunner)
    }

    fn browser(&self, request: BrowserRequest) -> Result<ProbeResult, ProbeError> {
        if prefer_local_compat() {
            return LocalBrowserAdapter::default().run(request);
        }

        let primary_request = request.clone();
        let fallback_request = request;
        run_with_local_fallback(
            || {
                ZeroClawBridge::from_system(StdProcessRunner)
                    .and_then(|bridge| bridge.browser(primary_request))
            },
            || LocalBrowserAdapter::default().run(fallback_request),
        )
    }

    fn file(&self, request: FileRequest) -> Result<ProbeResult, ProbeError> {
        if prefer_local_compat() {
            return LocalFileAdapter.run(request);
        }

        let primary_request = request.clone();
        let fallback_request = request;
        run_with_local_fallback(
            || {
                ZeroClawBridge::from_system(StdProcessRunner)
                    .and_then(|bridge| bridge.file(primary_request))
            },
            || LocalFileAdapter.run(fallback_request),
        )
    }

    fn native(&self, request: NativeRequest) -> Result<ProbeResult, ProbeError> {
        NativeAdapter::default().run(request)
    }
}

pub fn run(args: &[String]) -> AppOutcome {
    run_with_services(args, &RealServices)
}

fn run_with_local_fallback<Primary, Fallback>(
    primary: Primary,
    fallback: Fallback,
) -> Result<ProbeResult, ProbeError>
where
    Primary: FnOnce() -> Result<ProbeResult, ProbeError>,
    Fallback: FnOnce() -> Result<ProbeResult, ProbeError>,
{
    match primary() {
        Ok(result) => Ok(result),
        Err(error) if should_fallback_to_local_compat(&error) => fallback(),
        Err(error) => Err(error),
    }
}

pub fn run_with_services(args: &[String], services: &dyn AppServices) -> AppOutcome {
    match parse_cli(args) {
        Ok(invocation) => {
            let mode = if invocation.json {
                RenderMode::Json
            } else {
                RenderMode::Human
            };

            let result = match invocation.command {
                Command::Doctor => Ok(services.doctor()),
                Command::Browser(request) => services.browser(request),
                Command::File(request) => services.file(request),
                Command::Native(request) => services.native(request),
            };

            match result {
                Ok(result) => AppOutcome {
                    exit_code: result.exit_code(),
                    stdout: result.render(mode),
                    stderr: String::new(),
                },
                Err(error) => AppOutcome {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: error.render(mode),
                },
            }
        }
        Err(error) => AppOutcome {
            exit_code: 2,
            stdout: String::new(),
            stderr: error.render(RenderMode::Human),
        },
    }
}

fn should_fallback_to_local_compat(error: &ProbeError) -> bool {
    error.code == "external_command_failed"
        && error
            .details
            .as_deref()
            .is_some_and(|details| details.contains("unrecognized subcommand 'tool'"))
}

fn prefer_local_compat() -> bool {
    matches!(
        *ZEROCLAW_TOOL_SUPPORT.get_or_init(|| {
            ZeroClawBridge::tool_subcommand_supported_from_system(StdProcessRunner).ok()
        }),
        Some(false)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Invocation {
    json: bool,
    command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Doctor,
    Browser(BrowserRequest),
    File(FileRequest),
    Native(NativeRequest),
}

fn parse_cli(args: &[String]) -> Result<Invocation, ProbeError> {
    let mut rest = args.to_vec();
    let json = remove_flag(&mut rest, "--json");

    let Some(command) = rest.first().map(String::as_str) else {
        return Err(usage_error("missing subcommand"));
    };

    let command = match command {
        "doctor" => {
            if rest.len() != 1 {
                return Err(usage_error("doctor does not take extra arguments"));
            }
            Command::Doctor
        }
        "browser" => Command::Browser(parse_browser(&rest[1..])?),
        "file" => Command::File(parse_file(&rest[1..])?),
        "native" => Command::Native(parse_native(&rest[1..])?),
        other => {
            return Err(usage_error(&format!(
                "unknown subcommand `{other}`; expected doctor, browser, file, or native"
            )));
        }
    };

    Ok(Invocation { json, command })
}

fn parse_browser(args: &[String]) -> Result<BrowserRequest, ProbeError> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(usage_error("missing browser action"));
    };

    match action {
        "open" => Ok(BrowserRequest::Open {
            url: required_option(args, "--url")?,
        }),
        "read" => Ok(BrowserRequest::Read {
            target: optional_option(args, "--target"),
        }),
        "click" => Ok(BrowserRequest::Click {
            target: required_option(args, "--target")?,
        }),
        "fill" => Ok(BrowserRequest::Fill {
            target: required_option(args, "--target")?,
            value: browser_fill_value(args)?,
        }),
        "download" => Ok(BrowserRequest::Download {
            url: required_option(args, "--url")?,
            destination: optional_option(args, "--destination"),
        }),
        other => Err(usage_error(&format!(
            "unknown browser action `{other}`; expected open, read, click, fill, or download"
        ))),
    }
}

fn parse_file(args: &[String]) -> Result<FileRequest, ProbeError> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(usage_error("missing file action"));
    };

    match action {
        "list" => Ok(FileRequest::List {
            path: required_option(args, "--path")?,
        }),
        "stat" => Ok(FileRequest::Stat {
            path: required_option(args, "--path")?,
        }),
        "copy" => Ok(FileRequest::Copy {
            source: required_option(args, "--source")?,
            destination: required_option(args, "--destination")?,
        }),
        "move" => Ok(FileRequest::Move {
            source: required_option(args, "--source")?,
            destination: required_option(args, "--destination")?,
        }),
        "delete" => Ok(FileRequest::Delete {
            path: required_option(args, "--path")?,
        }),
        "open" => Ok(FileRequest::Open {
            path: required_option(args, "--path")?,
        }),
        other => Err(usage_error(&format!(
            "unknown file action `{other}`; expected list, stat, copy, move, delete, or open"
        ))),
    }
}

fn parse_native(args: &[String]) -> Result<NativeRequest, ProbeError> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err(usage_error("missing native action"));
    };

    match action {
        "inspect" => Ok(NativeRequest::Inspect {
            target: optional_option(args, "--target"),
        }),
        "act" => Ok(NativeRequest::Act {
            action: required_option(args, "--action")?,
            target: optional_option(args, "--target"),
            value: optional_option(args, "--value"),
        }),
        other => Err(usage_error(&format!(
            "unknown native action `{other}`; expected inspect or act"
        ))),
    }
}

fn remove_flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        args.remove(index);
        true
    } else {
        false
    }
}

fn required_option(args: &[String], key: &str) -> Result<String, ProbeError> {
    optional_option(args, key).ok_or_else(|| usage_error(&format!("missing required option `{key}`")))
}

fn optional_option(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn usage_error(message: &str) -> ProbeError {
    ProbeError::usage(
        message,
        "usage: rover-probe [--json] <doctor|browser|file|native|messenger> ...",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        env,
        time::{SystemTime, UNIX_EPOCH},
    };
    use rover_core::{OutputValue, Status};

    struct FakeServices;

    impl AppServices for FakeServices {
        fn doctor(&self) -> ProbeResult {
            ProbeResult::success("doctor", "check", 1, "doctor ok")
        }

        fn browser(&self, request: BrowserRequest) -> Result<ProbeResult, ProbeError> {
            Ok(ProbeResult::with_output(
                "browser",
                request.action_name(),
                Status::Success,
                5,
                "browser dispatched",
                OutputValue::object(vec![("kind", OutputValue::string("browser"))]),
            ))
        }

        fn file(&self, request: FileRequest) -> Result<ProbeResult, ProbeError> {
            Ok(ProbeResult::success("file", request.action_name(), 5, "file dispatched"))
        }

        fn native(&self, request: NativeRequest) -> Result<ProbeResult, ProbeError> {
            Ok(ProbeResult::not_implemented(
                "native",
                request.action_name(),
                "native not implemented",
            ))
        }

    }

    #[test]
    fn parses_doctor_command() {
        let parsed = parse_cli(&["doctor".into()]).unwrap();
        assert_eq!(parsed.command, Command::Doctor);
        assert!(!parsed.json);
    }

    #[test]
    fn parses_browser_fill_with_json_flag() {
        let parsed = parse_cli(&[
            "--json".into(),
            "browser".into(),
            "fill".into(),
            "--target".into(),
            "#email".into(),
            "--value".into(),
            "hello".into(),
        ])
        .unwrap();

        assert!(parsed.json);
        assert_eq!(
            parsed.command,
            Command::Browser(BrowserRequest::Fill {
                target: "#email".into(),
                value: "hello".into(),
            })
        );
    }

    #[test]
    fn parses_browser_fill_from_value_file() {
        let payload_path = unique_temp_file("fill-value");
        fs::write(&payload_path, "payload from file").unwrap();

        let parsed = parse_cli(&[
            "browser".into(),
            "fill".into(),
            "--target".into(),
            "#notes".into(),
            "--value-file".into(),
            payload_path.display().to_string(),
        ])
        .unwrap();

        assert_eq!(
            parsed.command,
            Command::Browser(BrowserRequest::Fill {
                target: "#notes".into(),
                value: "payload from file".into(),
            })
        );

        fs::remove_file(payload_path).unwrap();
    }

    #[test]
    fn dispatches_browser_command() {
        let outcome = run_with_services(
            &[
                "browser".into(),
                "open".into(),
                "--url".into(),
                "https://example.com".into(),
            ],
            &FakeServices,
        );

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("browser"));
    }

    #[test]
    fn renders_native_stub_in_json_mode() {
        let outcome = run_with_services(
            &[
                "--json".into(),
                "native".into(),
                "inspect".into(),
                "--target".into(),
                "Notepad".into(),
            ],
            &FakeServices,
        );

        assert_eq!(outcome.exit_code, 2);
        assert!(outcome.stdout.contains("\"status\":\"not_implemented\""));
        assert!(outcome.stderr.is_empty());
    }

    #[test]
    fn falls_back_to_local_adapter_only_for_incompatible_zeroclaw_errors() {
        let fallback_called = Cell::new(false);

        let result = run_with_local_fallback(
            || {
                Err(
                    ProbeError::new(
                        "external_command_failed",
                        "zeroclaw command shape was rejected",
                    )
                    .with_details("stderr: error: unrecognized subcommand 'tool'"),
                )
            },
            || {
                fallback_called.set(true);
                Ok(ProbeResult::success(
                    "local-browser",
                    "open",
                    1,
                    "compat adapter handled request",
                ))
            },
        )
        .unwrap();

        assert!(fallback_called.get());
        assert_eq!(result.adapter, "local-browser");
    }

    #[test]
    fn preserves_primary_error_when_fallback_condition_is_not_met() {
        let fallback_called = Cell::new(false);

        let error = run_with_local_fallback(
            || {
                Err(
                    ProbeError::new(
                        "process_spawn_failed",
                        "zeroclaw process could not be started",
                    )
                    .with_details("The system cannot find the file specified."),
                )
            },
            || {
                fallback_called.set(true);
                Ok(ProbeResult::success(
                    "local-file",
                    "stat",
                    1,
                    "compat adapter handled request",
                ))
            },
        )
        .unwrap_err();

        assert!(!fallback_called.get());
        assert_eq!(error.code, "process_spawn_failed");
    }

    fn unique_temp_file(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "rover-probe-{prefix}-{}-{}.txt",
            std::process::id(),
            suffix
        ))
    }
}

fn browser_fill_value(args: &[String]) -> Result<String, ProbeError> {
    let inline_value = optional_option(args, "--value");
    let value_file = optional_option(args, "--value-file");

    match (inline_value, value_file) {
        (Some(_), Some(_)) => Err(usage_error(
            "browser fill accepts either `--value` or `--value-file`, but not both",
        )),
        (Some(value), None) => Ok(value),
        (None, Some(path)) => fs::read_to_string(&path).map_err(|error| {
            usage_error(&format!("failed to read browser fill payload from `{path}`"))
                .with_details(error.to_string())
        }),
        (None, None) => Err(usage_error(
            "missing required option `--value` or `--value-file`",
        )),
    }
}
