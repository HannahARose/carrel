//! Stage 5: an item's text for the models, fitted to a budget (`docs/design/ingest.md`).

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use crate::card::{DateOrTime, Kind};
use crate::detect::{Rules, is_ignored};
use crate::providers::{ExtractionRequest, ProviderError, Registry};
use crate::review::Attempt;

use super::Candidate;

/// How much of a README or project file goes into a folder's description, in bytes.
const FILE_EXCERPT: usize = 4096;

/// How many entries of a folder's listing are shown.
const LISTING_ENTRIES: usize = 200;

/// Project files that say what a bundle is, in the order they're shown.
const PROJECT_FILES: [&str; 9] = [
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "go.mod",
    "CMakeLists.txt",
    "Makefile",
    "Package.swift",
    "DESCRIPTION",
];

/// The text read for an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemText {
    /// The text, before any trimming to a budget.
    pub text: String,
    /// The item's media type, for files.
    pub media_type: Option<String>,
    /// How it was read: `embedded`, `ocr`, `listing` (a folder's description) or `none`.
    pub method: String,
    /// Pages, when an extractor reported them.
    pub pages: Option<u32>,
    /// When it was made, when the extractor read it from the file, such as a photo's date
    /// taken.
    pub created: Option<DateOrTime>,
    /// Whether the file records where it was made, such as a photo's location.
    pub located: bool,
    /// Every extractor tried.
    pub attempts: Vec<Attempt>,
    /// Why there's no text, when there isn't.
    pub note: Option<String>,
}

/// The media type of the file at `path`: from its first bytes when they're distinctive,
/// otherwise from its extension, and `text/plain` for anything that reads as UTF-8 text.
#[must_use]
pub fn media_type(path: &Path) -> String {
    let mut head = [0u8; 512];
    let read = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    let head = &head[..read];
    let by_extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .and_then(|e| from_extension(&e));
    if let Some(sniffed) = sniff(head) {
        // A ZIP is often a document format with its own type, which the extension says.
        if sniffed == "application/zip" && by_extension.is_some() {
            return by_extension.unwrap_or_default().to_owned();
        }
        return sniffed.to_owned();
    }
    if let Some(known) = by_extension {
        return known.to_owned();
    }
    if !head.is_empty() && !head.contains(&0) && std::str::from_utf8(head).is_ok() {
        return "text/plain".to_owned();
    }
    "application/octet-stream".to_owned()
}

fn sniff(head: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| head.starts_with(magic);
    if starts(b"%PDF-") {
        Some("application/pdf")
    } else if starts(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if starts(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if starts(b"II*\0") || starts(b"MM\0*") {
        Some("image/tiff")
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" {
        match &head[8..12] {
            b"heic" | b"heix" | b"heim" | b"heis" => Some("image/heic"),
            b"mif1" | b"msf1" => Some("image/heif"),
            _ => None,
        }
    } else if starts(b"RIFF") && head.len() >= 12 && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if starts(b"PK\x03\x04") {
        Some("application/zip")
    } else {
        None
    }
}

fn from_extension(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "pdf" => "application/pdf",
        "txt" | "text" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "tex" => "text/x-tex",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        _ => return None,
    })
}

/// Reads an item's text: a file through the registry's extractors, a folder or bundle as a
/// description built from its README, project files and listing.
#[must_use]
pub fn read(registry: &Registry, rules: &Rules, candidate: &Candidate) -> ItemText {
    if candidate.kind == Kind::File {
        read_file(registry, &candidate.path)
    } else {
        ItemText {
            text: describe_folder(registry, rules, &candidate.path),
            media_type: None,
            method: "listing".into(),
            pages: None,
            created: None,
            located: false,
            attempts: Vec::new(),
            note: None,
        }
    }
}

fn read_file(registry: &Registry, path: &Path) -> ItemText {
    let media_type = media_type(path);
    let request = ExtractionRequest {
        path: path.to_owned(),
        media_type: media_type.clone(),
    };
    let result_name = |e: &ProviderError| e.result();
    let to_attempt = |a: &crate::providers::Attempt| Attempt {
        stage: "text".into(),
        provider: a.source.provider.clone(),
        offer: a.source.offer.clone(),
        result: result_name(&a.error).into(),
        message: Some(a.error.to_string()),
    };
    match registry.extract(&request) {
        Ok((extraction, source, failed)) => {
            let mut attempts: Vec<Attempt> = failed.iter().map(to_attempt).collect();
            attempts.push(Attempt {
                stage: "text".into(),
                provider: source.provider,
                offer: source.offer,
                result: "ok".into(),
                message: None,
            });
            ItemText {
                text: extraction.text,
                media_type: Some(media_type),
                method: extraction.method,
                pages: extraction.pages,
                created: extraction
                    .created
                    .map(DateOrTime)
                    .filter(DateOrTime::is_valid),
                located: extraction.located,
                attempts,
                note: None,
            }
        }
        Err(error) => ItemText {
            text: String::new(),
            media_type: Some(media_type.clone()),
            method: "none".into(),
            pages: None,
            created: None,
            located: false,
            attempts: Vec::new(),
            note: Some(match error {
                ProviderError::NoProvider(_) => {
                    format!("nothing on this device reads {media_type}")
                }
                other => other.to_string(),
            }),
        },
    }
}

