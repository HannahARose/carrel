//! Reading inside compressed files and expanding them (`docs/design/compressed.md`).
//!
//! Zip, tar and the common compressors (gzip, bzip2, xz and zstd) are read here, portably;
//! 7z and RAR go through an installed `7z` or `unar` ([`tools`]). Drafting reads a listing and
//! the odd file without writing anything ([`list`], [`read_entry`]); filing expands an
//! approved archive into a temporary folder, checking every entry as it goes ([`expand`]).

pub mod item;
pub mod plan;
pub mod tools;

use std::collections::HashSet;
use std::io::{Read, Write as _};
use std::path::{Component, Path, PathBuf};

use crate::detect::Compressed;

/// How many files listing reads before it stops: more than any limit allows.
const LISTING_LIMIT: usize = 100_000;

/// When expanding an archive looks too big, so Carrel asks rather than decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// It may expand to this many times its size…
    pub ratio: u64,
    /// …or this many bytes, whichever is more.
    pub size: u64,
    /// It may hold this many files.
    pub files: usize,
}

impl Limits {
    /// Ten times its size or 1 GB, and 100,000 files.
    pub const DEFAULT: Self = Self {
        ratio: 10,
        size: 1 << 30,
        files: LISTING_LIMIT,
    };

    /// The most an archive of `compressed_size` bytes may expand to.
    #[must_use]
    pub fn bound(&self, compressed_size: u64) -> u64 {
        compressed_size.saturating_mul(self.ratio).max(self.size)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The longest name, in bytes, most file systems hold.
const NAME_LIMIT: usize = 255;

/// One entry in a compressed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path inside, with `/` between names, as the archive gives it (backslashes read as
    /// `/`).
    pub path: String,
    /// What it is.
    pub kind: EntryKind,
    /// Its size in bytes, as the archive says; `None` when only expanding tells.
    pub size: Option<u64>,
    /// When it was last modified.
    pub modified: Option<jiff::Timestamp>,
    /// Whether it's executable.
    pub executable: bool,
    /// Its CRC-32, for zip entries.
    pub crc32: Option<u32>,
    /// Whether it's encrypted.
    pub encrypted: bool,
}

/// What an entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// A file.
    File,
    /// A folder.
    Folder,
    /// A symbolic link to the target.
    Link(String),
    /// A hard link to an earlier entry, by its path.
    HardLink(String),
    /// Anything else, such as a device or a pipe: never expanded.
    Other,
}

/// What drafting learned about a compressed file, without expanding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inside {
    /// Its format.
    pub format: Compressed,
    /// What it holds, when it could be read.
    pub listing: Option<Listing>,
    /// Why it couldn't be read, when it couldn't.
    pub unread: Option<String>,
    /// Whether nothing on this device reads its format.
    pub no_reader: bool,
    /// Whether it's one part of an archive split into several.
    pub split: bool,
}

impl Inside {
    /// The project markers (`rules`) at the top level of what expanding makes, sorted: what
    /// would make it a bundle.
    #[must_use]
    pub fn markers(&self, rules: &crate::detect::Rules) -> Vec<String> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        if matches!(listing.shape(), Shape::OneFile(_)) {
            return Vec::new();
        }
        listing
            .top_level()
            .into_iter()
            .filter(|name| rules.markers.iter().any(|p| p.matches(name)))
            .collect()
    }

    /// What it holds, in a few words: `one file, report.pdf`, `a folder, Export, with 440
    /// files` or `12 files`.
    #[must_use]
    pub fn describe(&self) -> String {
        let Some(listing) = &self.listing else {
            return "contents unknown".into();
        };
        let files = listing.file_count();
        let count = if files == 1 {
            "1 file".to_owned()
        } else {
            format!("{files} files")
        };
        match listing.shape() {
            Shape::Empty => "nothing".into(),
            Shape::OneFile(name) => format!("one file, {name}"),
            Shape::OneFolder(name) => format!("a folder, {name}, with {count}"),
            Shape::Many => count,
        }
    }
}

/// What the compressed file at `path` holds, read without expanding it; `None` for anything
/// that isn't a compressed file ([`Compressed::of`]) or a part of a split one.
#[must_use]
pub fn inspect(path: &Path) -> Option<Inside> {
    if let Some(format) = split_format(path) {
        return Some(Inside {
            format,
            listing: None,
            unread: None,
            no_reader: false,
            split: true,
        });
    }
    let format = Compressed::of(path)?;
    Some(match list(path, format, &Limits::DEFAULT) {
        Ok(listing) => Inside {
            format,
            listing: Some(listing),
            unread: None,
            no_reader: false,
            split: false,
        },
        Err(error) => Inside {
            format,
            listing: None,
            no_reader: matches!(error, Error::NoReader(_)),
            unread: Some(error.to_string()),
            split: false,
        },
    })
}

