//! The network helper against a small HTTP server on this machine.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use carrel_core::models::{FetchRequest, Fetcher as _, Helper, ModelError, hex};
use sha2::{Digest as _, Sha256};

/// The model file the server offers.
const CONTENTS: &[u8] = &[7; 300_000];

/// A server on 127.0.0.1 that offers [`CONTENTS`] at `/model` with range requests, redirects
/// `/moved` there, redirects `/away` to another host, and records each request's range.
struct Server {
    base: String,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&ranges);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let mut range = None;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("range:") {
                        range = Some(value.trim().to_owned());
                    }
                }
                seen.lock().unwrap().push(range.clone());
                let response = match path.as_str() {
                    "/model" => {
                        let start = range
                            .as_deref()
                            .and_then(|r| r.strip_prefix("bytes="))
                            .and_then(|r| r.strip_suffix('-'))
                            .and_then(|r| r.parse::<usize>().ok());
                        let status = if start.is_some() { "206 Partial Content" } else { "200 OK" };
                        let body = &CONTENTS[start.unwrap_or(0)..];
                        let mut head = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        head.extend_from_slice(body);
                        head
                    }
                    "/moved" => {
                        b"HTTP/1.1 302 Found\r\nLocation: /model\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                    "/away" => b"HTTP/1.1 302 Found\r\nLocation: https://example.com/model\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec(),
                    _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec(),
                };
                let _ = stream.write_all(&response);
            }
        });
        Self { base, ranges }
    }

    fn ranges(&self) -> Vec<Option<String>> {
        self.ranges.lock().unwrap().clone()
    }
}

fn helper() -> Helper {
    Helper::at(PathBuf::from(env!("CARGO_BIN_EXE_carrel-fetch")))
}

fn request(url: String, to: &Path, log: &Path) -> FetchRequest {
    FetchRequest {
        url,
        hosts: vec!["127.0.0.1".into()],
        size: CONTENTS.len() as u64,
        sha256: hex(&Sha256::digest(CONTENTS)),
        to: to.to_owned(),
        log: Some(log.to_owned()),
    }
}

#[test]
fn downloads_a_file_and_logs_the_request() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("m.gguf.part");
    let log = dir.path().join("logs/network.log");
    let mut progress = Vec::new();
    helper()
        .fetch(
            &request(format!("{}/model", server.base), &to, &log),
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), CONTENTS);
    assert_eq!(progress.first(), Some(&(0, 300_000)));
    assert_eq!(progress.last(), Some(&(300_000, 300_000)));
    let log = std::fs::read_to_string(log).unwrap();
    let line: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(line["status"], 200);
    assert_eq!(line["bytes"], 300_000);
    assert_eq!(line["result"], "ok");
}

#[test]
fn continues_a_partial_download() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("m.gguf.part");
    std::fs::write(&to, &CONTENTS[..100_000]).unwrap();
    helper()
        .fetch(
            &request(
                format!("{}/moved", server.base),
                &to,
                &dir.path().join("log"),
            ),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), CONTENTS);
    assert_eq!(
        server.ranges(),
        [Some("bytes=100000-".into()), Some("bytes=100000-".into())]
    );
}

#[test]
fn deletes_a_file_with_the_wrong_checksum() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("m.gguf.part");
    let mut wrong = request(
        format!("{}/model", server.base),
        &to,
        &dir.path().join("log"),
    );
    wrong.sha256 = "0".repeat(64);
    let error = helper().fetch(&wrong, &mut |_, _| {}).unwrap_err();
    assert!(
        error.to_string().contains("isn't the one expected"),
        "{error}"
    );
    assert!(!to.exists());
}

#[test]
fn refuses_hosts_it_was_not_given() {
    let server = Server::start();
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("m.gguf.part");
    let log = dir.path().join("log");

    let redirected = helper()
        .fetch(
            &request(format!("{}/away", server.base), &to, &log),
            &mut |_, _| {},
        )
        .unwrap_err();
    assert!(
        redirected
            .to_string()
            .contains("example.com isn't an allowed host"),
        "{redirected}"
    );

    let mut elsewhere = request(format!("{}/model", server.base), &to, &log);
    elsewhere.hosts = vec!["huggingface.co".into()];
    let error = helper().fetch(&elsewhere, &mut |_, _| {}).unwrap_err();
    assert!(
        error.to_string().contains("isn't an allowed host"),
        "{error}"
    );
    assert_eq!(server.ranges().len(), 1, "only the redirect was requested");
}

#[test]
fn refuses_plain_http_to_other_machines() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("m.gguf.part");
    let mut insecure = request(
        "http://example.com/model".into(),
        &to,
        &dir.path().join("log"),
    );
    insecure.hosts = vec!["example.com".into()];
    let error = helper().fetch(&insecure, &mut |_, _| {}).unwrap_err();
    assert!(matches!(error, ModelError::Fetch(_)));
    assert!(error.to_string().contains("only https"), "{error}");
}