/// A folder described for a model: its README, project files, and a listing of what's in it.
fn describe_folder(registry: &Registry, rules: &Rules, folder: &Path) -> String {
    let mut text = String::new();
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    writeln!(text, "Folder: {name}").unwrap();

    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    if let Some(readme) = entries.iter().find(|p| {
        p.is_file()
            && p.file_name().is_some_and(|n| {
                n.to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("readme")
            })
    }) {
        let readme_text = read_file(registry, readme).text;
        writeln!(text, "\nREADME:\n{}", excerpt(&readme_text, FILE_EXCERPT)).unwrap();
    }
    for project in PROJECT_FILES {
        let path = folder.join(project);
        if path.is_file()
            && let Ok(contents) = std::fs::read_to_string(&path)
        {
            writeln!(text, "\n{project}:\n{}", excerpt(&contents, FILE_EXCERPT)).unwrap();
        }
    }

    let mut listing = Vec::new();
    list(folder, rules, 0, &mut listing);
    let total = listing.len();
    writeln!(text, "\nContents:").unwrap();
    for line in listing.iter().take(LISTING_ENTRIES) {
        writeln!(text, "{line}").unwrap();
    }
    if total > LISTING_ENTRIES {
        writeln!(text, "… and {} more", total - LISTING_ENTRIES).unwrap();
    }
    text
}

/// Two levels of a folder, indented, with regenerable folders named but not entered.
fn list(dir: &Path, rules: &Rules, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) || name.starts_with('.') {
            continue;
        }
        let indent = "  ".repeat(depth);
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if !is_dir {
            out.push(format!("{indent}{name}"));
        } else if rules.is_regenerable(&name) {
            out.push(format!("{indent}{name}/ (regenerable)"));
        } else {
            out.push(format!("{indent}{name}/"));
            if depth < 1 {
                list(&entry.path(), rules, depth + 1, out);
            }
        }
    }
}

fn excerpt(text: &str, bytes: usize) -> &str {
    &text[..text.floor_char_boundary(bytes)]
}

/// Headings that start back matter a model doesn't need.
const BACK_MATTER: [&str; 9] = [
    "references",
    "bibliography",
    "works cited",
    "literature cited",
    "acknowledgments",
    "acknowledgements",
    "acknowledgment",
    "acknowledgement",
    "funding",
];

fn heading_words(line: &str) -> String {
    line.trim()
        .trim_start_matches(['#', '*'])
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c.is_whitespace())
        .trim_end_matches([':', '*'])
        .trim()
        .to_lowercase()
}

/// Removes references, acknowledgments and similar back matter, from such a heading to the
/// next appendix heading or the end.
#[must_use]
pub fn drop_back_matter(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dropping = false;
    for line in text.split_inclusive('\n') {
        let words = heading_words(line);
        let short = line.trim().len() <= 40;
        if short && BACK_MATTER.contains(&words.as_str()) {
            dropping = true;
            continue;
        }
        if dropping
            && short
            && (words.starts_with("appendix") || words.starts_with("supplementary"))
        {
            dropping = false;
        }
        if !dropping {
            out.push_str(line);
        }
    }
    out
}

/// The mark left where text was cut.
pub const CUT: &str = "\n\n[…]\n\n";

/// `text` within `budget` tokens as `count` measures them. Back matter goes first; if that
/// isn't enough, the start and the end are kept (seven tenths and three tenths), each cut at a
/// sentence boundary, with [`CUT`] between. Returns the text and whether it was shortened.
pub fn fit(text: &str, budget: u64, count: &dyn Fn(&str) -> u64) -> (String, bool) {
    if count(text) <= budget {
        return (text.to_owned(), false);
    }
    let text = drop_back_matter(text);
    let tokens = count(&text);
    if tokens <= budget {
        return (text, true);
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let mut target = (text.len() as f64 * budget as f64 / tokens.max(1) as f64 * 0.95) as usize;
    for _ in 0..4 {
        let head_len = target * 7 / 10;
        let tail_len = target.saturating_sub(head_len).saturating_sub(CUT.len());
        let head = sentence_end(&text, head_len);
        let tail = sentence_start(&text, text.len().saturating_sub(tail_len));
        let fitted = format!("{}{CUT}{}", head.trim_end(), tail.trim_start());
        if count(&fitted) <= budget {
            return (fitted, true);
        }
        target = target * 8 / 10;
    }
    let head = sentence_end(&text, target);
    (head.to_owned(), true)
}

/// The text up to `at`, ending at the last sentence or paragraph end in the final fifth, or
/// the last space, or `at` itself.
fn sentence_end(text: &str, at: usize) -> &str {
    let at = text.floor_char_boundary(at.min(text.len()));
    let window = &text[..at];
    let floor = window.floor_char_boundary(at * 4 / 5);
    let found = ["\n\n", ". ", "? ", "! ", ".\n"]
        .iter()
        .filter_map(|mark| window[floor..].rfind(mark).map(|i| floor + i + mark.len()))
        .max();
    match found.or_else(|| window.rfind(' ')) {
        Some(end) => &text[..end],
        None => window,
    }
}

/// The text from the first sentence start at or after `from`.
fn sentence_start(text: &str, from: usize) -> &str {
    let from = text.ceil_char_boundary(from.min(text.len()));
    let window = &text[from..];
    let found = ["\n\n", ". ", "? ", "! ", ".\n"]
        .iter()
        .filter_map(|mark| window.find(mark).map(|i| from + i + mark.len()))
        .min();
    match found {
        Some(start) if start - from < window.len() / 2 => &text[start..],
        _ => window,
    }
}