/// For a part of a split archive (`backup.z01`, `backup.7z.001`, `film.part2.rar`, or a
/// `backup.zip` with a `backup.z01` beside it), the archive's format.
fn split_format(path: &Path) -> Option<Compressed> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if crate::detect::split_part(&name).is_some() {
        let (rest, last) = name.rsplit_once('.')?;
        return Some(match last.as_bytes()[0] {
            b'z' => Compressed::Zip,
            b'0'..=b'9' => Compressed::from_name(rest).unwrap_or(Compressed::Rar),
            _ => Compressed::Rar,
        });
    }
    name.strip_suffix(".zip")?;
    let sibling = path.with_extension("z01");
    sibling.exists().then_some(Compressed::Zip)
}

/// What a compressed file holds, without the files Carrel drops (`__MACOSX/`, `.DS_Store`,
/// `AppleDouble` `._` files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The format.
    pub format: Compressed,
    /// The file's own size in bytes.
    pub compressed_size: u64,
    /// The entries, in the archive's order.
    pub entries: Vec<Entry>,
    /// Whether listing stopped at the file limit ([`Limits::files`]), so there are more.
    pub truncated: bool,
}

/// The shape of an archive's contents, which decides what expanding it makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// Nothing at all.
    Empty,
    /// One file, at this path: it becomes that file.
    OneFile(String),
    /// One folder wrapping everything, at this path: it becomes that folder.
    OneFolder(String),
    /// Anything else: it becomes a folder named after the archive.
    Many,
}

/// Why a compressed file couldn't be read or expanded.
#[derive(Debug)]
pub enum Error {
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The file isn't a valid archive of its format, or an entry failed its check.
    Invalid(String),
    /// It's encrypted; Carrel never asks for archive passwords.
    Encrypted,
    /// Nothing on this device reads its format.
    NoReader(Compressed),
    /// Expanding it would be unsafe: an entry would land outside its folder, or its name can't
    /// be held.
    Unsafe(String),
    /// It's bigger than the limits allow.
    TooBig(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid(message) | Self::Unsafe(message) | Self::TooBig(message) => {
                f.write_str(message)
            }
            Self::Encrypted => f.write_str("it's encrypted"),
            Self::NoReader(format) => write!(
                f,
                "nothing on this device opens {} files; install 7-Zip (7z) or unar",
                format.name()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn invalid(e: impl std::fmt::Display) -> Error {
    Error::Invalid(e.to_string())
}

/// Whether Carrel drops this entry when reading or expanding: anything under `__MACOSX/`,
/// and `.DS_Store` or `AppleDouble` (`._`) files anywhere.
#[must_use]
pub fn is_dropped(path: &str) -> bool {
    path.split('/')
        .any(|name| name == "__MACOSX" || name == ".DS_Store" || name.starts_with("._"))
}

impl Listing {
    /// The entries that are files.
    pub fn files(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::File | EntryKind::HardLink(_)))
    }

    /// How many files it holds.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files().count()
    }

    /// The total size of its files, as far as the listing says.
    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.files().filter_map(|e| e.size).sum()
    }

    /// Whether any entry is encrypted.
    #[must_use]
    pub fn encrypted(&self) -> bool {
        self.entries.iter().any(|e| e.encrypted)
    }

    /// What expanding makes: one file, one wrapping folder, or many entries.
    #[must_use]
    pub fn shape(&self) -> Shape {
        let tops: HashSet<&str> = self
            .entries
            .iter()
            .map(|e| top(&e.path))
            .filter(|t| !t.is_empty())
            .collect();
        let Some(&only) = tops.iter().next() else {
            return Shape::Empty;
        };
        if tops.len() > 1 {
            return Shape::Many;
        }
        let nested = self.entries.iter().any(|e| trimmed(&e.path) != only);
        let is_file = self
            .entries
            .iter()
            .any(|e| trimmed(&e.path) == only && !matches!(e.kind, EntryKind::Folder));
        match (is_file, nested) {
            (true, false) => Shape::OneFile(only.to_owned()),
            (false, _) => Shape::OneFolder(only.to_owned()),
            (true, true) => Shape::Many,
        }
    }

    /// The prefix the item's own contents start under: the wrapping folder and a `/` for
    /// [`Shape::OneFolder`], otherwise nothing.
    #[must_use]
    pub fn root(&self) -> String {
        match self.shape() {
            Shape::OneFolder(folder) => format!("{folder}/"),
            _ => String::new(),
        }
    }

    /// The names at the top level of the item expanding makes, sorted: for a wrapping folder,
    /// what's inside it.
    #[must_use]
    pub fn top_level(&self) -> Vec<String> {
        let root = self.root();
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| trimmed(&e.path).strip_prefix(root.as_str()))
            .map(|rest| top(rest).to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The entry at `path` inside the item (after the wrapping folder), if there is one.
    #[must_use]
    pub fn find(&self, path: &str) -> Option<&Entry> {
        let root = self.root();
        self.entries
            .iter()
            .find(|e| trimmed(&e.path).strip_prefix(root.as_str()) == Some(path))
    }

    /// What makes expanding it unsafe, if anything: an entry that would land outside its
    /// folder, a link pointing out, a name the file system can't hold, or two names that
    /// differ only in case.
    #[must_use]
    pub fn unsafe_entry(&self) -> Option<String> {
        let mut seen = HashSet::new();
        for entry in &self.entries {
            if let Err(why) = check_entry(entry) {
                return Some(why);
            }
            if !seen.insert(trimmed(&entry.path).to_lowercase()) && entry.kind != EntryKind::Folder
            {
                return Some(format!(
                    "two entries are named \u{201c}{}\u{201d}, apart from case",
                    entry.path
                ));
            }
        }
        None
    }

    /// Why expanding looks too big under `limits`, if it does.
    #[must_use]
    pub fn too_big(&self, limits: &Limits) -> Option<String> {
        let count = self.file_count();
        if self.truncated || count > limits.files {
            return Some(format!("it holds more than {} files", limits.files));
        }
        let size = self.total_size();
        (size > limits.bound(self.compressed_size)).then(|| {
            format!(
                "it would expand to {}, more than {} times its size",
                crate::human_size(size),
                limits.ratio
            )
        })
    }
}

