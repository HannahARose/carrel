//! The Mac agent's lock service: on a Mac the agent reaches `CloudKit`, and the app and the
//! command line take an archive's writer lock through it (`docs/design/sync-and-mobile.md`,
//! decision 2). The agent serves its [`RemoteLock`] on a Unix socket in the configuration
//! folder, [`SOCKET_NAME`]; each connection carries one request and its reply, as a line of
//! JSON each way.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::cloud::{LockRecord, RemoteError, RemoteLock, Saved, StoredRecord};
use crate::card::Id;

/// The socket's name in the configuration folder.
pub const SOCKET_NAME: &str = "cloud-lock.sock";

/// Set to the socket's path to use another, as tests do.
pub const SOCKET_ENV: &str = "CARREL_CLOUD_LOCK_SOCKET";

/// How long a request may take, `CloudKit`'s round trip included.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where the agent's socket is: [`SOCKET_ENV`], else [`SOCKET_NAME`] in the configuration
/// folder.
#[must_use]
pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    crate::local::config_dir().map(|dir| dir.join(SOCKET_NAME))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum Request {
    Fetch {
        archive: Id,
    },
    Save {
        archive: Id,
        record: Box<LockRecord>,
        expected: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Reply {
    Record(Option<Box<StoredRecord>>),
    Saved(Saved),
    Error(String),
}

/// The agent's lock service, as the app and the command line reach it.
#[derive(Debug, Clone)]
pub struct AgentLockClient {
    path: PathBuf,
}

impl AgentLockClient {
    /// A client of the service at `path`.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// A client of the agent's service, when it's running.
    #[must_use]
    pub fn running() -> Option<Self> {
        let path = socket_path()?;
        UnixStream::connect(&path).ok()?;
        Some(Self::new(path))
    }

    fn send(&self, request: &Request) -> Result<Reply, RemoteError> {
        let failed = |e: std::io::Error| RemoteError(format!("Carrel Agent didn't answer: {e}"));
        let mut stream = UnixStream::connect(&self.path).map_err(failed)?;
        stream.set_read_timeout(Some(TIMEOUT)).map_err(failed)?;
        let mut line = serde_json::to_string(request).expect("requests always serialize");
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(failed)?;
        let mut reply = String::new();
        BufReader::new(stream)
            .read_line(&mut reply)
            .map_err(failed)?;
        serde_json::from_str(&reply)
            .map_err(|_| RemoteError("Carrel Agent's reply couldn't be read".into()))
    }
}

impl RemoteLock for AgentLockClient {
    fn fetch(&self, archive: Id) -> Result<Option<StoredRecord>, RemoteError> {
        match self.send(&Request::Fetch { archive })? {
            Reply::Record(record) => Ok(record.map(|r| *r)),
            Reply::Error(e) => Err(RemoteError(e)),
            Reply::Saved(_) => Err(RemoteError("Carrel Agent answered another request".into())),
        }
    }

    fn save(
        &self,
        archive: Id,
        record: &LockRecord,
        expected: Option<&str>,
    ) -> Result<Saved, RemoteError> {
        let request = Request::Save {
            archive,
            record: Box::new(record.clone()),
            expected: expected.map(str::to_owned),
        };
        match self.send(&request)? {
            Reply::Saved(saved) => Ok(saved),
            Reply::Error(e) => Err(RemoteError(e)),
            Reply::Record(_) => Err(RemoteError("Carrel Agent answered another request".into())),
        }
    }
}

/// The agent's side: serves `remote` on the socket until dropped.
#[derive(Debug)]
pub struct AgentLockService {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl AgentLockService {
    /// Serves `remote` at `path`, replacing a socket left by an agent that has stopped.
    ///
    /// # Errors
    ///
    /// [`std::io::ErrorKind::AddrInUse`] if another agent is serving there, or an I/O error.
    pub fn start(path: &Path, remote: Arc<dyn RemoteLock>) -> std::io::Result<Self> {
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another Carrel Agent is serving the writer lock",
            ));
        }
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(path)?;
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("carrel-cloud-lock".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let remote = Arc::clone(&remote);
                    std::thread::spawn(move || answer(&stream, remote.as_ref()));
                }
            })?;
        Ok(Self {
            path: path.to_owned(),
            stop,
        })
    }

    /// The socket's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AgentLockService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread so it sees the stop.
        let _ = UnixStream::connect(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn answer(stream: &UnixStream, remote: &dyn RemoteLock) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    if reader.read_line(&mut line).is_err() || line.is_empty() {
        return;
    }
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(Request::Fetch { archive }) => remote
            .fetch(archive)
            .map_or_else(|e| Reply::Error(e.0), |r| Reply::Record(r.map(Box::new))),
        Ok(Request::Save {
            archive,
            record,
            expected,
        }) => remote
            .save(archive, &record, expected.as_deref())
            .map_or_else(|e| Reply::Error(e.0), Reply::Saved),
        Err(_) => Reply::Error("Carrel Agent didn't understand the request; update Carrel".into()),
    };
    let mut text = serde_json::to_string(&reply).expect("replies always serialize");
    text.push('\n');
    let mut stream = stream;
    let _ = stream.write_all(text.as_bytes());
}
