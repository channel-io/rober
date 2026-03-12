use rover_core::{
    BrowserRequest, FileRequest, NativeRequest, ProbeAdapter, ProbeError, ProbeResult, RenderMode,
};
use rover_zeroclaw_bridge::{StdProcessRunner, ZeroClawBridge};
use rover_windows_native::NativeAdapter;

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
        ZeroClawBridge::from_system(StdProcessRunner)?.browser(request)
    }

    fn file(&self, request: FileRequest) -> Result<ProbeResult, ProbeError> {
        ZeroClawBridge::from_system(StdProcessRunner)?.file(request)
    }

    fn native(&self, request: NativeRequest) -> Result<ProbeResult, ProbeError> {
        NativeAdapter::default().run(request)
    }
}

pub fn run(args: &[String]) -> AppOutcome {
    run_with_services(args, &RealServices)
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
            value: required_option(args, "--value")?,
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
        "usage: rover-probe [--json] <doctor|browser|file|native> ...",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