/// The first name in `path`.
fn top(path: &str) -> &str {
    trimmed(path).split('/').next().unwrap_or_default()
}

/// `path` without leading `./` or a trailing `/`.
fn trimmed(path: &str) -> &str {
    let mut path = path;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    path.trim_end_matches('/')
}

/// `path` as a relative path inside the item's folder, or why it can't be one.
///
/// # Errors
///
/// A message when the path is absolute, climbs out with `..`, or has a name the file system
/// can't hold.
pub fn safe_path(path: &str) -> Result<PathBuf, String> {
    let path = trimmed(path);
    let windows_drive = path.len() >= 2 && path.as_bytes()[1] == b':';
    if path.starts_with('/') || windows_drive {
        return Err(format!("\u{201c}{path}\u{201d} is an absolute path"));
    }
    let mut safe = PathBuf::new();
    for name in path.split('/') {
        match name {
            "" | "." => {}
            ".." => return Err(format!("\u{201c}{path}\u{201d} climbs out of its folder")),
            _ if name.len() > NAME_LIMIT => {
                return Err(format!("a name in \u{201c}{path}\u{201d} is too long"));
            }
            _ if name.chars().any(char::is_control) => {
                return Err(format!(
                    "\u{201c}{}\u{201d} has a control character in its name",
                    path.escape_debug()
                ));
            }
            _ => safe.push(name),
        }
    }
    let first = Path::new(path).components().next();
    if matches!(first, Some(Component::Prefix(_) | Component::RootDir)) {
        return Err(format!("\u{201c}{path}\u{201d} is an absolute path"));
    }
    Ok(safe)
}

/// Checks one entry's path, and a link's target, stay inside the item's folder.
fn check_entry(entry: &Entry) -> Result<(), String> {
    let path = safe_path(&entry.path)?;
    match &entry.kind {
        EntryKind::Link(target) => {
            let parent = path.parent().unwrap_or(Path::new(""));
            if target.starts_with('/') || escapes(&parent.join(target)) {
                return Err(format!(
                    "the link \u{201c}{}\u{201d} points outside its folder",
                    entry.path
                ));
            }
        }
        EntryKind::HardLink(target) => {
            safe_path(target)?;
        }
        EntryKind::Other => {
            return Err(format!(
                "\u{201c}{}\u{201d} is a device or pipe, not a file",
                entry.path
            ));
        }
        EntryKind::File | EntryKind::Folder => {}
    }
    Ok(())
}

/// Whether a relative path, followed name by name, climbs above where it starts.
fn escapes(path: &Path) -> bool {
    let mut depth = 0i64;
    for component in path.components() {
        match component {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return true,
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

/// A decompressing reader over a compressor's stream.
fn decompress(format: Compressed, file: std::fs::File) -> Result<Box<dyn Read>, Error> {
    let file = std::io::BufReader::new(file);
    Ok(match format {
        Compressed::Zip | Compressed::Tar | Compressed::SevenZ | Compressed::Rar => Box::new(file),
        Compressed::TarGz | Compressed::Gz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        Compressed::TarBz2 | Compressed::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(file)),
        Compressed::TarXz | Compressed::Xz => Box::new(lzma_rust2::XzReader::new(file, true)),
        Compressed::TarZst | Compressed::Zst => Box::new(ZstFrames::new(file)?),
    })
}

/// A zstd stream of one or more frames.
struct ZstFrames<R: Read> {
    decoder: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: Read> ZstFrames<R> {
    fn new(source: R) -> Result<Self, Error> {
        let decoder = ruzstd::decoding::StreamingDecoder::new(source).map_err(invalid)?;
        Ok(Self {
            decoder: Some(decoder),
        })
    }
}

impl<R: Read> Read for ZstFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let Some(decoder) = self.decoder.as_mut() else {
                return Ok(0);
            };
            let read = decoder.read(buf)?;
            if read > 0 || buf.is_empty() {
                return Ok(read);
            }
            // The frame is done: start the next one, if the stream has more.
            let mut decoder = self.decoder.take().expect("checked above");
            let mut frame = ruzstd::decoding::FrameDecoder::new();
            match frame.init(decoder.get_mut()) {
                Ok(()) => {
                    decoder.decoder = frame;
                    self.decoder = Some(decoder);
                }
                Err(ruzstd::decoding::errors::FrameDecoderError::ReadFrameHeaderError(
                    ruzstd::decoding::errors::ReadFrameHeaderError::SkipFrame { .. }
                    | ruzstd::decoding::errors::ReadFrameHeaderError::MagicNumberReadError(_),
                )) => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e.to_string())),
            }
        }
    }
}

