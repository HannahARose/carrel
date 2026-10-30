//! Offering a [`Provider`] to Carrel as a provider program: the other end of
//! [`process::ProcessProvider`](super::process::ProcessProvider) (`docs/providers.md`).

use std::io::{BufRead, Write};

use serde::Serialize;
use serde_json::{Value, json};

use super::{CompletionRequest, ExtractionRequest, PROTOCOL_VERSION, Provider, ProviderError};

/// Information `initialize` reports besides the offers.
#[derive(Debug, Clone, Copy)]
pub struct About<'a> {
    /// The provider's name for people, such as "llama.cpp".
    pub label: &'a str,
    /// Its version.
    pub version: &'a str,
}

/// Answers provider protocol requests from `input` on `output` until `input` ends, as a
/// provider program's `main` does with standard input and output.
///
/// # Errors
///
/// An I/O error reading `input` or writing `output`.
pub fn serve(
    provider: &dyn Provider,
    about: About<'_>,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if let Some(reply) = reply(provider, about, &line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The reply to one line, or `None` when none is due: a notification, or a line that isn't a
/// request.
#[must_use]
pub fn reply(provider: &dyn Provider, about: About<'_>, line: &str) -> Option<String> {
    let message: Value = serde_json::from_str(line).ok()?;
    let method = message.get("method")?.as_str()?;
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let reply = match answer(provider, about, method, params) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    };
    Some(reply.to_string())
}

fn answer(
    provider: &dyn Provider,
    about: About<'_>,
    method: &str,
    params: Value,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocol": PROTOCOL_VERSION,
            "name": about.label,
            "version": about.version,
            "offers": provider.offers(),
        })),
        "shutdown" => Ok(Value::Null),
        "text/complete" => {
            let (offer, request) = parse::<CompletionRequest>(params)?;
            to_result(provider.complete(&offer, &request))
        }
        "text/extract" => {
            let (offer, request) = parse::<ExtractionRequest>(params)?;
            to_result(provider.extract(&offer, &request))
        }
        "text/count-tokens" => {
            let (offer, counted) = parse::<CountRequest>(params)?;
            to_result(
                provider
                    .count_tokens(&offer, &counted.text)
                    .map(|tokens| json!({ "tokens": tokens })),
            )
        }
        _ => Err((-32601, format!("unknown method {method}"))),
    }
}

#[derive(serde::Deserialize)]
struct CountRequest {
    text: String,
}

/// The `offer` parameter and the rest of the parameters as a request.
fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<(String, T), (i64, String)> {
    let offer = params
        .get("offer")
        .and_then(Value::as_str)
        .ok_or((-32602, "missing offer".to_owned()))?
        .to_owned();
    let request = serde_json::from_value(params).map_err(|e| (-32602, e.to_string()))?;
    Ok((offer, request))
}

fn to_result<T: Serialize>(result: Result<T, ProviderError>) -> Result<Value, (i64, String)> {
    match result {
        Ok(value) => Ok(serde_json::to_value(value).expect("results always serialize")),
        Err(ProviderError::Unavailable(m)) => Err((1, m)),
        Err(ProviderError::Unsupported(m)) => Err((2, m)),
        Err(ProviderError::Declined(m)) => Err((4, m)),
        Err(ProviderError::Failed(m) | ProviderError::Protocol(m)) => Err((3, m)),
        Err(e @ ProviderError::NoProvider(_)) => Err((1, e.to_string())),
    }
}
