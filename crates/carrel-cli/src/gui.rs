//! `carrel gui`: opens the Mac app on the archive commands would use, through the
//! `carrel://open` link the app handles, or on its Welcome window when there's no archive.

use std::path::Path;

use serde_json::json;

use crate::commands::{find_item, locate};
use crate::start_screen::StartScreen;
use crate::{Failure, Report};

/// The app's bundle identifier: the release build's, which the Debug build doesn't share.
const APP_BUNDLE_ID: &str = "io.github.hannaharose.Carrel";

/// Opens the app, on `screen` and with `item` chosen when they're given.
pub fn open_app(
    archive: Option<&Path>,
    screen: Option<StartScreen>,
    item: Option<&str>,
) -> Result<Report, Failure> {
    // Checked first, so elsewhere it fails the same way whatever the archive.
    open_args(std::env::consts::OS, None)?;
    let link = gui_link(archive, screen, item)?;
    let args = open_args(std::env::consts::OS, link.as_deref())?;
    let status = std::process::Command::new("open")
        .args(&args)
        .status()
        .map_err(|e| Failure::new(format!("the app couldn't be opened: {e}")))?;
    if !status.success() {
        return Err(Failure::new(
            "the app couldn't be opened; is Carrel in Applications?",
        ));
    }
    let text = if link.is_some() {
        "Opened the archive in Carrel"
    } else {
        "Opened Carrel"
    };
    Ok(Report::new(format!("{text}\n"), json!({ "link": link })))
}

/// The link for the archive commands would use, or none when there isn't one, so the app
/// opens on its Welcome window. An archive named with `--archive` has to open, and so does
/// one when an item is asked for.
fn gui_link(
    archive: Option<&Path>,
    screen: Option<StartScreen>,
    item: Option<&str>,
) -> Result<Option<String>, Failure> {
    let found = match locate(archive) {
        Ok(found) => found,
        Err(_) if archive.is_none() && item.is_none() => return Ok(None),
        Err(e) => return Err(e),
    };
    let item = match item {
        Some(query) => Some(find_item(&found, &found.items()?, query)?.id.to_string()),
        None => None,
    };
    let root = std::fs::canonicalize(found.root()).map_err(|e| Failure::io(found.root(), &e))?;
    Ok(Some(open_link(&root, screen, item.as_deref())))
}

/// `carrel://open?archive=<root>&item=<id>&screen=<name>`, as `AgentLink` in CarrelKit reads it.
pub(crate) fn open_link(root: &Path, screen: Option<StartScreen>, item: Option<&str>) -> String {
    let mut link = format!(
        "carrel://open?archive={}",
        percent_encode_query(&root.to_string_lossy())
    );
    if let Some(item) = item {
        link.push_str("&item=");
        link.push_str(&percent_encode_query(item));
    }
    if let Some(screen) = screen {
        link.push_str("&screen=");
        link.push_str(&percent_encode_query(&screen.name()));
    }
    link
}

/// `open`'s arguments on `os`: the link for the app, or the app alone. The app is macOS's
/// only until v2.0, so anywhere else this fails, pointing to the interactive mode.
pub(crate) fn open_args(os: &str, link: Option<&str>) -> Result<Vec<String>, Failure> {
    if os != "macos" {
        return Err(Failure::new(
            "the Carrel app runs only on macOS for now; `carrel tui` opens the interactive mode",
        ));
    }
    let mut args = vec!["-b".to_owned(), APP_BUNDLE_ID.to_owned()];
    args.extend(link.map(str::to_owned));
    Ok(args)
}

/// `value` with everything but unreserved characters and `/` percent-encoded, for a query.
fn percent_encode_query(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{open_args, open_link};
    use crate::start_screen::StartScreen;

    #[test]
    fn link_encodes_the_archive_path() {
        let link = open_link(
            Path::new("/Users/me/Tax & Bills/Archive 2026"),
            Some(StartScreen::DiscoverWander),
            Some("0193b6a2-7c1e-7d3f-8a00-000000000001"),
        );
        assert_eq!(
            link,
            "carrel://open?archive=/Users/me/Tax%20%26%20Bills/Archive%202026\
             &item=0193b6a2-7c1e-7d3f-8a00-000000000001&screen=discover:wander"
        );
    }

    #[test]
    fn link_without_screen_or_item_names_only_the_archive() {
        assert_eq!(
            open_link(Path::new("/a/b+c=d?#%"), None, None),
            "carrel://open?archive=/a/b%2Bc%3Dd%3F%23%25"
        );
    }

    #[test]
    fn opens_the_release_app() {
        assert_eq!(
            open_args("macos", Some("carrel://open?archive=/a")).unwrap(),
            [
                "-b",
                "io.github.hannaharose.Carrel",
                "carrel://open?archive=/a"
            ]
        );
        assert_eq!(
            open_args("macos", None).unwrap(),
            ["-b", "io.github.hannaharose.Carrel"]
        );
    }

    #[test]
    fn fails_elsewhere_pointing_to_the_interactive_mode() {
        for os in ["linux", "windows"] {
            let error = open_args(os, None).unwrap_err().0;
            assert!(error.contains("only on macOS"), "{error}");
            assert!(error.contains("carrel tui"), "{error}");
        }
    }
}
