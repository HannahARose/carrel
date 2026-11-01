//! Running provider programs: finding them, starting them and speaking the provider protocol
//! with them (`docs/providers.md`).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Completion, CompletionRequest, EmbeddingRequest, Embeddings, Extraction, ExtractionRequest,
    Offer, PROTOCOL_VERSION, Provider, ProviderError,
};

/// The start of every provider program's name.
pub const PREFIX: &str = "carrel-provider-";

/// How long a provider has to answer `initialize`.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a provider has to answer any other request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_mins(10);

/// How many of a provider's last log lines are kept for error messages.
const LOG_LINES: usize = 20;

pub use super::device::Found;

/// Finds provider programs in `folders`, in order. When the same name appears in several
/// folders, the first is used.
#[must_use]
pub fn discover(folders: &[PathBuf]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        let mut here: Vec<Found> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = provider_name(&path)?;
                is_executable(&path).then_some(Found { name, path })
            })
            .collect();
        here.sort_by(|a, b| a.name.cmp(&b.name));
        for candidate in here {
            if !found.iter().any(|f| f.name == candidate.name) {
                found.push(candidate);
            }
        }
    }
    found
}

/// The first executable named `name` (plus `.exe` on Windows) in `folders`, such as a tool
/// Carrel runs that isn't a provider.
#[must_use]
pub fn find_program(name: &str, folders: &[PathBuf]) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    folders
        .iter()
        .map(|folder| folder.join(&file))
        .find(|path| is_executable(path))
}

/// The folders Carrel looks in: the one holding the running program, an app bundle's
/// `Contents/Helpers` when it runs from one, then `PATH`.
#[must_use]
pub fn default_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        folders.push(dir.to_owned());
        if dir.ends_with("Contents/MacOS")
            && let Some(contents) = dir.parent()
        {
            folders.push(contents.join("Helpers"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        folders.extend(std::env::split_paths(&path));
    }
    folders
}

fn provider_name(path: &Path) -> Option<String> {
    let file = path.file_name()?.to_str()?;
    let file = file
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .filter(|_| !std::env::consts::EXE_SUFFIX.is_empty())
        .unwrap_or(file);
    let name = file.strip_prefix(PREFIX)?;
    (!name.is_empty() && !name.contains('.')).then(|| name.to_owned())
}

/// Starts `path` with piped standard streams. On Linux, a program that was written a moment
/// ago can be briefly "busy" while another newly started process still holds it open, so that
/// error is retried for a short while.
fn spawn(path: &Path) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        let result = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match result {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 50 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file() && path.extension() == Some(std::ffi::OsStr::new("exe"))
}

/// A provider program: running, or, when made from what it offered before
/// ([`ProcessProvider::introduced`]), started the first time it's asked to do something.
pub struct ProcessProvider {
    found: Found,
    introduced: Introduced,
    request_timeout: Duration,
    connection: Mutex<Link>,
}

/// What a provider program says about itself when it starts: kept, so that a registry can
/// list it later without starting it again (#333).
#[derive(Debug, Clone, PartialEq)]
pub struct Introduced {
    /// The name it gives itself, such as "Apple".
    pub label: Option<String>,
    /// Its version, if it reports one.
    pub version: Option<String>,
    /// What it offers.
    pub offers: Vec<Offer>,
}

/// The conversation with a provider program, once there is one.
enum Link {
    /// Not started yet.
    Waiting,
    Running(Connection),
    /// It couldn't be started, and why.
    Failed(String),
}

impl std::fmt::Debug for ProcessProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessProvider")
            .field("name", &self.found.name)
            .field("version", &self.introduced.version)
            .field("offers", &self.introduced.offers)
            .finish_non_exhaustive()
    }
}

struct Connection {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    log: Arc<Mutex<VecDeque<String>>>,
    /// Disconnects when the provider's standard error has been read to the end.
    log_done: Receiver<()>,
    next_id: u64,
    broken: Option<String>,
}

#[derive(Deserialize)]
struct Initialized {
    protocol: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    offers: Vec<Offer>,
}

