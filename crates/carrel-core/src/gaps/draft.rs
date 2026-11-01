//! A GitHub issue draft reporting a gap to development: a title and a body with the gap, the
//! version and the counts, and any words the owner adds, as a prefilled new-issue address on
//! Horizon-Labs/carrel. Carrel never sends it: the owner opens the address in their browser,
//! reads and edits the draft, and submits it themself, or doesn't.

use std::fmt::Write as _;

use super::{Gap, GapKind};

/// Where new issues for Carrel are opened.
pub const NEW_ISSUE: &str = "https://github.com/Horizon-Labs/carrel/issues/new";

/// An issue draft for one gap.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssueDraft {
    /// The issue's title.
    pub title: String,
    /// Its body, in Markdown.
    pub body: String,
}

impl IssueDraft {
    /// The draft for `gap`, from Carrel `version` on `os`, with the owner's `added` words, if
    /// any. Nothing else goes in: the gap holds no titles, text, paths or file names.
    #[must_use]
    pub fn of(gap: &Gap, version: &str, os: &str, added: Option<&str>) -> Self {
        let title = match (gap.kind, gap.file_type().is_empty()) {
            (GapKind::OwnerReported, true) => "Missing feature".to_owned(),
            (GapKind::OwnerReported, false) => {
                format!("Missing feature for {} files", gap.file_type())
            }
            (kind, true) => capitalized(kind.describe()),
            (kind, false) => format!("{}: {}", capitalized(kind.describe()), gap.file_type()),
        };
        let mut body = String::from(
            "Reported from Carrel's gap log (`carrel report gaps`). It holds no titles, text, paths or file names.\n\n",
        );
        let row = |body: &mut String, name: &str, value: &str| {
            if !value.is_empty() {
                writeln!(body, "- **{name}:** {value}").unwrap();
            }
        };
        row(&mut body, "Gap", gap.kind.describe());
        row(&mut body, "Kind", gap.kind.name());
        row(&mut body, "Media type", &gap.media_type);
        let extension = if gap.extension.is_empty() {
            String::new()
        } else {
            format!(".{}", gap.extension)
        };
        row(&mut body, "Extension", &extension);
        row(&mut body, "Provider or stage", &gap.stage);
        row(&mut body, "Items affected", &gap.items.to_string());
        row(
            &mut body,
            "Seen",
            &format!("{} to {}", gap.first_seen, gap.last_seen),
        );
        row(
            &mut body,
            "Carrel version",
            &if gap.first_version == gap.last_version {
                gap.last_version.clone()
            } else {
                format!("{} to {}", gap.first_version, gap.last_version)
            },
        );
        row(&mut body, "Running", &format!("Carrel {version} on {os}"));
        if let Some(words) = gap.words.as_deref().filter(|w| !w.is_empty()) {
            write!(body, "\n### What Carrel should do\n\n{words}\n").unwrap();
        }
        if let Some(added) = added.map(str::trim).filter(|a| !a.is_empty()) {
            write!(body, "\n### More from the owner\n\n{added}\n").unwrap();
        }
        Self { title, body }
    }

    /// The prefilled new-issue address for the draft.
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "{NEW_ISSUE}?title={}&body={}",
            encode(&self.title),
            encode(&self.body)
        )
    }
}

/// `text` with its first letter in capitals.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `text` percent-encoded for a query value: everything but letters, digits and `-._~`.
#[must_use]
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

/// The program and arguments that open `url` in the browser on `os` (as
/// [`std::env::consts::OS`] names it): `open` on macOS, `start` on Windows, `xdg-open`
/// elsewhere.
#[must_use]
pub fn opener(os: &str, url: &str) -> (String, Vec<String>) {
    match os {
        "macos" => ("open".into(), vec![url.into()]),
        // The address has no spaces or quotes, so `start` doesn't take it for a window's
        // title; `&` would end the command.
        "windows" => (
            "cmd".into(),
            vec!["/C".into(), "start".into(), url.replace('&', "^&")],
        ),
        _ => ("xdg-open".into(), vec![url.into()]),
    }
}

/// Opens `url` in this device's browser, without waiting for it.
///
/// # Errors
///
/// An error starting the opener, such as `xdg-open` not being installed.
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    let (program, args) = opener(std::env::consts::OS, url);
    let mut child = std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    // Waited for elsewhere, so it doesn't linger once it's done.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{EXTRACTION, GapLog, Sighting};
    use super::*;

    fn gap() -> Gap {
        let mut log = GapLog::default();
        let n = log.note(
            &Sighting::new(GapKind::UnreadFormat, "model/3mf", "3mf", EXTRACTION),
            7,
            "2026-10-30",
            "0.6.0",
        );
        log.get(n).unwrap().clone()
    }

    #[test]
    fn the_draft_has_the_gap_version_and_counts() {
        let draft = IssueDraft::of(&gap(), "0.6.1", "macos", None);
        assert_eq!(draft.title, "No reader for the file type: .3mf (model/3mf)");
        for part in [
            "model/3mf",
            ".3mf",
            "text extraction",
            "**Items affected:** 7",
            "**Carrel version:** 0.6.0",
            "Carrel 0.6.1 on macos",
        ] {
            assert!(draft.body.contains(part), "{part} in {}", draft.body);
        }
        assert!(!draft.body.contains("More from the owner"));
        let added = IssueDraft::of(&gap(), "0.6.1", "macos", Some("Bambu files"));
        assert!(
            added
                .body
                .ends_with("### More from the owner\n\nBambu files\n")
        );
    }

    #[test]
    fn the_address_is_a_prefilled_new_issue() {
        let draft = IssueDraft {
            title: "A & b".into(),
            body: "x=1\n#2 é".into(),
        };
        assert_eq!(
            draft.url(),
            "https://github.com/Horizon-Labs/carrel/issues/new?title=A%20%26%20b&body=x%3D1%0A%232%20%C3%A9"
        );
    }

    #[test]
    fn each_system_opens_it_its_own_way() {
        let url = "https://x/?a=1&b=2";
        assert_eq!(opener("macos", url), ("open".into(), vec![url.into()]));
        assert_eq!(opener("linux", url).0, "xdg-open");
        let (program, args) = opener("windows", url);
        assert_eq!(program, "cmd");
        assert_eq!(args[1], "start");
        assert_eq!(args[2], "https://x/?a=1^&b=2");
    }
}
