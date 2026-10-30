//! `carrel-fetch`: Carrel's network helper, the only Carrel program that goes online
//! (`docs/design/models.md`).
//!
//! It reads one [`FetchRequest`] as JSON on standard input, downloads the URL into the file it
//! names, and reports [`FetchEvent`]s as JSON lines on standard output. It follows redirects
//! only to the hosts the request allows, continues a partial file with a range request, and
//! keeps the file only if its size and SHA-256 are the ones asked for. Every request it makes
//! is appended to the request's log. It never reads anything but that one file.

use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, Read as _, Seek as _, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use carrel_core::models::{FetchEvent, FetchRequest, hex, host_allowed};
use sha2::{Digest as _, Sha256};
use ureq::http::Uri;

/// How many redirects to follow before giving up.
const MAX_REDIRECTS: usize = 10;

/// Free space to leave on the disk beyond the file itself.
const MARGIN: u64 = 256 * 1024 * 1024;

fn main() -> ExitCode {
    let mut line = String::new();
    let outcome = match std::io::stdin().lock().read_line(&mut line) {
        Ok(_) => match serde_json::from_str::<FetchRequest>(&line) {
            Ok(request) => fetch(&request),
            Err(e) => Err(format!("unreadable request: {e}")),
        },
        Err(e) => Err(format!("couldn't read the request: {e}")),
    };
    let (event, status) = match outcome {
        Ok((bytes, sha256)) => (FetchEvent::Done { bytes, sha256 }, ExitCode::SUCCESS),
        Err(message) => (FetchEvent::Failed { message }, ExitCode::FAILURE),
    };
    emit(&event);
    status
}

/// Writes one event to standard output.
fn emit(event: &FetchEvent) {
    if let Ok(line) = serde_json::to_string(event) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// The file being written, and the hash of what's in it so far.
struct Part {
    file: File,
    hasher: Sha256,
    written: u64,
}

impl Part {
    /// Opens `path`, hashing what an earlier attempt left. A file longer than `size` can't be
    /// a start of the right one, so it's emptied.
    fn open(path: &Path, size: u64) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let mut part = Self {
            hasher: Sha256::new(),
            written: 0,
            file: file.try_clone()?,
        };
        if file.metadata()?.len() > size {
            part.restart()?;
            return Ok(part);
        }
        let mut buffer = vec![0; 1 << 20];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            part.hasher.update(&buffer[..read]);
            part.written += read as u64;
        }
        Ok(part)
    }

    /// Empties the file, for a server that sent the whole file instead of the rest of it.
    fn restart(&mut self) -> std::io::Result<()> {
        self.file.set_len(0)?;
        self.file.rewind()?;
        self.hasher = Sha256::new();
        self.written = 0;
        Ok(())
    }

    fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.file.seek(std::io::SeekFrom::End(0))?;
        self.file.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }
}

/// Downloads as `request` asks, giving the finished file's size and SHA-256.
fn fetch(request: &FetchRequest) -> Result<(u64, String), String> {
    let at = |e: std::io::Error| format!("{}: {e}", request.to.display());
    let mut part = Part::open(&request.to, request.size).map_err(at)?;
    if part.written < request.size {
        let folder = request
            .to
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let needed = request.size - part.written + MARGIN;
        let free = fs4::available_space(folder).map_err(at)?;
        if free < needed {
            return Err(format!(
                "not enough space: {} free in {}, {} needed",
                gigabytes(free),
                folder.display(),
                gigabytes(needed)
            ));
        }
        download(request, &mut part)?;
    }
    let sha256 = hex(&part.hasher.clone().finalize());
    let written = part.written;
    if written != request.size || sha256 != request.sha256 {
        drop(part);
        let _ = std::fs::remove_file(&request.to);
        return Err(format!(
            "the file isn't the one expected ({} bytes with SHA-256 {}, received {written} bytes with {sha256}), so it was deleted",
            request.size, request.sha256
        ));
    }
    Ok((written, sha256))
}

