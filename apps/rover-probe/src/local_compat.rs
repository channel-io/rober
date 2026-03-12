use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rover_core::{BrowserRequest, FileRequest, OutputValue, ProbeAdapter, ProbeError, ProbeResult, Status};

#[derive(Default)]
pub struct LocalFileAdapter;

impl ProbeAdapter<FileRequest> for LocalFileAdapter {
    fn adapter_name(&self) -> &'static str {
        "local-file"
    }

    fn run(&self, request: FileRequest) -> Result<ProbeResult, ProbeError> {
        let action = request.action_name().to_string();
        let started = Instant::now();

        let result = match request {
            FileRequest::List { path } => {
                let entries = fs::read_dir(&path)
                    .map_err(|error| io_error("list_failed", &path, error))?
                    .filter_map(Result::ok)
                    .map(|entry| OutputValue::string(entry.path().display().to_string()))
                    .collect::<Vec<_>>();

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "listed directory through local compatibility adapter",
                    OutputValue::object(vec![
                        ("path", OutputValue::string(path)),
                        ("entries", OutputValue::Array(entries)),
                    ]),
                )
            }
            FileRequest::Stat { path } => {
                let metadata =
                    fs::metadata(&path).map_err(|error| io_error("stat_failed", &path, error))?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "read file metadata through local compatibility adapter",
                    OutputValue::object(vec![
                        ("path", OutputValue::string(path)),
                        ("is_file", OutputValue::Bool(metadata.is_file())),
                        ("is_dir", OutputValue::Bool(metadata.is_dir())),
                        ("readonly", OutputValue::Bool(metadata.permissions().readonly())),
                        ("len", OutputValue::Number(saturating_i64(metadata.len()))),
                    ]),
                )
            }
            FileRequest::Copy {
                source,
                destination,
            } => {
                if let Some(parent) = Path::new(&destination).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .map_err(|error| io_error("copy_prepare_failed", &destination, error))?;
                    }
                }

                let bytes = fs::copy(&source, &destination)
                    .map_err(|error| io_error("copy_failed", &source, error))?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "copied file through local compatibility adapter",
                    OutputValue::object(vec![
                        ("source", OutputValue::string(source)),
                        ("destination", OutputValue::string(destination)),
                        ("bytes_copied", OutputValue::Number(saturating_i64(bytes))),
                    ]),
                )
            }
            FileRequest::Move {
                source,
                destination,
            } => {
                if let Some(parent) = Path::new(&destination).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .map_err(|error| io_error("move_prepare_failed", &destination, error))?;
                    }
                }

                fs::rename(&source, &destination)
                    .map_err(|error| io_error("move_failed", &source, error))?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "moved file through local compatibility adapter",
                    OutputValue::object(vec![
                        ("source", OutputValue::string(source)),
                        ("destination", OutputValue::string(destination)),
                    ]),
                )
            }
            FileRequest::Delete { path } => {
                let target = Path::new(&path);
                if target.is_dir() {
                    fs::remove_dir_all(target)
                        .map_err(|error| io_error("delete_failed", &path, error))?;
                } else {
                    fs::remove_file(target).map_err(|error| io_error("delete_failed", &path, error))?;
                }

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "deleted path through local compatibility adapter",
                    OutputValue::object(vec![("path", OutputValue::string(path))]),
                )
            }
            FileRequest::Open { path } => {
                let contents =
                    fs::read_to_string(&path).map_err(|error| io_error("open_failed", &path, error))?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "opened file through local compatibility adapter",
                    OutputValue::object(vec![
                        ("path", OutputValue::string(path)),
                        ("contents", OutputValue::string(contents)),
                    ]),
                )
            }
        };

        Ok(result)
    }
}

pub struct LocalBrowserAdapter {
    session_path: PathBuf,
}

impl Default for LocalBrowserAdapter {
    fn default() -> Self {
        Self {
            session_path: PathBuf::from("target")
                .join("rover-probe")
                .join("browser-session.state"),
        }
    }
}

