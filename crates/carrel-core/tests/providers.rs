//! Providers and the provider protocol (`docs/providers.md`), using the example provider
//! `examples/echo_provider.rs` as a real separate program.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carrel_core::providers::process::{Found, ProcessProvider, discover};
use carrel_core::providers::{
    Capability, CompletionRequest, ExtractionRequest, PlainText, Provider, ProviderError, Registry,
    priority,
};

/// The built example provider, building it first if this test run didn't.
fn echo_program() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().unwrap().parent().unwrap();
    let path = profile_dir
        .join("examples")
        .join(format!("echo_provider{}", std::env::consts::EXE_SUFFIX));
    if !path.exists() {
        let status = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "--quiet",
                "-p",
                "carrel-core",
                "--example",
                "echo_provider",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "couldn't build the example provider");
    }
    path
}

/// A copy of the example provider named `carrel-provider-<name>` in `dir`. Names after
/// `echo-` select the example's test modes.
fn install(dir: &Path, name: &str) -> Found {
    let path = dir.join(format!(
        "carrel-provider-{name}{}",
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(echo_program(), &path).unwrap();
    Found {
        name: name.to_owned(),
        path,
    }
}

fn start(dir: &Path, name: &str) -> ProcessProvider {
    ProcessProvider::start(&install(dir, name)).unwrap()
}

#[test]
fn providers_are_found_by_program_name_in_folder_order() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    install(first.path(), "echo");
    install(second.path(), "echo");
    install(second.path(), "other");
    std::fs::write(
        second.path().join("carrel-provider-notes.txt"),
        "not a program",
    )
    .unwrap();
    std::fs::write(second.path().join("unrelated"), "").unwrap();
    #[cfg(unix)]
    std::fs::write(
        second.path().join("carrel-provider-plain-file"),
        "not executable",
    )
    .unwrap();

    let found = discover(&[
        first.path().to_owned(),
        second.path().to_owned(),
        PathBuf::from("/nowhere"),
    ]);
    let names: Vec<_> = found.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["echo", "other"]);
    assert!(
        found[0].path.starts_with(first.path()),
        "the first folder wins"
    );
}

#[test]
fn a_provider_reports_its_offers_and_answers_requests() {
    let dir = tempfile::tempdir().unwrap();
    let echo = start(dir.path(), "echo");
    assert_eq!(echo.name(), "echo");
    assert_eq!(echo.label(), Some("Echo"));
    assert_eq!(echo.version(), Some("1.0"));
    let offers: Vec<_> = echo
        .offers()
        .iter()
        .map(|o| (o.capability, o.id.as_str()))
        .collect();
    assert_eq!(
        offers,
        [
            (Capability::TextModel, "echo"),
            (Capability::ExtractText, "echo-text")
        ]
    );
    assert_eq!(
        echo.offers()[0].extra.get("structured_output"),
        Some(&serde_json::Value::Bool(true)),
        "unknown fields are kept"
    );

    let plain = echo
        .complete(
            "echo",
            &CompletionRequest {
                prompt: "hello".into(),
                ..CompletionRequest::default()
            },
        )
        .unwrap();
    assert_eq!(plain.text, "echo: hello");
    assert_eq!(plain.model, "echo/1");
    assert_eq!(plain.json, None);

    let structured = echo
        .complete(
            "echo",
            &CompletionRequest {
                prompt: "hi".into(),
                schema: Some(serde_json::json!({"type": "object"})),
                ..CompletionRequest::default()
            },
        )
        .unwrap();
    assert_eq!(structured.json, Some(serde_json::json!({"echo": "hi"})));

    let file = dir.path().join("note.txt");
    std::fs::write(&file, "Some notes.\n").unwrap();
    let extracted = echo
        .extract(
            "echo-text",
            &ExtractionRequest {
                path: file,
                media_type: "text/plain".into(),
            },
        )
        .unwrap();
    assert_eq!(extracted.text, "Some notes.\n");
    assert_eq!(extracted.method, "embedded");
}

