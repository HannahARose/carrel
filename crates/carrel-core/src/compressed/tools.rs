//! 7z and RAR, through an installed 7-Zip (`7zz`, `7z` or `7za`) or `unar` and `lsar`
//! (`docs/design/compressed.md`), found like other helper programs: beside Carrel, then on
//! `PATH`. Without one, these files are kept compressed and logged as unreadable.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Entry, EntryKind, Error, Listing};
use crate::detect::Compressed;

/// An installed program that reads 7z and RAR files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    /// 7-Zip, at this path.
    SevenZip(PathBuf),
    /// `unar`, with `lsar` beside it for listings.
    Unar {
        /// `unar`.
        unar: PathBuf,
        /// `lsar`.
        lsar: PathBuf,
    },
}

/// The installed tool, if there is one: 7-Zip first, then `unar`. Never on iOS and the other
/// platforms where apps can't run other programs.
#[must_use]
pub fn find() -> Option<Tool> {
    #[cfg(not(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    )))]
    {
        find_in(&crate::providers::process::default_folders())
    }
    #[cfg(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
    {
        None
    }
}

/// The tool in `folders`, if there is one.
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[must_use]
pub fn find_in(folders: &[PathBuf]) -> Option<Tool> {
    use crate::providers::process::find_program;
    for name in ["7zz", "7z", "7za"] {
        if let Some(path) = find_program(name, folders) {
            return Some(Tool::SevenZip(path));
        }
    }
    let unar = find_program("unar", folders)?;
    let lsar = find_program("lsar", folders)?;
    Some(Tool::Unar { unar, lsar })
}

fn tool(format: Compressed) -> Result<Tool, Error> {
    find().ok_or(Error::NoReader(format))
}

/// Runs `command`, returning its standard output, or what went wrong.
fn run(mut command: Command) -> Result<Vec<u8>, Error> {
    let output = command.stdin(Stdio::null()).output().map_err(|e| {
        Error::Invalid(format!(
            "couldn't run {}: {e}",
            command.get_program().display()
        ))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let said = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .and_then(|s| s.lines().last())
            .unwrap_or("it failed")
            .to_owned();
        if said.to_lowercase().contains("password") || said.to_lowercase().contains("encrypted") {
            return Err(Error::Encrypted);
        }
        return Err(Error::Invalid(said));
    }
    Ok(output.stdout)
}

/// What a 7z or RAR file holds, listed by an installed tool.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed, or the tool's error.
pub fn list(path: &Path, format: Compressed) -> Result<Listing, Error> {
    list_with(&tool(format)?, path, format)
}

/// [`list`] with `tool`.
///
/// # Errors
///
/// As [`list`].
pub fn list_with(tool: &Tool, path: &Path, format: Compressed) -> Result<Listing, Error> {
    let compressed_size = std::fs::metadata(path)
        .map_err(|e| Error::Invalid(e.to_string()))?
        .len();
    let entries = match tool {
        Tool::SevenZip(program) => {
            let mut command = Command::new(program);
            command.args(["l", "-slt", "-p", "--"]).arg(path);
            parse_7z(&String::from_utf8_lossy(&run(command)?))
        }
        Tool::Unar { lsar, .. } => {
            let mut command = Command::new(lsar);
            command.arg("-j").arg(path);
            parse_lsar(&String::from_utf8_lossy(&run(command)?))?
        }
    };
    let entries: Vec<Entry> = entries
        .into_iter()
        .filter(|e| !super::is_dropped(&e.path))
        .collect();
    Ok(Listing {
        format,
        compressed_size,
        entries,
        truncated: false,
    })
}

/// Parses `7z l -slt`: blocks of `Key = value` lines, one per entry, after a `----------`
/// line.
#[must_use]
pub fn parse_7z(text: &str) -> Vec<Entry> {
    let Some((_, body)) = text.split_once("\n----------") else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for block in body.split("\n\n") {
        let field = |key: &str| {
            block
                .lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix(" = "))
                .map(str::trim_end)
        };
        let Some(path) = field("Path") else { continue };
        let attributes = field("Attributes").unwrap_or_default();
        let folder = field("Folder") == Some("+") || attributes.starts_with('D');
        let link =
            attributes.contains("-l") || field("Symbolic Link").is_some_and(|l| !l.is_empty());
        entries.push(Entry {
            path: path.replace('\\', "/"),
            kind: if folder {
                EntryKind::Folder
            } else if link {
                EntryKind::Link(field("Symbolic Link").unwrap_or_default().to_owned())
            } else {
                EntryKind::File
            },
            size: (!folder)
                .then(|| field("Size").and_then(|s| s.parse().ok()))
                .flatten(),
            modified: field("Modified").and_then(|m| local_time(m.get(..19)?)),
            executable: attributes.contains(" -rwx") || attributes.contains("x "),
            crc32: None,
            encrypted: field("Encrypted") == Some("+"),
        });
    }
    entries
}