/// What a walk calls with each entry, and a reader for a file's contents when asked for.
type Visit<'a> = dyn FnMut(&Entry, Option<&mut dyn Read>) -> Result<Flow, Error> + 'a;

/// What a walk over the entries does next.
enum Flow {
    Next,
    Stop,
}

/// Calls `visit` with each entry Carrel keeps, and a reader for a file's contents when
/// `contents` is true. Stops after `limit` files, returning whether it did.
fn walk(
    path: &Path,
    format: Compressed,
    contents: bool,
    limit: usize,
    visit: &mut Visit<'_>,
) -> Result<bool, Error> {
    let file = std::fs::File::open(path).map_err(io(path))?;
    match format {
        Compressed::Zip => walk_zip(file, contents, limit, visit),
        Compressed::Tar
        | Compressed::TarGz
        | Compressed::TarBz2
        | Compressed::TarXz
        | Compressed::TarZst => walk_tar(decompress(format, file)?, contents, limit, visit),
        Compressed::Gz | Compressed::Bz2 | Compressed::Xz | Compressed::Zst => {
            let name = path
                .file_name()
                .map(|n| format.stem(&n.to_string_lossy()))
                .unwrap_or_default();
            let entry = Entry {
                path: name,
                kind: EntryKind::File,
                size: None,
                modified: std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| jiff::Timestamp::try_from(t).ok()),
                executable: false,
                crc32: None,
                encrypted: false,
            };
            let mut reader = decompress(format, file)?;
            visit(&entry, contents.then_some(&mut *reader as &mut dyn Read))?;
            Ok(false)
        }
        Compressed::SevenZ | Compressed::Rar => Err(Error::NoReader(format)),
    }
}

fn walk_zip(
    file: std::fs::File,
    contents: bool,
    limit: usize,
    visit: &mut Visit<'_>,
) -> Result<bool, Error> {
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(invalid)?;
    let mut files = 0;
    for i in 0..zip.len() {
        let entry = zip_entry(&zip.by_index_raw(i).map_err(invalid)?);
        if is_dropped(&entry.path) {
            continue;
        }
        if entry.kind == EntryKind::File {
            files += 1;
            if files > limit {
                return Ok(true);
            }
        }
        let flow = if contents && entry.kind == EntryKind::File && !entry.encrypted {
            let mut item = zip.by_index(i).map_err(invalid)?;
            visit(&entry, Some(&mut item))?
        } else if matches!(entry.kind, EntryKind::Link(_)) && !entry.encrypted {
            let mut target = String::new();
            let mut item = zip.by_index(i).map_err(invalid)?;
            item.read_to_string(&mut target).map_err(invalid)?;
            visit(
                &Entry {
                    kind: EntryKind::Link(target),
                    ..entry
                },
                None,
            )?
        } else {
            visit(&entry, None)?
        };
        if matches!(flow, Flow::Stop) {
            break;
        }
    }
    Ok(false)
}

/// A zip entry as an [`Entry`]. A link's target is read from its contents by the caller.
fn zip_entry<R: Read>(item: &zip::read::ZipFile<'_, R>) -> Entry {
    let path = zip_name(item.name_raw()).unwrap_or_else(|| item.name().unwrap_or_default().into());
    let path = path.replace('\\', "/");
    let mode = item.unix_mode();
    let kind = if item.is_dir() {
        EntryKind::Folder
    } else if item.is_symlink() {
        EntryKind::Link(String::new())
    } else {
        EntryKind::File
    };
    Entry {
        path,
        size: Some(item.size()),
        modified: item
            .last_modified()
            .and_then(|d| jiff::civil::DateTime::try_from(d).ok())
            .and_then(|d| d.to_zoned(jiff::tz::TimeZone::system()).ok())
            .map(|z| z.timestamp()),
        executable: mode.is_some_and(|m| m & 0o111 != 0) && kind == EntryKind::File,
        crc32: Some(item.crc32()),
        encrypted: item.encrypted(),
        kind,
    }
}