impl ProcessProvider {
    /// Starts the provider program `found` and asks what it offers.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Protocol`] if it can't be started, doesn't answer `initialize` within
    /// [`INITIALIZE_TIMEOUT`], or speaks another protocol version.
    pub fn start(found: &Found) -> Result<Self, ProviderError> {
        let (connection, introduced) = open(found)?;
        Ok(Self {
            found: found.clone(),
            introduced,
            request_timeout: REQUEST_TIMEOUT,
            connection: Mutex::new(Link::Running(connection)),
        })
    }

    /// The provider program `found`, which offered `introduced` when it last started. It
    /// isn't started until it's asked to do something, and then only once: if it can't
    /// start then, that request and every later one fail with why.
    #[must_use]
    pub fn introduced(found: Found, introduced: Introduced) -> Self {
        Self {
            found,
            introduced,
            request_timeout: REQUEST_TIMEOUT,
            connection: Mutex::new(Link::Waiting),
        }
    }

    /// What the provider said about itself when it started.
    #[must_use]
    pub fn introduction(&self) -> &Introduced {
        &self.introduced
    }

    /// The name the provider gives itself, such as "Apple".
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.introduced.label.as_deref()
    }

    /// Changes how long requests may take before the provider is given up on.
    pub fn set_request_timeout(&mut self, timeout: Duration) {
        self.request_timeout = timeout;
    }

    /// The provider's version, if it reports one.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.introduced.version.as_deref()
    }

    fn request<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, ProviderError> {
        let mut link = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*link, Link::Waiting) {
            *link = match open(&self.found) {
                Ok((connection, _)) => Link::Running(connection),
                Err(e) => Link::Failed(e.to_string()),
            };
        }
        let connection = match &mut *link {
            Link::Running(connection) => connection,
            Link::Failed(why) => return Err(ProviderError::Protocol(why.clone())),
            Link::Waiting => unreachable!("started above"),
        };
        let result = connection.call(method, Some(params), self.request_timeout)?;
        serde_json::from_value(result).map_err(|e| {
            ProviderError::Protocol(format!(
                "{}: unexpected {method} result: {e}",
                self.found.name
            ))
        })
    }
}

impl Provider for ProcessProvider {
    fn name(&self) -> &str {
        &self.found.name
    }

    fn program(&self) -> Option<super::Program> {
        Some(super::Program {
            path: self.found.path.clone(),
            label: self.introduced.label.clone(),
            version: self.introduced.version.clone(),
        })
    }

    fn offers(&self) -> &[Offer] {
        &self.introduced.offers
    }

    fn complete(
        &self,
        offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let mut params = serde_json::to_value(request).expect("requests always serialize");
        params["offer"] = Value::from(offer);
        self.request("text/complete", params)
    }

    fn extract(
        &self,
        offer: &str,
        request: &ExtractionRequest,
    ) -> Result<Extraction, ProviderError> {
        let mut params = serde_json::to_value(request).expect("requests always serialize");
        params["offer"] = Value::from(offer);
        self.request("text/extract", params)
    }

    fn embed(&self, offer: &str, request: &EmbeddingRequest) -> Result<Embeddings, ProviderError> {
        let mut params = serde_json::to_value(request).expect("requests always serialize");
        params["offer"] = Value::from(offer);
        self.request("text/embed", params)
    }

    fn count_tokens(&self, offer: &str, text: &str) -> Result<u64, ProviderError> {
        #[derive(Deserialize)]
        struct Counted {
            tokens: u64,
        }
        let counted: Counted =
            self.request("text/count-tokens", json!({"offer": offer, "text": text}))?;
        Ok(counted.tokens)
    }
}

