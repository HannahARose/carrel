//! A complete Carrel provider (`docs/providers.md`), as an example for provider authors and a
//! stand-in for real providers in Carrel's tests.
//!
//! It offers a text model, `echo`, that answers with the prompt, and a plain-text extractor,
//! `echo-text`. Install it as `carrel-provider-echo` to try it with `carrel providers`.
//!
//! For tests, a copy named `carrel-provider-echo-<mode>` behaves differently:
//! `unavailable` (the model is unavailable), `fail` (requests fail), `exit` (it exits when
//! asked anything after `initialize`), `garbage` (it answers with something that isn't JSON),
//! `silent` (it answers only `initialize`), `old` (it speaks another protocol version) and
//! `card` (it answers every request for structured output with the same card draft).

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let program = std::env::current_exe().unwrap_or_default();
    let stem = program
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let mode = stem
        .strip_prefix("carrel-provider-echo-")
        .unwrap_or_default()
        .to_owned();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            eprintln!("echo: ignoring a line that isn't JSON");
            continue;
        };
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default();
        eprintln!("echo: {method}");
        if method != "initialize" && mode == "exit" {
            return;
        }
        if mode == "silent" && method != "initialize" {
            continue;
        }
        if mode == "garbage" && method != "initialize" {
            writeln!(stdout, "this isn't JSON").unwrap();
            stdout.flush().unwrap();
            continue;
        }
        let reply = match answer(method, &request["params"], &mode) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        writeln!(stdout, "{reply}").unwrap();
        stdout.flush().unwrap();
    }
}

fn answer(method: &str, params: &Value, mode: &str) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocol": u32::from(mode != "old"),
            "name": "Echo",
            "version": "1.0",
            "offers": [
                {
                    "capability": "text-model", "id": "echo", "label": "Echo model",
                    "priority": 1, "available": mode != "unavailable",
                    "reason": if mode == "unavailable" { Value::from("switched off") } else { Value::Null },
                    "structured_output": true
                },
                {
                    "capability": "extract-text", "id": "echo-text", "priority": 1,
                    "available": true, "media_types": ["text/plain"]
                }
            ]
        })),
        _ if mode == "fail" => Err((3, "echo was asked to fail".into())),
        "text/complete" => {
            let prompt = params["prompt"].as_str().unwrap_or_default();
            let text = format!("echo: {prompt}");
            let json = params.get("schema").filter(|s| !s.is_null()).map(|_| {
                if mode == "card" {
                    json!({
                        "title": "Echoed card",
                        "summary": "A card the echo provider drafted.",
                        "tags": ["draft"]
                    })
                } else {
                    json!({ "echo": prompt })
                }
            });
            Ok(
                json!({"text": text, "json": json, "model": "echo/1", "usage": {"input_tokens": 1, "output_tokens": 1}}),
            )
        }
        "text/extract" => {
            let path = params["path"].as_str().unwrap_or_default();
            std::fs::read_to_string(path)
                .map(|text| json!({"text": text, "method": "embedded", "pages": 1}))
                .map_err(|e| (3, format!("{path}: {e}")))
        }
        "shutdown" => Ok(Value::Null),
        _ => Err((-32601, format!("unknown method {method}"))),
    }
}