/// A zip entry's name when its bytes are UTF-8: always when the UTF-8 flag is set, and in
/// practice when they're valid, as macOS writes them without the flag. `None` means CP437,
/// which the zip reader decodes.
fn zip_name(raw: &[u8]) -> Option<String> {
    std::str::from_utf8(raw).ok().map(str::to_owned)
}

fn walk_tar(
    reader: Box<dyn Read>,
    contents: bool,
    limit: usize,
    visit: &mut Visit<'_>,
) -> Result<bool, Error> {
    let mut tar = tar::Archive::new(reader);
    let mut files = 0;
    for item in tar.entries().map_err(invalid)? {
        let mut item = item.map_err(invalid)?;
        let Some(entry) = tar_entry(&item)? else {
            continue;
        };
        if is_dropped(&entry.path) {
            continue;
        }
        if matches!(entry.kind, EntryKind::File | EntryKind::HardLink(_)) {
            files += 1;
            if files > limit {
                return Ok(true);
            }
        }
        let reader = (contents && entry.kind == EntryKind::File).then_some(&mut item);
        if matches!(
            visit(&entry, reader.map(|r| r as &mut dyn Read))?,
            Flow::Stop
        ) {
            break;
        }
    }
    Ok(false)
}

/// A tar entry as an [`Entry`]; `None` for the headers tar uses for itself.
fn tar_entry<R: Read>(item: &tar::Entry<'_, R>) -> Result<Option<Entry>, Error> {
    use tar::EntryType as T;
    let header = item.header();
    let path = String::from_utf8(item.path_bytes().into_owned()).map_err(|e| {
        Error::Unsafe(format!(
            "\u{201c}{}\u{201d} isn't a name this device can hold",
            String::from_utf8_lossy(e.as_bytes())
        ))
    })?;
    let target = || {
        item.link_name_bytes()
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .unwrap_or_default()
    };
    let kind = match header.entry_type() {
        T::Regular | T::Continuous => EntryKind::File,
        T::Directory => EntryKind::Folder,
        T::Symlink => EntryKind::Link(target()),
        T::Link => EntryKind::HardLink(target()),
        T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink => return Ok(None),
        _ => EntryKind::Other,
    };
    let mode = header.mode().unwrap_or(0);
    Ok(Some(Entry {
        path,
        size: (kind == EntryKind::File).then(|| header.size().unwrap_or(0)),
        modified: header
            .mtime()
            .ok()
            .and_then(|t| jiff::Timestamp::from_second(i64::try_from(t).ok()?).ok()),
        executable: kind == EntryKind::File && mode & 0o111 != 0,
        crc32: None,
        encrypted: false,
        kind,
    }))
}

/// What `path`, a compressed file of `format`, holds. For a compressor around one file, its
/// size is found by reading it, up to the bound `limits` set.
///
/// # Errors
///
/// [`Error`] if it can't be read.
pub fn list(path: &Path, format: Compressed, limits: &Limits) -> Result<Listing, Error> {
    let compressed_size = std::fs::metadata(path).map_err(io(path))?.len();
    if !format.is_portable() {
        return tools::list(path, format);
    }
    let mut entries = Vec::new();
    let single = !format.is_archive();
    let bound = limits.bound(compressed_size);
    let truncated = walk(path, format, single, limits.files, &mut |entry, reader| {
        let mut entry = entry.clone();
        if let Some(reader) = reader {
            let counted = std::io::copy(&mut reader.take(bound + 1), &mut std::io::sink())
                .map_err(invalid)?;
            entry.size = Some(counted);
        }
        entries.push(entry);
        Ok(Flow::Next)
    })?;
    Ok(Listing {
        format,
        compressed_size,
        entries,
        truncated,
    })
}

/// Up to `limit` bytes of the file at `inner` (its path in the archive, as listed).
///
/// # Errors
///
/// [`Error`] if the archive can't be read, or has no such file.
pub fn read_entry(
    path: &Path,
    format: Compressed,
    inner: &str,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    if !format.is_portable() {
        return tools::read_entry(path, format, inner, limit);
    }
    let mut found = None;
    walk(path, format, true, usize::MAX, &mut |entry, reader| {
        if entry.path != inner {
            return Ok(Flow::Next);
        }
        if entry.encrypted {
            return Err(Error::Encrypted);
        }
        let mut bytes = Vec::new();
        if let Some(reader) = reader {
            reader
                .take(limit)
                .read_to_end(&mut bytes)
                .map_err(invalid)?;
        }
        found = Some(bytes);
        Ok(Flow::Stop)
    })?;
    found.ok_or_else(|| Error::Invalid(format!("no {inner} inside")))
}