/// Requests the rest of the file, following allowed redirects, and appends what comes.
fn download(request: &FetchRequest, part: &mut Part) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(format!("carrel-fetch/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut url = request.url.clone();
    for _ in 0..=MAX_REDIRECTS {
        let uri = check(&url, &request.hosts)?;
        let mut call = agent.get(&url);
        if part.written > 0 {
            call = call.header("Range", format!("bytes={}-", part.written));
        }
        let response = call.call();
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                log(request, &uri, None, 0, &e.to_string());
                return Err(format!("{}: {e}", shown(&uri)));
            }
        };
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .ok_or_else(|| format!("{}: a redirect with nowhere to go", shown(&uri)))?;
            log(request, &uri, Some(status), 0, "redirect");
            url = resolve(&uri, location);
            continue;
        }
        let result = match status {
            206 => receive(request, part, response.into_body()),
            200 => {
                // The server ignored the range, or there was none: start again from the top.
                part.restart()
                    .map_err(|e| e.to_string())
                    .and_then(|()| receive(request, part, response.into_body()))
            }
            416 if part.written == request.size => Ok(0),
            _ => Err(format!("{}: HTTP {status}", shown(&uri))),
        };
        let (bytes, note) = match &result {
            Ok(bytes) => (*bytes, "ok".to_owned()),
            Err(e) => (0, e.clone()),
        };
        log(request, &uri, Some(status), bytes, &note);
        return result.map(|_| ());
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}

/// Appends the body to the file, reporting progress about every 1%. Gives the bytes received.
fn receive(request: &FetchRequest, part: &mut Part, body: ureq::Body) -> Result<u64, String> {
    let mut reader = body.into_reader();
    let mut buffer = vec![0; 1 << 16];
    let step = (request.size / 100).max(1);
    let mut next = part.written + step;
    let start = part.written;
    emit(&FetchEvent::Progress {
        done: part.written,
        total: request.size,
    });
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        if part.written + read as u64 > request.size {
            return Err("the server sent more than the file's size".into());
        }
        part.append(&buffer[..read])
            .map_err(|e| format!("{}: {e}", request.to.display()))?;
        if part.written >= next {
            emit(&FetchEvent::Progress {
                done: part.written,
                total: request.size,
            });
            next = part.written + step;
        }
    }
    part.file.flush().map_err(|e| e.to_string())?;
    part.file.sync_all().map_err(|e| e.to_string())?;
    emit(&FetchEvent::Progress {
        done: part.written,
        total: request.size,
    });
    Ok(part.written - start)
}

/// Parses `url` and checks it may be fetched: `https` on an allowed host, or plain `http` to
/// this machine only.
fn check(url: &str, hosts: &[String]) -> Result<Uri, String> {
    let uri: Uri = url.parse().map_err(|e| format!("{url}: {e}"))?;
    let host = uri.host().unwrap_or_default();
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    match uri.scheme_str() {
        Some("https") => {}
        Some("http") if loopback => {}
        _ => return Err(format!("{}: only https downloads are allowed", shown(&uri))),
    }
    if !host_allowed(host, hosts) {
        return Err(format!("{host} isn't an allowed host for this download"));
    }
    Ok(uri)
}

/// Where a redirect's `location` points, relative to `from`.
fn resolve(from: &Uri, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }
    let origin = format!(
        "{}://{}",
        from.scheme_str().unwrap_or("https"),
        from.authority()
            .map_or("", ureq::http::uri::Authority::as_str)
    );
    if location.starts_with('/') {
        format!("{origin}{location}")
    } else {
        let path = from.path();
        let folder = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        format!("{origin}{folder}{location}")
    }
}

/// A URL without its query, which for signed CDN links is long and says nothing useful.
fn shown(uri: &Uri) -> String {
    format!(
        "{}://{}{}",
        uri.scheme_str().unwrap_or(""),
        uri.authority()
            .map_or("", ureq::http::uri::Authority::as_str),
        uri.path()
    )
}

/// Appends a line for one request to the request's log, if it has one. A log that can't be
/// written doesn't stop the download.
fn log(request: &FetchRequest, uri: &Uri, status: Option<u16>, bytes: u64, result: &str) {
    let Some(path) = &request.log else {
        return;
    };
    let line = serde_json::json!({
        "time": jiff::Timestamp::now().to_string(),
        "program": "carrel-fetch",
        "url": shown(uri),
        "status": status,
        "bytes": bytes,
        "sha256": request.sha256,
        "result": result,
    });
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

#[allow(clippy::cast_precision_loss)]
fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}