impl LocalBrowserAdapter {
    #[cfg(test)]
    fn new(session_path: PathBuf) -> Self {
        Self { session_path }
    }
}

impl ProbeAdapter<BrowserRequest> for LocalBrowserAdapter {
    fn adapter_name(&self) -> &'static str {
        "local-browser"
    }

    fn run(&self, request: BrowserRequest) -> Result<ProbeResult, ProbeError> {
        let action = request.action_name().to_string();
        let started = Instant::now();

        let result = match request {
            BrowserRequest::Open { url } => {
                let path = file_url_to_path(&url)?;
                let html =
                    fs::read_to_string(&path).map_err(|error| io_error("open_failed", &url, error))?;
                let session = BrowserSession::from_fixture(&url, &html);
                session.save(&self.session_path)?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "opened page through local browser compatibility adapter",
                    OutputValue::object(vec![
                        ("url", OutputValue::string(url)),
                        ("title", OutputValue::string(session.title)),
                    ]),
                )
            }
            BrowserRequest::Read { target } => {
                let session = BrowserSession::load(&self.session_path)?;
                let selector = target.unwrap_or_else(|| "body".to_string());
                let text = session.read_target(&selector)?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "read page state through local browser compatibility adapter",
                    OutputValue::object(vec![
                        ("target", OutputValue::string(selector)),
                        ("text", OutputValue::string(text)),
                        ("url", OutputValue::string(session.url)),
                    ]),
                )
            }
            BrowserRequest::Fill { target, value } => {
                let mut session = BrowserSession::load(&self.session_path)?;
                session.fill(&target, &value)?;
                session.save(&self.session_path)?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "updated field through local browser compatibility adapter",
                    OutputValue::object(vec![
                        ("target", OutputValue::string(target)),
                        ("value", OutputValue::string(value)),
                    ]),
                )
            }
            BrowserRequest::Click { target } => {
                let mut session = BrowserSession::load(&self.session_path)?;
                session.click(&target)?;
                session.save(&self.session_path)?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "clicked element through local browser compatibility adapter",
                    OutputValue::object(vec![
                        ("target", OutputValue::string(target)),
                        ("result", OutputValue::string(session.result)),
                    ]),
                )
            }
            BrowserRequest::Download { url, destination } => {
                let source = file_url_to_path(&url)?;
                let destination = destination.unwrap_or_else(|| {
                    source
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "downloaded-file".to_string())
                });
                if let Some(parent) = Path::new(&destination).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).ok();
                    }
                }
                let bytes = fs::copy(&source, &destination)
                    .map_err(|error| io_error("download_failed", &url, error))?;

                ProbeResult::with_output(
                    self.adapter_name(),
                    action,
                    Status::Success,
                    started.elapsed().as_millis(),
                    "downloaded file through local browser compatibility adapter",
                    OutputValue::object(vec![
                        ("url", OutputValue::string(url)),
                        ("destination", OutputValue::string(destination)),
                        ("bytes_copied", OutputValue::Number(saturating_i64(bytes))),
                    ]),
                )
            }
        };

        Ok(result)
    }
}

#[derive(Debug, Clone, Default)]
struct BrowserSession {
    url: String,
    title: String,
    intro: String,
    name: String,
    notes: String,
    result: String,
}

impl BrowserSession {
    fn from_fixture(url: &str, html: &str) -> Self {
        Self {
            url: url.to_string(),
            title: extract_tag_text(html, "title").unwrap_or_else(|| "Untitled".to_string()),
            intro: extract_text_by_id(html, "intro").unwrap_or_default(),
            name: String::new(),
            notes: String::new(),
            result: extract_text_by_id(html, "result")
                .unwrap_or_else(|| "Waiting for input".to_string()),
        }
    }

