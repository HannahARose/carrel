//! Stage 5: an item's text for the models, fitted to a budget (`docs/design/ingest.md`).

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::card::{DateOrTime, Kind};
use crate::compressed::{self, EntryKind, Inside, Shape};
use crate::detect::{Compressed, Rules, is_ignored};
use crate::providers::{ExtractionRequest, Picture, ProviderError, Registry};
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
    /// For a file whose text couldn't be read, why: kept in this device's log of the types
    /// it can't read ([`crate::formats`]).
    pub unread: Option<Unread>,
}

/// Why a file's text couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unread {
    /// Nothing on this device reads its type.
    NoReader,
    /// Every reader that tried failed.
    Failed,
    /// It was read, but had no text.
    Empty,
}

impl Unread {
    /// In words: `no reader`, `reader failed`, `read but empty`.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::NoReader => "no reader",
            Self::Failed => "reader failed",
            Self::Empty => "read but empty",
        }
    }
}

/// The media type of the file at `path`: from its first bytes when they're distinctive,
/// otherwise from its extension, and `text/plain` for anything that reads as UTF-8 text. The
/// extension comes before that rule, so an ASCII STL or a STEP file is a 3D model, not text.
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
        // A zip is often a document format with its own type, and a compressor often wraps a
        // tar, which the extension says.
        if Compressed::sniff(head).is_some() && by_extension.is_some() {
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
    } else {
        Compressed::sniff(head).map(Compressed::media_type)
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
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "epub" => "application/epub+zip",
        "jar" => "application/java-archive",
        "apk" => "application/vnd.android.package-archive",
        "ipa" => "application/x-ios-app",
        "whl" => "application/x-wheel+zip",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" | "tgz" => "application/gzip",
        "bz2" | "tbz2" | "tbz" => "application/x-bzip2",
        "xz" | "txz" => "application/x-xz",
        "zst" | "tzst" => "application/zstd",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        // 3D models, and the CAD and slicer files around them (`docs/design/models-3d.md`).
        "stl" => "model/stl",
        "obj" => "model/obj",
        "ply" => "model/ply",
        "usdz" => "model/vnd.usdz+zip",
        "usd" | "usda" | "usdc" => "model/vnd.usd",
        "3mf" => "model/3mf",
        "step" | "stp" => "model/step",
        "igs" | "iges" => "model/iges",
        "gcode" | "gco" => "text/x-gcode",
        _ => return None,
    })
}

/// Reads an item's text: a file through the registry's extractors, a folder or bundle as a
/// description built from its README, project files and listing, and a compressed file from
/// what's inside, without expanding it: its one file's text, or a description like a
/// folder's.
#[must_use]
pub fn read(registry: &Registry, rules: &Rules, candidate: &Candidate) -> ItemText {
    if let Some(inside) = &candidate.inside {
        read_compressed(registry, rules, &candidate.path, inside)
    } else if candidate.kind == Kind::File {
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
            unread: None,
        }
    }
}

fn read_file(registry: &Registry, path: &Path) -> ItemText {
    let media_type = media_type(path);
    let request = ExtractionRequest {
        path: path.to_owned(),
        media_type: media_type.clone(),
        context: None,
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
            let mut text = extraction.text;
            for picture in &extraction.pictures {
                if let Some(shows) = describe_picture(registry, picture) {
                    write!(text, "\nShows: {shows}").unwrap();
                }
            }
            let unread = text.trim().is_empty().then_some(Unread::Empty);
            ItemText {
                text,
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
                unread,
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
            unread: Some(if matches!(error, ProviderError::NoProvider(_)) {
                Unread::NoReader
            } else {
                Unread::Failed
            }),
            note: Some(match error {
                ProviderError::NoProvider(_) => {
                    format!("nothing on this device reads {media_type}")
                }
                other => other.to_string(),
            }),
        },
    }
}

/// What the best image reader says a provider's picture shows, given its context and not the
/// file's name; then the picture is deleted. None when nothing here describes images, or the
/// picture isn't a file in the temporary folder, where providers write them.
fn describe_picture(registry: &Registry, picture: &Picture) -> Option<String> {
    let temporary = std::env::temp_dir().canonicalize().ok()?;
    let path = picture.path.canonicalize().ok()?;
    if !path.starts_with(&temporary) || !path.is_file() {
        return None;
    }
    let request = ExtractionRequest {
        media_type: media_type(&path),
        path: path.clone(),
        context: Some(picture.context.clone()),
    };
    let shows = registry.describe(&request).ok().and_then(|(read, _, _)| {
        read.text
            .lines()
            .find_map(|line| line.strip_prefix("Shows: "))
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    });
    let _ = std::fs::remove_file(&path);
    shows
}

/// The most of a compressed file's one file that's read for its text, in bytes.
const INNER_FILE_LIMIT: u64 = 64 << 20;