/// What [`expand`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    /// The item, inside the folder it was expanded into: the one file, the wrapping folder, or
    /// a folder named after the archive.
    pub path: PathBuf,
    /// How many files were written.
    pub files: usize,
    /// How many bytes.
    pub size: u64,
}

/// Expands `path`, a compressed file of `format`, into `into` (a new, empty folder on the
/// volume it's going to), and unwraps it: one file inside becomes that file, one folder
/// wrapping everything becomes that folder, and anything else a folder named after the
/// archive. On Windows, symbolic links are left out. Every entry is checked as it's written: no path or link may land outside,
/// `limits` hold even if the listing understated them (`None` when the owner said to go ahead
/// anyway), and each file's CRC-32 (zip) or size (tar) must match. Modification times and execute permissions are kept; `__MACOSX/`,
/// `.DS_Store` and `._` files are dropped.
///
/// # Errors
///
/// [`Error`]; `into` may then hold part of the expansion, for the caller to remove.
pub fn expand(
    path: &Path,
    format: Compressed,
    into: &Path,
    limits: Option<&Limits>,
) -> Result<Expanded, Error> {
    let name = path
        .file_name()
        .map(|n| format.stem(&n.to_string_lossy()))
        .unwrap_or_default();
    let name = safe_path(&name)
        .ok()
        .filter(|p| p.components().count() == 1)
        .map_or_else(|| "Expanded".to_owned(), |_| name);
    let base = into.join(&name);
    let compressed_size = std::fs::metadata(path).map_err(io(path))?.len();
    let mut writer = Writer::new(base.clone(), compressed_size, limits.copied());
    if format.is_portable() {
        std::fs::create_dir_all(&base).map_err(io(&base))?;
        walk(path, format, true, usize::MAX, &mut |entry, reader| {
            writer.write(entry, reader)?;
            Ok(Flow::Next)
        })?;
    } else {
        // The tool can't be watched entry by entry, so its listing is checked first, and
        // what it wrote afterwards.
        let listing = tools::list(path, format)?;
        if let Some(why) = listing.unsafe_entry() {
            return Err(Error::Unsafe(why));
        }
        if listing.encrypted() {
            return Err(Error::Encrypted);
        }
        if let Some(why) = limits.and_then(|l| listing.too_big(l)) {
            return Err(Error::TooBig(why));
        }
        tools::expand(path, format, &base)?;
        writer.check_tree()?;
    }
    writer.finish()?;
    let (files, size) = (writer.files, writer.size);
    Ok(Expanded {
        path: unwrap(&base)?,
        files,
        size,
    })
}

/// The item `base` holds: the one file or folder in it, moved up beside it, or `base` itself.
fn unwrap(base: &Path) -> Result<PathBuf, Error> {
    let children: Vec<PathBuf> = std::fs::read_dir(base)
        .map_err(io(base))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let [only] = children.as_slice() else {
        if children.is_empty() {
            return Err(Error::Invalid("there's nothing inside".into()));
        }
        return Ok(base.to_owned());
    };
    if std::fs::symlink_metadata(only).is_ok_and(|m| m.file_type().is_symlink()) {
        return Ok(base.to_owned());
    }
    let parent = base.parent().unwrap_or(base);
    let name = only.file_name().unwrap_or_default();
    // Move it beside the base folder, through a temporary name in case they're the same.
    let temp = parent.join(format!(".unwrap-{}", crate::card::Id::new_v7()));
    std::fs::rename(only, &temp).map_err(io(only))?;
    std::fs::remove_dir(base).map_err(io(base))?;
    let target = parent.join(name);
    std::fs::rename(&temp, &target).map_err(io(&temp))?;
    Ok(target)
}

/// Writes entries under one folder, keeping count against the limits.
struct Writer {
    base: PathBuf,
    compressed_size: u64,
    limits: Option<Limits>,
    files: usize,
    size: u64,
    seen: HashSet<String>,
    /// Modification times to set once everything is written, folders last.
    times: Vec<(PathBuf, jiff::Timestamp)>,
}

impl Writer {
    fn new(base: PathBuf, compressed_size: u64, limits: Option<Limits>) -> Self {
        Self {
            base,
            compressed_size,
            limits,
            files: 0,
            size: 0,
            seen: HashSet::new(),
            times: Vec::new(),
        }
    }