    fn load(path: &Path) -> Result<Self, ProbeError> {
        let contents = fs::read_to_string(path).map_err(|error| {
            ProbeError::new("browser_session_missing", "browser session was not initialized")
                .with_details(format!("{} ({error})", path.display()))
        })?;

        let mut session = Self::default();
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('\t') else {
                continue;
            };
            let value = decode_field(value);
            match key {
                "url" => session.url = value,
                "title" => session.title = value,
                "intro" => session.intro = value,
                "name" => session.name = value,
                "notes" => session.notes = value,
                "result" => session.result = value,
                _ => {}
            }
        }

        if session.url.is_empty() {
            return Err(
                ProbeError::new("browser_session_invalid", "browser session data was incomplete")
                    .with_details(path.display().to_string()),
            );
        }

        Ok(session)
    }

    fn save(&self, path: &Path) -> Result<(), ProbeError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("browser_session_prepare_failed", &parent.display().to_string(), error))?;
        }

        let payload = [
            ("url", &self.url),
            ("title", &self.title),
            ("intro", &self.intro),
            ("name", &self.name),
            ("notes", &self.notes),
            ("result", &self.result),
        ]
        .into_iter()
        .map(|(key, value)| format!("{key}\t{}", encode_field(value)))
        .collect::<Vec<_>>()
        .join("\n");

        fs::write(path, payload)
            .map_err(|error| io_error("browser_session_write_failed", &path.display().to_string(), error))
    }

    fn read_target(&self, selector: &str) -> Result<String, ProbeError> {
        match selector {
            "body" => Ok(self.body_text()),
            "#intro" => Ok(self.intro.clone()),
            "#name" => Ok(self.name.clone()),
            "#notes" => Ok(self.notes.clone()),
            "#result" => Ok(self.result.clone()),
            "#submit" => Ok("Submit".to_string()),
            other => Err(unsupported_selector_error(other)),
        }
    }

    fn fill(&mut self, selector: &str, value: &str) -> Result<(), ProbeError> {
        match selector {
            "#name" => self.name = value.to_string(),
            "#notes" => self.notes = value.to_string(),
            other => return Err(unsupported_selector_error(other)),
        }

        Ok(())
    }

    fn click(&mut self, selector: &str) -> Result<(), ProbeError> {
        match selector {
            "#submit" => {
                self.result = format!("Submitted:{}|{}", self.name, self.notes);
                Ok(())
            }
            other => Err(unsupported_selector_error(other)),
        }
    }

    fn body_text(&self) -> String {
        [
            self.title.clone(),
            self.intro.clone(),
            self.result.clone(),
        ]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
    }
}

fn file_url_to_path(url: &str) -> Result<PathBuf, ProbeError> {
    if let Some(path) = url.strip_prefix("file:///") {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = url.strip_prefix("file://") {
        return Ok(PathBuf::from(path));
    }

    if url.contains("://") {
        return Err(
            ProbeError::new("unsupported_url", "only file:// URLs are supported in local mode")
                .with_details(url.to_string()),
        );
    }

    Ok(PathBuf::from(url))
}

fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = html.find(&open)? + open.len();
    let end = html[start..].find(&close)? + start;
    Some(normalize_text(&html[start..end]))
}

fn extract_text_by_id(html: &str, id: &str) -> Option<String> {
    let marker = format!("id=\"{id}\"");
    let marker_index = html.find(&marker)?;
    let after_marker = &html[marker_index..];
    let content_start = after_marker.find('>')? + 1;
    let content = &after_marker[content_start..];
    let content_end = content.find('<')?;
    Some(normalize_text(&content[..content_end]))
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn encode_field(value: &str) -> String {
    let mut encoded = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            other => encoded.push(other),
        }
    }
    encoded
}

fn decode_field(value: &str) -> String {
    let mut decoded = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => decoded.push('\n'),
                Some('r') => decoded.push('\r'),
                Some('t') => decoded.push('\t'),
                Some('\\') => decoded.push('\\'),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(ch);
        }
    }
    decoded
}