#[test]
fn the_registry_prefers_higher_priority_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(start(dir.path(), "echo")));

    let best = registry.best(Capability::ExtractText).unwrap();
    assert_eq!(
        (best.provider.as_str(), best.offer.as_str()),
        ("built-in", "plain-text")
    );
    assert_eq!(best.priority, priority::BUILT_IN);

    registry.prefer(Capability::ExtractText, "echo", "echo-text");
    let best = registry.best(Capability::ExtractText).unwrap();
    assert_eq!(best.provider, "echo", "a choice in settings beats priority");

    let file = dir.path().join("a.md");
    std::fs::write(&file, "# Title\n").unwrap();
    let (extracted, source, attempts) = registry
        .extract(&ExtractionRequest {
            path: file,
            media_type: "text/markdown".into(),
        })
        .unwrap();
    assert_eq!(extracted.text, "# Title\n");
    assert_eq!(
        source.provider, "built-in",
        "echo doesn't read Markdown, so it isn't tried"
    );
    assert_eq!(attempts, []);
}

#[test]
fn failures_fall_back_to_the_next_offer() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    registry.add(Box::new(start(dir.path(), "echo")));
    registry.add(Box::new(start(dir.path(), "echo-fail")));
    registry.add(Box::new(start(dir.path(), "echo-unavailable")));

    let ranked: Vec<_> = registry
        .ranked(Capability::TextModel)
        .iter()
        .map(|(p, _)| p.name().to_owned())
        .collect();
    assert_eq!(
        ranked,
        ["echo-fail", "echo"],
        "unavailable offers aren't ranked; later ones win ties"
    );

    let (answer, source, attempts) = registry
        .complete(&CompletionRequest {
            prompt: "x".into(),
            ..CompletionRequest::default()
        })
        .unwrap();
    assert_eq!(answer.text, "echo: x");
    assert_eq!(source.provider, "echo");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].source.provider, "echo-fail");
    assert_eq!(
        attempts[0].error,
        ProviderError::Failed("echo was asked to fail".into())
    );

    let unavailable = registry
        .providers()
        .find(|p| p.name() == "echo-unavailable")
        .unwrap();
    assert_eq!(
        unavailable.offers()[0].reason.as_deref(),
        Some("switched off")
    );
}

#[test]
fn with_no_provider_the_registry_says_so() {
    let registry = Registry::with_built_ins();
    assert_eq!(registry.best(Capability::TextModel), None);
    assert_eq!(
        registry
            .complete(&CompletionRequest::default())
            .unwrap_err(),
        ProviderError::NoProvider(Capability::TextModel)
    );
}

#[test]
fn misbehaving_providers_are_reported_with_their_log() {
    let dir = tempfile::tempdir().unwrap();
    let request = CompletionRequest {
        prompt: "x".into(),
        ..CompletionRequest::default()
    };

    let exits = start(dir.path(), "echo-exit");
    let ProviderError::Protocol(why) = exits.complete("echo", &request).unwrap_err() else {
        panic!("expected a protocol error");
    };
    assert!(
        why.contains("exited before answering text/complete"),
        "{why}"
    );
    assert!(
        why.contains("echo: text/complete"),
        "includes the provider's log: {why}"
    );
    assert!(
        matches!(
            exits.complete("echo", &request),
            Err(ProviderError::Protocol(_))
        ),
        "a broken provider stays broken"
    );

    let garbage = start(dir.path(), "echo-garbage");
    let ProviderError::Protocol(why) = garbage.complete("echo", &request).unwrap_err() else {
        panic!("expected a protocol error");
    };
    assert!(why.contains("isn't JSON"), "{why}");

    let mut silent = start(dir.path(), "echo-silent");
    silent.set_request_timeout(Duration::from_millis(300));
    let ProviderError::Protocol(why) = silent.complete("echo", &request).unwrap_err() else {
        panic!("expected a protocol error");
    };
    assert!(why.contains("no answer to text/complete"), "{why}");

    let ProviderError::Protocol(why) =
        ProcessProvider::start(&install(dir.path(), "echo-old")).unwrap_err()
    else {
        panic!("expected a protocol error");
    };
    assert!(why.contains("speaks provider protocol 0"), "{why}");

    let missing = Found {
        name: "missing".into(),
        path: dir.path().join("carrel-provider-missing"),
    };
    assert!(matches!(
        ProcessProvider::start(&missing),
        Err(ProviderError::Protocol(_))
    ));
}