    fn write(&mut self, entry: &Entry, reader: Option<&mut dyn Read>) -> Result<(), Error> {
        if entry.encrypted {
            return Err(Error::Encrypted);
        }
        check_entry(entry).map_err(Error::Unsafe)?;
        let relative = safe_path(&entry.path).map_err(Error::Unsafe)?;
        if relative.as_os_str().is_empty() {
            return Ok(());
        }
        let key = relative.to_string_lossy().to_lowercase();
        if !self.seen.insert(key) && entry.kind != EntryKind::Folder {
            return Err(Error::Unsafe(format!(
                "two entries are named \u{201c}{}\u{201d}, apart from case",
                entry.path
            )));
        }
        let target = self.base.join(&relative);
        if let Some(parent) = target.parent() {
            self.make_folder(parent)?;
        }
        match &entry.kind {
            EntryKind::Folder => self.make_folder(&target)?,
            EntryKind::File => self.write_file(entry, &target, reader)?,
            EntryKind::Link(link) => make_link(link, &target)?,
            EntryKind::HardLink(earlier) => {
                let source = self.base.join(safe_path(earlier).map_err(Error::Unsafe)?);
                if !std::fs::symlink_metadata(&source).is_ok_and(|m| m.is_file()) {
                    return Err(Error::Invalid(format!(
                        "{} links to {earlier}, which isn't an earlier file",
                        entry.path
                    )));
                }
                let copied = std::fs::copy(&source, &target).map_err(io(&target))?;
                self.count(copied)?;
            }
            EntryKind::Other => unreachable!("check_entry refuses these"),
        }
        if let Some(modified) = entry.modified
            && !matches!(entry.kind, EntryKind::Link(_))
        {
            self.times.push((target, modified));
        }
        Ok(())
    }