fn io_error(code: &str, path: &str, error: std::io::Error) -> ProbeError {
    ProbeError::new(code, format!("local compatibility operation failed for {path}"))
        .with_details(error.to_string())
}

fn unsupported_selector_error(selector: &str) -> ProbeError {
    ProbeError::new(
        "unsupported_selector",
        "selector is not supported by the local browser compatibility adapter",
    )
    .with_details(selector.to_string())
}

fn saturating_i64(value: u64) -> i64 {
    value.try_into().unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn local_file_adapter_lists_reads_and_stats_paths() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let note_path = temp_root.join("note.txt");
        let nested_dir = temp_root.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(&note_path, "hello rover").unwrap();
        fs::write(nested_dir.join("child.txt"), "child").unwrap();

        let adapter = LocalFileAdapter;
        let listed = adapter
            .run(FileRequest::List {
                path: temp_root.display().to_string(),
            })
            .unwrap();
        let statted = adapter
            .run(FileRequest::Stat {
                path: note_path.display().to_string(),
            })
            .unwrap();
        let opened = adapter
            .run(FileRequest::Open {
                path: note_path.display().to_string(),
            })
            .unwrap();

        assert_eq!(listed.status, Status::Success);
        assert_eq!(array_field_len(&listed, "entries"), 2);
        assert!(rendered_json(&listed).contains("note.txt"));
        assert!(rendered_json(&listed).contains("nested"));
        assert_eq!(bool_field(&statted, "is_file"), true);
        assert_eq!(number_field(&statted, "len"), 11);
        assert_eq!(string_field(&opened, "contents"), "hello rover");

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_file_adapter_reports_missing_paths() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let missing_path = temp_root.join("missing.txt");
        let adapter = LocalFileAdapter;

        let stat_error = adapter
            .run(FileRequest::Stat {
                path: missing_path.display().to_string(),
            })
            .unwrap_err();
        let open_error = adapter
            .run(FileRequest::Open {
                path: missing_path.display().to_string(),
            })
            .unwrap_err();

        assert_eq!(stat_error.code, "stat_failed");
        assert_eq!(open_error.code, "open_failed");

        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_file_adapter_handles_readonly_large_files_and_directory_delete() {
        let temp_root = unique_temp_dir();
        let nested_dir = temp_root.join("deep").join("nested");
        let large_path = nested_dir.join("large.txt");
        fs::create_dir_all(&nested_dir).unwrap();
        let payload = ascii_payload(1024 * 1024, "large-payload|");
        fs::write(&large_path, &payload).unwrap();

        let mut permissions = fs::metadata(&large_path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&large_path, permissions).unwrap();

        let adapter = LocalFileAdapter;
        let statted = adapter
            .run(FileRequest::Stat {
                path: large_path.display().to_string(),
            })
            .unwrap();
        let opened = adapter
            .run(FileRequest::Open {
                path: large_path.display().to_string(),
            })
            .unwrap();

        assert_eq!(bool_field(&statted, "readonly"), true);
        assert_eq!(number_field(&statted, "len"), payload.len() as i64);
        assert_eq!(string_field(&opened, "contents").len(), payload.len());

        let mut reset_permissions = fs::metadata(&large_path).unwrap().permissions();
        reset_permissions.set_readonly(false);
        fs::set_permissions(&large_path, reset_permissions).unwrap();

        adapter
            .run(FileRequest::Delete {
                path: temp_root.join("deep").display().to_string(),
            })
            .unwrap();

        assert!(!temp_root.join("deep").exists());
        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_file_adapter_stress_cycles_copy_move_delete_paths() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let source = temp_root.join("source.txt");
        let copied = temp_root.join("copied.txt");
        let moved = temp_root.join("moved.txt");
        let payload = ascii_payload(1024 * 1024, "stress-file|");
        fs::write(&source, &payload).unwrap();

        let adapter = LocalFileAdapter;
        for _ in 0..8 {
            let copy_result = adapter
                .run(FileRequest::Copy {
                    source: source.display().to_string(),
                    destination: copied.display().to_string(),
                })
                .unwrap();
            let move_result = adapter
                .run(FileRequest::Move {
                    source: copied.display().to_string(),
                    destination: moved.display().to_string(),
                })
                .unwrap();
            let delete_result = adapter
                .run(FileRequest::Delete {
                    path: moved.display().to_string(),
                })
                .unwrap();

            assert_eq!(copy_result.status, Status::Success);
            assert_eq!(move_result.status, Status::Success);
            assert_eq!(delete_result.status, Status::Success);
            assert!(!copied.exists());
            assert!(!moved.exists());
        }

        assert!(source.exists());
        assert_eq!(fs::read_to_string(&source).unwrap().len(), payload.len());
        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_browser_reads_body_downloads_fixture_and_preserves_state() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let fixture_path = temp_root.join("fixture.html");
        fs::write(&fixture_path, browser_fixture_html()).unwrap();

        let adapter = LocalBrowserAdapter::new(temp_root.join("session.state"));
        let fixture_url = to_file_url(&fixture_path);
        let download_path = temp_root.join("downloaded.html");

        let opened = adapter
            .run(BrowserRequest::Open {
                url: fixture_url.clone(),
            })
            .unwrap();
        let body = adapter
            .run(BrowserRequest::Read {
                target: Some("body".into()),
            })
            .unwrap();
        adapter
            .run(BrowserRequest::Fill {
                target: "#name".into(),
                value: "Rover".into(),
            })
            .unwrap();
        adapter
            .run(BrowserRequest::Fill {
                target: "#notes".into(),
                value: "Ready".into(),
            })
            .unwrap();
        adapter
            .run(BrowserRequest::Click {
                target: "#submit".into(),
            })
            .unwrap();
        let result = adapter
            .run(BrowserRequest::Read {
                target: Some("#result".into()),
            })
            .unwrap();
        let downloaded = adapter
            .run(BrowserRequest::Download {
                url: fixture_url,
                destination: Some(download_path.display().to_string()),
            })
            .unwrap();

        assert_eq!(string_field(&opened, "title"), "Fixture");
        assert!(string_field(&body, "text").contains("Waiting for input"));
        assert_eq!(string_field(&result, "text"), "Submitted:Rover|Ready");
        assert_eq!(string_field(&downloaded, "destination"), download_path.display().to_string());
        assert!(download_path.exists());
        assert!(fs::read_to_string(download_path).unwrap().contains("benchmark-form"));
        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_browser_requires_open_before_session_actions() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let adapter = LocalBrowserAdapter::new(temp_root.join("session.state"));
        let error = adapter
            .run(BrowserRequest::Read {
                target: Some("#result".into()),
            })
            .unwrap_err();

        assert_eq!(error.code, "browser_session_missing");
        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_browser_rejects_unsupported_selectors() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let fixture_path = temp_root.join("fixture.html");
        fs::write(&fixture_path, browser_fixture_html()).unwrap();

        let adapter = LocalBrowserAdapter::new(temp_root.join("session.state"));
        adapter
            .run(BrowserRequest::Open {
                url: to_file_url(&fixture_path),
            })
            .unwrap();

        let error = adapter
            .run(BrowserRequest::Fill {
                target: "#missing".into(),
                value: "nope".into(),
            })
            .unwrap_err();

        assert_eq!(error.code, "unsupported_selector");
        fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn local_browser_stress_loop_handles_large_payloads() {
        let temp_root = unique_temp_dir();
        fs::create_dir_all(&temp_root).unwrap();

        let fixture_path = temp_root.join("fixture.html");
        fs::write(&fixture_path, browser_fixture_html()).unwrap();

        let adapter = LocalBrowserAdapter::new(temp_root.join("session.state"));
        let fixture_url = to_file_url(&fixture_path);

        for iteration in 0..4 {
            let name = format!("worker-{iteration}");
            let notes = ascii_payload(64 * 1024, &format!("notes-{iteration}|"));
            let download_path = temp_root.join(format!("download-{iteration}.html"));

            adapter
                .run(BrowserRequest::Open {
                    url: fixture_url.clone(),
                })
                .unwrap();
            let body = adapter
                .run(BrowserRequest::Read {
                    target: Some("body".into()),
                })
                .unwrap();
            adapter
                .run(BrowserRequest::Fill {
                    target: "#name".into(),
                    value: name.clone(),
                })
                .unwrap();
            adapter
                .run(BrowserRequest::Fill {
                    target: "#notes".into(),
                    value: notes.clone(),
                })
                .unwrap();
            adapter
                .run(BrowserRequest::Click {
                    target: "#submit".into(),
                })
                .unwrap();
            let result = adapter
                .run(BrowserRequest::Read {
                    target: Some("#result".into()),
                })
                .unwrap();
            let downloaded = adapter
                .run(BrowserRequest::Download {
                    url: fixture_url.clone(),
                    destination: Some(download_path.display().to_string()),
                })
                .unwrap();

            assert!(string_field(&body, "text").contains("Waiting for input"));
            assert_eq!(
                string_field(&result, "text"),
                format!("Submitted:{name}|{notes}")
            );
            assert_eq!(number_field(&downloaded, "bytes_copied") > 0, true);
            assert!(download_path.exists());
        }

        fs::remove_dir_all(temp_root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "rover-probe-local-compat-{}-{}",
            std::process::id(),
            suffix
        ))
    }

    fn browser_fixture_html() -> &'static str {
        r#"<!doctype html>
        <html>
          <head><title>Fixture</title></head>
          <body>
            <form id="benchmark-form">
              <p id="intro">Hello benchmark</p>
              <input id="name" />
              <textarea id="notes"></textarea>
              <button id="submit">Submit</button>
            </form>
            <div id="result">Waiting for input</div>
          </body>
        </html>"#
    }

    fn to_file_url(path: &Path) -> String {
        format!("file:///{}", path.display().to_string().replace('\\', "/"))
    }

    fn ascii_payload(size_bytes: usize, prefix: &str) -> String {
        let mut payload = String::from(prefix);
        while payload.len() < size_bytes {
            payload.push('x');
        }
        payload.truncate(size_bytes);
        payload
    }

    fn rendered_json(result: &ProbeResult) -> String {
        result.render(rover_core::RenderMode::Json)
    }

    fn string_field(result: &ProbeResult, key: &str) -> String {
        match output_field(result, key) {
            OutputValue::String(value) => value.clone(),
            other => panic!("expected string field `{key}`, got {other:?}"),
        }
    }

    fn bool_field(result: &ProbeResult, key: &str) -> bool {
        match output_field(result, key) {
            OutputValue::Bool(value) => *value,
            other => panic!("expected bool field `{key}`, got {other:?}"),
        }
    }

    fn number_field(result: &ProbeResult, key: &str) -> i64 {
        match output_field(result, key) {
            OutputValue::Number(value) => *value,
            other => panic!("expected number field `{key}`, got {other:?}"),
        }
    }

    fn array_field_len(result: &ProbeResult, key: &str) -> usize {
        match output_field(result, key) {
            OutputValue::Array(values) => values.len(),
            other => panic!("expected array field `{key}`, got {other:?}"),
        }
    }

    fn output_field<'a>(result: &'a ProbeResult, key: &str) -> &'a OutputValue {
        match &result.structured_output {
            OutputValue::Object(entries) => entries
                .get(key)
                .unwrap_or_else(|| panic!("missing field `{key}` in structured output")),
            other => panic!("expected object structured output, got {other:?}"),
        }
    }
}