#[test]
fn the_built_in_extractor_reads_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"\xc3\xa9t\xc3\xa9 \xff\n").unwrap();
    let plain = PlainText::new();
    assert!(plain.offers()[0].reads("TEXT/PLAIN"));
    assert!(!plain.offers()[0].reads("application/pdf"));
    let text = plain
        .extract(
            "plain-text",
            &ExtractionRequest {
                path: file,
                media_type: "text/plain".into(),
            },
        )
        .unwrap();
    assert_eq!(
        text.text, "été \u{fffd}\n",
        "invalid UTF-8 is replaced, not fatal"
    );
}

#[test]
fn serve_offers_a_rust_provider_over_the_protocol() {
    use carrel_core::providers::serve::{About, serve};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "Hello.\n").unwrap();
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":1}}"#.to_owned(),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "text/extract", "params": {
            "offer": "plain-text", "path": file, "media_type": "text/plain"}})
        .to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"text/complete","params":{"offer":"x","prompt":"hi"}}"#
            .to_owned(),
        r#"{"jsonrpc":"2.0","id":4,"method":"text/extract","params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":5,"method":"bogus"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"initialize"}"#.to_owned(),
        "not json".to_owned(),
        r#"{"jsonrpc":"2.0","id":6,"method":"shutdown"}"#.to_owned(),
    ];
    let input = requests.join("\n") + "\n";
    let mut output = Vec::new();
    let about = About {
        label: "Test",
        version: "1",
    };
    serve(&PlainText::new(), about, input.as_bytes(), &mut output).unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        replies.len(),
        6,
        "no replies to the notification or non-JSON"
    );
    assert_eq!(replies[0]["result"]["protocol"], 1);
    assert_eq!(replies[0]["result"]["name"], "Test");
    assert_eq!(replies[0]["result"]["offers"][0]["id"], "plain-text");
    assert_eq!(replies[1]["result"]["text"], "Hello.\n");
    assert_eq!(
        replies[2]["error"]["code"], 2,
        "the built-in has no text models"
    );
    assert_eq!(replies[3]["error"]["code"], -32602);
    assert_eq!(replies[4]["error"]["code"], -32601);
    assert_eq!(replies[5]["id"], 6);
    assert_eq!(replies[5]["result"], serde_json::Value::Null);
}

#[test]
fn providers_without_token_counting_fall_back_to_an_estimate() {
    use carrel_core::providers::{Source, estimate_tokens};

    let dir = tempfile::tempdir().unwrap();
    // The example provider predates text/count-tokens, so it answers "unknown method".
    let echo = start(dir.path(), "echo");
    assert!(matches!(
        echo.count_tokens("echo", "some text"),
        Err(ProviderError::Unsupported(_))
    ));
    let still_works = echo
        .complete(
            "echo",
            &CompletionRequest {
                prompt: "hi".into(),
                ..CompletionRequest::default()
            },
        )
        .unwrap();
    assert_eq!(
        still_works.text, "echo: hi",
        "an unknown method doesn't break the provider"
    );

    let mut registry = Registry::new();
    registry.add(Box::new(echo));
    let source = Source {
        provider: "echo".into(),
        offer: "echo".into(),
        priority: 1,
    };
    let text = "The oscillators drift in and out of phase.";
    assert_eq!(
        registry.count_tokens(&source, text),
        (estimate_tokens(text), false)
    );
}

#[test]
fn the_estimate_errs_high() {
    use carrel_core::providers::estimate_tokens;
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("a"), 1);
    // English averages about four bytes a token, so twelve bytes estimate four tokens, not three.
    assert_eq!(estimate_tokens("twelve bytes"), 4);
    // CJK characters are three bytes each and about a token each.
    assert_eq!(estimate_tokens("日本語"), 3);
}