    fn make_folder(&self, folder: &Path) -> Result<(), Error> {
        // A link inside may not stand in for a folder, or a later entry could be written
        // through it.
        let mut at = self.base.clone();
        for component in folder
            .strip_prefix(&self.base)
            .unwrap_or(folder)
            .components()
        {
            at.push(component);
            match std::fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(Error::Unsafe(format!(
                        "an entry would be written through the link {}",
                        at.strip_prefix(&self.base).unwrap_or(&at).display()
                    )));
                }
                Ok(m) if !m.is_dir() => {
                    return Err(Error::Invalid(format!(
                        "{} is both a file and a folder",
                        at.strip_prefix(&self.base).unwrap_or(&at).display()
                    )));
                }
                Ok(_) => {}
                Err(_) => std::fs::create_dir(&at).map_err(io(&at))?,
            }
        }
        Ok(())
    }

    fn write_file(
        &mut self,
        entry: &Entry,
        target: &Path,
        reader: Option<&mut dyn Read>,
    ) -> Result<(), Error> {
        let reader =
            reader.ok_or_else(|| Error::Invalid(format!("{} can't be read", entry.path)))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(io(target))?;
        let mut hasher = crc32fast::Hasher::new();
        let mut written = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|e| Error::Invalid(format!("{} failed its check: {e}", entry.path)))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read]).map_err(io(target))?;
            written += read as u64;
            self.count_bytes(read as u64)?;
        }
        self.count_file()?;
        if let Some(size) = entry.size
            && size != written
        {
            return Err(Error::Invalid(format!(
                "{} is {written} bytes, not the {size} the archive says",
                entry.path
            )));
        }
        if let Some(crc) = entry.crc32
            && crc != hasher.finalize()
        {
            return Err(Error::Invalid(format!(
                "{} failed its checksum",
                entry.path
            )));
        }
        if entry.executable {
            make_executable(&file, target)?;
        }
        Ok(())
    }

    fn count(&mut self, n: u64) -> Result<(), Error> {
        self.count_file()?;
        self.count_bytes(n)
    }

    fn count_file(&mut self) -> Result<(), Error> {
        self.files += 1;
        match self.limits {
            Some(limits) if self.files > limits.files => Err(Error::TooBig(format!(
                "it holds more than {} files",
                limits.files
            ))),
            _ => Ok(()),
        }
    }

    fn count_bytes(&mut self, n: u64) -> Result<(), Error> {
        self.size += n;
        match self.limits {
            Some(limits) if self.size > limits.bound(self.compressed_size) => {
                Err(Error::TooBig(format!(
                    "it expands to more than {}, over {} times its size",
                    crate::human_size(limits.bound(self.compressed_size)),
                    limits.ratio
                )))
            }
            _ => Ok(()),
        }
    }

    /// Checks a tree an installed tool wrote: no links pointing out, no names that can't be
    /// held, and the limits.
    fn check_tree(&mut self) -> Result<(), Error> {
        let mut stack = vec![self.base.clone()];
        while let Some(dir) = stack.pop() {
            for child in std::fs::read_dir(&dir).map_err(io(&dir))? {
                let child = child.map_err(io(&dir))?;
                let path = child.path();
                let relative = path
                    .strip_prefix(&self.base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let meta = std::fs::symlink_metadata(&path).map_err(io(&path))?;
                if is_dropped(&relative) {
                    remove(&path).map_err(io(&path))?;
                    continue;
                }
                let kind = if meta.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).map_err(io(&path))?;
                    EntryKind::Link(target.to_string_lossy().into_owned())
                } else if meta.is_dir() {
                    stack.push(path.clone());
                    EntryKind::Folder
                } else if meta.is_file() {
                    self.count(meta.len())?;
                    EntryKind::File
                } else {
                    EntryKind::Other
                };
                let entry = Entry {
                    path: relative,
                    kind,
                    size: None,
                    modified: None,
                    executable: false,
                    crc32: None,
                    encrypted: false,
                };
                check_entry(&entry).map_err(Error::Unsafe)?;
            }
        }
        Ok(())
    }

    /// Sets the modification times, deepest first so writing a file doesn't change its
    /// folder's time afterwards.
    fn finish(&mut self) -> Result<(), Error> {
        self.times
            .sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
        for (path, time) in &self.times {
            let time = filetime::FileTime::from_unix_time(
                time.as_second(),
                u32::try_from(time.subsec_nanosecond()).unwrap_or(0),
            );
            filetime::set_symlink_file_times(path, time, time).map_err(io(path))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn make_executable(file: &std::fs::File, path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = file.metadata().map_err(io(path))?.permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    file.set_permissions(permissions).map_err(io(path))
}

#[cfg(not(unix))]
fn make_executable(_file: &std::fs::File, _path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(unix)]
fn make_link(link: &str, at: &Path) -> Result<(), Error> {
    std::os::unix::fs::symlink(link, at).map_err(io(at))
}

/// Where making a symbolic link needs privileges most people don't have (Windows), links are
/// left out; their targets are expanded like any other entry.
#[cfg(not(unix))]
fn make_link(_link: &str, _at: &Path) -> Result<(), Error> {
    Ok(())
}

fn remove(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: EntryKind) -> Entry {
        Entry {
            path: path.into(),
            kind,
            size: Some(1),
            modified: None,
            executable: false,
            crc32: None,
            encrypted: false,
        }
    }

    fn listing(entries: Vec<Entry>) -> Listing {
        Listing {
            format: Compressed::Zip,
            compressed_size: 10,
            entries,
            truncated: false,
        }
    }

    #[test]
    fn shapes_decide_what_expanding_makes() {
        assert_eq!(listing(vec![]).shape(), Shape::Empty);
        assert_eq!(
            listing(vec![entry("report.pdf", EntryKind::File)]).shape(),
            Shape::OneFile("report.pdf".into())
        );
        let wrapped = listing(vec![
            entry("Export/", EntryKind::Folder),
            entry("Export/a.csv", EntryKind::File),
            entry("Export/README.md", EntryKind::File),
        ]);
        assert_eq!(wrapped.shape(), Shape::OneFolder("Export".into()));
        assert_eq!(wrapped.top_level(), ["README.md", "a.csv"]);
        assert!(wrapped.find("README.md").is_some());
        let implied = listing(vec![entry("./Export/a.csv", EntryKind::File)]);
        assert_eq!(implied.shape(), Shape::OneFolder("Export".into()));
        let many = listing(vec![
            entry("a.csv", EntryKind::File),
            entry("b.csv", EntryKind::File),
        ]);
        assert_eq!(many.shape(), Shape::Many);
        assert_eq!(many.top_level(), ["a.csv", "b.csv"]);
    }

    #[test]
    fn unsafe_paths_and_links_are_found() {
        for path in [
            "/etc/passwd",
            "../up",
            "a/../../up",
            "C:/Windows",
            "a/\u{1}b",
        ] {
            assert!(safe_path(path).is_err(), "{path}");
        }
        assert_eq!(safe_path("./a/b/").unwrap(), Path::new("a/b"));
        assert!(safe_path(&"x".repeat(300)).is_err());
        let out = listing(vec![entry("a/link", EntryKind::Link("../../etc".into()))]);
        assert!(out.unsafe_entry().unwrap().contains("points outside"));
        let fine = listing(vec![entry("a/link", EntryKind::Link("../b".into()))]);
        assert_eq!(fine.unsafe_entry(), None);
        let cased = listing(vec![
            entry("Notes.txt", EntryKind::File),
            entry("notes.TXT", EntryKind::File),
        ]);
        assert!(cased.unsafe_entry().unwrap().contains("apart from case"));
    }

    #[test]
    fn limits_ask_for_big_expansions() {
        let mut big = listing(vec![Entry {
            size: Some(2 << 30),
            ..entry("disk.img", EntryKind::File)
        }]);
        let limits = Limits::DEFAULT;
        assert!(big.too_big(&limits).unwrap().contains("10 times"));
        big.compressed_size = 1 << 30;
        assert_eq!(big.too_big(&limits), None, "under ten times its size");
        assert_eq!(limits.bound(1), 1 << 30);
        let mut many = listing(vec![]);
        many.truncated = true;
        assert!(many.too_big(&limits).unwrap().contains("100000 files"));
    }

    #[test]
    fn dropped_entries() {
        assert!(is_dropped("__MACOSX/Export/._a.csv"));
        assert!(is_dropped("Export/.DS_Store"));
        assert!(is_dropped("Export/._a.csv"));
        assert!(!is_dropped("Export/a.csv"));
        assert!(!is_dropped("Vault/.obsidian/app.json"));
    }
}