impl Drop for ProcessProvider {
    fn drop(&mut self) {
        let Link::Running(connection) = self
            .connection
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        else {
            return;
        };
        if connection.broken.is_none() {
            let _ = connection.call("shutdown", None, Duration::from_secs(2));
        }
        // Closing standard input tells the provider to exit.
        connection.stdin = None;
        for _ in 0..20 {
            if matches!(connection.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = connection.child.kill();
        let _ = connection.child.wait();
    }
}

/// Starts the provider program `found` and asks what it offers.
fn open(found: &Found) -> Result<(Connection, Introduced), ProviderError> {
    let mut child = spawn(&found.path).map_err(|e| {
        ProviderError::Protocol(format!("can't start {}: {e}", found.path.display()))
    })?;
    let stdin = child.stdin.take();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(ProviderError::Protocol(format!(
            "{}: no output streams",
            found.name
        )));
    };

    let (sender, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let log = Arc::new(Mutex::new(VecDeque::new()));
    let log_writer = Arc::clone(&log);
    let (log_finished, log_done) = channel::<()>();
    std::thread::spawn(move || {
        // Dropped when the log ends, which tells `failure` the log is complete.
        let _finished = log_finished;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let mut log = log_writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if log.len() == LOG_LINES {
                log.pop_front();
            }
            log.push_back(line);
        }
    });

    let mut connection = Connection {
        child,
        stdin,
        lines,
        log,
        log_done,
        next_id: 1,
        broken: None,
    };
    let params = json!({
        "protocol": PROTOCOL_VERSION,
        "client": format!("carrel {}", crate::CARREL_VERSION),
    });
    let result = connection.call("initialize", Some(params), INITIALIZE_TIMEOUT)?;
    let initialized: Initialized = serde_json::from_value(result).map_err(|e| {
        ProviderError::Protocol(format!("{}: unexpected initialize result: {e}", found.name))
    })?;
    if initialized.protocol != PROTOCOL_VERSION {
        return Err(ProviderError::Protocol(format!(
            "{} speaks provider protocol {}, but this Carrel speaks {PROTOCOL_VERSION}",
            found.name, initialized.protocol
        )));
    }
    let introduced = Introduced {
        label: initialized.name,
        version: initialized.version,
        offers: initialized.offers,
    };
    Ok((connection, introduced))
}

impl Connection {
    fn call(
        &mut self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        if let Some(why) = &self.broken {
            return Err(ProviderError::Protocol(why.clone()));
        }
        let result = self.exchange(method, params, timeout);
        if let Err(ProviderError::Protocol(why)) = &result {
            // The conversation can't be trusted after a protocol failure.
            self.broken = Some(why.clone());
            let _ = self.child.kill();
        }
        result
    }

    fn exchange(
        &mut self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if let Some(params) = params {
            message["params"] = params;
        }
        let line = message.to_string() + "\n";
        let written = match self.stdin.as_mut() {
            Some(stdin) => stdin
                .write_all(line.as_bytes())
                .and_then(|()| stdin.flush()),
            None => return Err(self.failure("standard input is closed")),
        };
        if let Err(e) = written {
            return Err(self.failure(&format!("can't send {method}: {e}")));
        }

        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(self.failure(&format!("no answer to {method} within {timeout:?}")));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.failure(&format!("exited before answering {method}")));
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let reply: Value = serde_json::from_str(&line).map_err(|_| {
                self.failure(&format!(
                    "sent something that isn't JSON: {}",
                    shorten(&line)
                ))
            })?;
            if reply.get("id").and_then(Value::as_u64) != Some(id) {
                // A notification, or a stray reply; neither is ours.
                continue;
            }
            if let Some(error) = reply.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("no message")
                    .to_owned();
                return Err(ProviderError::from_code(code, message));
            }
            return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// A protocol failure, with the provider's last log lines. A provider that has exited may
    /// still have log lines on their way, so this waits briefly for its log to end.
    fn failure(&self, what: &str) -> ProviderError {
        let _ = self.log_done.recv_timeout(Duration::from_millis(200));
        let log = self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if log.is_empty() {
            ProviderError::Protocol(what.to_owned())
        } else {
            let lines: Vec<&str> = log.iter().map(String::as_str).collect();
            ProviderError::Protocol(format!("{what}; its log ends:\n{}", lines.join("\n")))
        }
    }
}

fn shorten(text: &str) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        short + "…"
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_come_from_program_names() {
        let name = |file: &str| provider_name(Path::new(file));
        assert_eq!(name("/bin/carrel-provider-apple").as_deref(), Some("apple"));
        assert_eq!(
            name("carrel-provider-llama-cpp").as_deref(),
            Some("llama-cpp")
        );
        assert_eq!(name("carrel-provider-").as_deref(), None);
        assert_eq!(name("carrel").as_deref(), None);
        assert_eq!(name("carrel-provider-apple.txt").as_deref(), None);
        if std::env::consts::EXE_SUFFIX == ".exe" {
            assert_eq!(name("carrel-provider-apple.exe").as_deref(), Some("apple"));
        }
    }
}