/// A compressed file's text: the text of the one file inside, or a description of what's
/// inside like a folder's. One that can't be read is described by its name, and noted as
/// unread, so it's logged with the other types this device can't read.
fn read_compressed(registry: &Registry, rules: &Rules, path: &Path, inside: &Inside) -> ItemText {
    let mut text = ItemText {
        text: String::new(),
        media_type: Some(inside.format.media_type().to_owned()),
        method: "listing".into(),
        pages: None,
        created: None,
        located: false,
        attempts: Vec::new(),
        note: None,
        unread: None,
    };
    let Some(listing) = &inside.listing else {
        text.method = "none".into();
        text.note.clone_from(&inside.unread);
        text.unread = Some(if inside.no_reader {
            Unread::NoReader
        } else {
            Unread::Failed
        });
        return text;
    };
    if let Shape::OneFile(name) = listing.shape()
        && let Some(entry) = listing
            .entries
            .iter()
            .find(|e| e.path.trim_start_matches("./") == name)
        && let Some(inner) = read_inner(registry, path, inside.format, &entry.path, &name)
    {
        return inner;
    }
    text.text = describe_listing(registry, rules, path, inside, listing);
    text
}

/// The text of one file inside, read by extracting it to a temporary folder.
fn read_inner(
    registry: &Registry,
    path: &Path,
    format: crate::detect::Compressed,
    inner: &str,
    name: &str,
) -> Option<ItemText> {
    let bytes = compressed::read_entry(path, format, inner, INNER_FILE_LIMIT).ok()?;
    let folder = std::env::temp_dir().join(format!("carrel-read-{}", crate::card::Id::new_v7()));
    std::fs::create_dir_all(&folder).ok()?;
    let file_name = Path::new(name).file_name()?;
    let file = folder.join(file_name);
    let text = std::fs::write(&file, bytes)
        .ok()
        .map(|()| read_file(registry, &file));
    let _ = std::fs::remove_dir_all(&folder);
    text.filter(|t| t.unread.is_none())
}

/// A compressed file described for a model, like a folder: its README, project files, and a
/// listing, all read from inside it.
fn describe_listing(
    registry: &Registry,
    rules: &Rules,
    path: &Path,
    inside: &Inside,
    listing: &compressed::Listing,
) -> String {
    let mut text = String::new();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    writeln!(
        text,
        "Compressed file: {name} ({}, holding {}, {} expanded)",
        inside.format.name(),
        inside.describe(),
        crate::human_size(listing.total_size())
    )
    .unwrap();
    let root = listing.root();
    let top = listing.top_level();
    if let Some(readme) = top
        .iter()
        .find(|n| n.to_ascii_lowercase().starts_with("readme"))
        .filter(|n| listing.find(n).is_some_and(|e| e.kind == EntryKind::File))
        && let Some(readme_text) = read_inner(
            registry,
            path,
            inside.format,
            &format!("{root}{readme}"),
            readme,
        )
    {
        writeln!(
            text,
            "\nREADME:\n{}",
            excerpt(&readme_text.text, FILE_EXCERPT)
        )
        .unwrap();
    }
    for project in PROJECT_FILES {
        if top.iter().any(|n| n == project)
            && let Ok(bytes) = compressed::read_entry(
                path,
                inside.format,
                &format!("{root}{project}"),
                FILE_EXCERPT as u64,
            )
        {
            let contents = String::from_utf8_lossy(&bytes);
            writeln!(text, "\n{project}:\n{}", excerpt(&contents, FILE_EXCERPT)).unwrap();
        }
    }
    let markers = inside.markers(rules);
    if !markers.is_empty() {
        writeln!(text, "\nProject markers: {}", markers.join(", ")).unwrap();
    }
    let lines = list_entries(listing, rules);
    writeln!(text, "\nContents:").unwrap();
    for line in lines.iter().take(LISTING_ENTRIES) {
        writeln!(text, "{line}").unwrap();
    }
    if lines.len() > LISTING_ENTRIES {
        writeln!(text, "… and {} more", lines.len() - LISTING_ENTRIES).unwrap();
    }
    text
}

/// Two levels of a compressed file's contents, inside any wrapping folder, indented like a
/// folder's listing.
fn list_entries(listing: &compressed::Listing, rules: &Rules) -> Vec<String> {
    let root = listing.root();
    let mut paths: Vec<(String, bool)> = Vec::new();
    for entry in &listing.entries {
        let path = entry.path.trim_start_matches("./");
        let Some(rest) = path.strip_prefix(root.as_str()) else {
            continue;
        };
        let is_folder = entry.kind == EntryKind::Folder;
        let names: Vec<&str> = rest.split('/').filter(|n| !n.is_empty()).collect();
        // Every folder along the way, implied or not, then the entry itself.
        for depth in 1..=names.len().min(2) {
            let folder = depth < names.len() || is_folder;
            paths.push((names[..depth].join("/"), folder));
        }
    }
    paths.sort();
    paths.dedup();
    let mut lines = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (path, folder) in paths {
        let names: Vec<&str> = path.split('/').collect();
        let name = names[names.len() - 1];
        if name.starts_with('.') || skipped.iter().any(|s| path.starts_with(s.as_str())) {
            continue;
        }
        let indent = "  ".repeat(names.len() - 1);
        if !folder {
            lines.push(format!("{indent}{name}"));
        } else if rules.is_regenerable(name) {
            lines.push(format!("{indent}{name}/ (regenerable)"));
            skipped.push(format!("{path}/"));
        } else {
            lines.push(format!("{indent}{name}/"));
        }
    }
    lines
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