/// Parses `lsar -j`: a JSON object whose `lsarContents` lists the entries.
///
/// # Errors
///
/// [`Error::Invalid`] if it isn't that JSON.
pub fn parse_lsar(text: &str) -> Result<Vec<Entry>, Error> {
    let json: serde_json::Value =
        serde_json::from_str(text).map_err(|e| Error::Invalid(format!("lsar: {e}")))?;
    let contents = json["lsarContents"]
        .as_array()
        .ok_or_else(|| Error::Invalid("lsar listed nothing".into()))?;
    let flag = |entry: &serde_json::Value, key: &str| entry[key].as_i64().is_some_and(|v| v != 0);
    Ok(contents
        .iter()
        .filter_map(|entry| {
            let path = entry["XADFileName"].as_str()?.replace('\\', "/");
            let kind = if flag(entry, "XADIsDirectory") {
                EntryKind::Folder
            } else if let Some(target) = entry["XADLinkDestination"].as_str() {
                EntryKind::Link(target.to_owned())
            } else {
                EntryKind::File
            };
            let mode = entry["XADPosixPermissions"].as_u64().unwrap_or(0);
            Some(Entry {
                size: (kind == EntryKind::File)
                    .then(|| entry["XADFileSize"].as_u64())
                    .flatten(),
                modified: entry["XADLastModificationDate"]
                    .as_str()
                    .and_then(|d| jiff::Timestamp::strptime("%Y-%m-%d %H:%M:%S %z", d).ok()),
                executable: kind == EntryKind::File && mode & 0o111 != 0,
                crc32: None,
                encrypted: flag(entry, "XADIsEncrypted"),
                path,
                kind,
            })
        })
        .collect())
}

/// A `YYYY-MM-DD HH:MM:SS` time on this device's clock.
fn local_time(text: &str) -> Option<jiff::Timestamp> {
    let civil: jiff::civil::DateTime = text.parse().ok()?;
    civil
        .to_zoned(jiff::tz::TimeZone::system())
        .ok()
        .map(|z| z.timestamp())
}

/// Up to `limit` bytes of one file inside, read by an installed tool.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed, or the tool's error.
pub fn read_entry(
    path: &Path,
    format: Compressed,
    inner: &str,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    let mut command = match tool(format)? {
        Tool::SevenZip(program) => {
            let mut command = Command::new(program);
            command.args(["x", "-so", "-p", "--"]).arg(path).arg(inner);
            command
        }
        Tool::Unar { unar, .. } => {
            let mut command = Command::new(unar);
            command.args(["-q", "-o", "-"]).arg(path).arg(inner);
            command
        }
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let mut bytes = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Invalid(e.to_string()))?;
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(bytes)
}

/// Expands into `base`, a new folder, with an installed tool. The caller checks what it wrote.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed, or the tool's error.
pub fn expand(path: &Path, format: Compressed, base: &Path) -> Result<(), Error> {
    expand_with(&tool(format)?, path, base)
}

/// [`expand`] with `tool`.
///
/// # Errors
///
/// As [`expand`].
pub fn expand_with(tool: &Tool, path: &Path, base: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(base).map_err(|e| Error::Invalid(e.to_string()))?;
    let command = match tool {
        Tool::SevenZip(program) => {
            let mut command = Command::new(program);
            command
                .arg("x")
                .arg(format!("-o{}", base.display()))
                .args(["-y", "-p", "--"])
                .arg(path);
            command
        }
        Tool::Unar { unar, .. } => {
            let mut command = Command::new(unar);
            command
                .args(["-q", "-D", "-k", "skip", "-o"])
                .arg(base)
                .arg(path);
            command
        }
    };
    run(command).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_zip_listings_are_parsed() {
        let text = "7-Zip 24.08\n\nListing archive: photos.7z\n\n--\nPath = photos.7z\nType = 7z\n\n----------\nPath = Holiday\nFolder = +\nSize = 0\nModified = 2024-05-06 07:08:00.0000000\nAttributes = D drwxr-xr-x\n\nPath = Holiday/beach.jpg\nFolder = -\nSize = 2048\nModified = 2024-05-06 07:08:00.0000000\nAttributes = A -rw-r--r--\nEncrypted = -\n\nPath = Holiday/run.sh\nFolder = -\nSize = 10\nAttributes = A -rwxr-xr-x\n";
        let entries = parse_7z(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, EntryKind::Folder);
        assert_eq!(entries[1].path, "Holiday/beach.jpg");
        assert_eq!(entries[1].size, Some(2048));
        assert!(entries[1].modified.is_some());
        assert!(!entries[1].executable && entries[2].executable);
    }

    #[test]
    fn lsar_listings_are_parsed() {
        let text = r#"{"lsarFormatVersion":2,"lsarContents":[
            {"XADFileName":"Holiday/beach.jpg","XADFileSize":2048,"XADLastModificationDate":"2024-05-06 07:08:00 +0000","XADPosixPermissions":420},
            {"XADFileName":"Holiday","XADIsDirectory":1},
            {"XADFileName":"Holiday/latest","XADLinkDestination":"beach.jpg"},
            {"XADFileName":"secret.txt","XADFileSize":5,"XADIsEncrypted":1}
        ]}"#;
        let entries = parse_lsar(text).unwrap();
        assert_eq!(entries[0].size, Some(2048));
        assert_eq!(
            entries[0].modified.unwrap().to_string(),
            "2024-05-06T07:08:00Z"
        );
        assert_eq!(entries[1].kind, EntryKind::Folder);
        assert_eq!(entries[2].kind, EntryKind::Link("beach.jpg".into()));
        assert!(entries[3].encrypted);
        assert!(parse_lsar("not json").is_err());
    }

    #[test]
    fn tools_are_found_7zip_first() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_in(&[dir.path().to_owned()]), None);
        let make = |name: &str| {
            let path = dir
                .path()
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            path
        };
        let unar = make("unar");
        assert_eq!(
            find_in(&[dir.path().to_owned()]),
            None,
            "lsar is needed too"
        );
        let lsar = make("lsar");
        assert_eq!(
            find_in(&[dir.path().to_owned()]),
            Some(Tool::Unar { unar, lsar })
        );
        let seven = make("7z");
        assert_eq!(
            find_in(&[dir.path().to_owned()]),
            Some(Tool::SevenZip(seven))
        );
    }
}
