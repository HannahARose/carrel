//! Finder tags on macOS: the names in a file's `com.apple.metadata:_kMDItemUserTags`
//! extended attribute, a binary property list holding an array of strings (`Name`, or
//! `Name\nColour` for a coloured tag). Carrel adds one of its own and takes it away again,
//! leaving every other tag as it was. Elsewhere these do nothing.
//!
//! The attribute is read and written with macOS's own `/usr/bin/xattr`, so no unsafe code is
//! needed. A list Carrel can't read is left alone rather than replaced.

use std::path::Path;

/// The tag a checked-out item's working copy carries (`docs/design/checkout.md`).
pub const CHECKED_OUT: &str = "Checked Out";

/// Adds the tag `name` to the file or folder at `path`, after any it already has. A tag of
/// that name already there, in any colour, is left as it is.
///
/// # Errors
///
/// An error running `xattr`, or the existing tags not being a list it can read. Never on
/// other systems.
pub fn add_tag(path: &Path, name: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut tags = mac::read(path)?;
        if tags.iter().any(|t| tag_name(t) == name) {
            return Ok(());
        }
        tags.push(name.to_owned());
        mac::write(path, &tags)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, name);
        Ok(())
    }
}

/// Removes the tag `name`, in any colour, from the file or folder at `path`, keeping the
/// others. Nothing at `path`, or no such tag, is not an error.
///
/// # Errors
///
/// As [`add_tag`].
pub fn remove_tag(path: &Path, name: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if std::fs::symlink_metadata(path).is_err() {
            return Ok(());
        }
        let mut tags = mac::read(path)?;
        let before = tags.len();
        tags.retain(|t| tag_name(t) != name);
        if tags.len() == before {
            return Ok(());
        }
        mac::write(path, &tags)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, name);
        Ok(())
    }
}

/// The Finder tags on `path`, each as stored (`Name` or `Name\nColour`); always empty on
/// other systems.
///
/// # Errors
///
/// As [`add_tag`].
pub fn tags(path: &Path) -> std::io::Result<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        mac::read(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Ok(Vec::new())
    }
}

/// A stored tag's name, without its colour.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn tag_name(tag: &str) -> &str {
    tag.split_once('\n').map_or(tag, |(name, _)| name)
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{Error, ErrorKind};
    use std::path::Path;
    use std::process::{Command, Stdio};

    const ATTRIBUTE: &str = "com.apple.metadata:_kMDItemUserTags";
    const XATTR: &str = "/usr/bin/xattr";

    /// The tags on `path`; none if it has no tag attribute.
    pub(super) fn read(path: &Path) -> std::io::Result<Vec<String>> {
        let output = Command::new(XATTR)
            .args(["-px", ATTRIBUTE])
            .arg(path)
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such xattr") {
                return Ok(Vec::new());
            }
            return Err(Error::other(stderr.trim().to_owned()));
        }
        let bytes = from_hex(&String::from_utf8_lossy(&output.stdout))
            .ok_or_else(|| invalid("xattr printed something that isn't hexadecimal"))?;
        super::plist::decode(&bytes).ok_or_else(|| invalid("the Finder tags aren't a list"))
    }

    /// Sets the tags on `path`, removing the attribute when there are none.
    pub(super) fn write(path: &Path, tags: &[String]) -> std::io::Result<()> {
        let mut command = Command::new(XATTR);
        if tags.is_empty() {
            command.args(["-d", ATTRIBUTE]);
        } else {
            command
                .args(["-wx", ATTRIBUTE])
                .arg(to_hex(&super::plist::encode(tags)));
        }
        let output = command.arg(path).stdin(Stdio::null()).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }

    fn invalid(why: &str) -> Error {
        Error::new(ErrorKind::InvalidData, why)
    }

    fn to_hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02X}");
            hex
        })
    }

    fn from_hex(text: &str) -> Option<Vec<u8>> {
        let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if !digits.len().is_multiple_of(2) {
            return None;
        }
        digits
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
            .collect()
    }
}

/// The little of the binary property list format (`bplist00`) Finder tags need: an array of
/// strings.
#[cfg(target_os = "macos")]
mod plist {
    /// An array of strings as a binary property list.
    pub(super) fn encode(strings: &[String]) -> Vec<u8> {
        let count = strings.len() + 1;
        let ref_size: usize = if count < 0x100 { 1 } else { 2 };
        let mut out = b"bplist00".to_vec();
        let mut offsets = vec![out.len()];
        marker(&mut out, 0xA0, strings.len());
        for i in 1..count {
            push_int(&mut out, i as u64, ref_size);
        }
        for s in strings {
            offsets.push(out.len());
            if s.is_ascii() {
                marker(&mut out, 0x50, s.len());
                out.extend_from_slice(s.as_bytes());
            } else {
                let units: Vec<u16> = s.encode_utf16().collect();
                marker(&mut out, 0x60, units.len());
                for unit in units {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
            }
        }
        let table = out.len();
        let offset_size = int_size(table as u64);
        for offset in offsets {
            push_int(&mut out, offset as u64, offset_size);
        }
        out.extend_from_slice(&[0; 6]);
        out.push(u8::try_from(offset_size).unwrap_or(8));
        out.push(u8::try_from(ref_size).unwrap_or(2));
        out.extend_from_slice(&(count as u64).to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&(table as u64).to_be_bytes());
        out
    }

    /// The strings in a binary property list whose top object is an array of strings, or
    /// `None` for anything else.
    pub(super) fn decode(bytes: &[u8]) -> Option<Vec<String>> {
        if !bytes.starts_with(b"bplist00") || bytes.len() < 8 + 32 {
            return None;
        }
        let trailer = &bytes[bytes.len() - 32..];
        let offset_size = usize::from(trailer[6]);
        let ref_size = usize::from(trailer[7]);
        let count = usize::try_from(read_int(&trailer[8..16])?).ok()?;
        let top = usize::try_from(read_int(&trailer[16..24])?).ok()?;
        let table = usize::try_from(read_int(&trailer[24..32])?).ok()?;
        let offset = |i: usize| -> Option<usize> {
            let at = table.checked_add(i.checked_mul(offset_size)?)?;
            usize::try_from(read_int(bytes.get(at..at + offset_size)?)?).ok()
        };
        if top >= count {
            return None;
        }
        let at = offset(top)?;
        let (kind, len, body) = object(bytes, at)?;
        if kind != 0xA0 {
            return None;
        }
        let mut strings = Vec::with_capacity(len);
        for i in 0..len {
            let r = body.checked_add(i.checked_mul(ref_size)?)?;
            let index = usize::try_from(read_int(bytes.get(r..r + ref_size)?)?).ok()?;
            if index >= count {
                return None;
            }
            let (kind, len, body) = object(bytes, offset(index)?)?;
            let text = match kind {
                0x50 => {
                    String::from_utf8(bytes.get(body..body.checked_add(len)?)?.to_vec()).ok()?
                }
                0x60 => {
                    let raw = bytes.get(body..body.checked_add(len.checked_mul(2)?)?)?;
                    let units: Vec<u16> = raw
                        .chunks(2)
                        .map(|p| u16::from_be_bytes([p[0], p[1]]))
                        .collect();
                    String::from_utf16(&units).ok()?
                }
                _ => return None,
            };
            strings.push(text);
        }
        Some(strings)
    }

    /// An object's type (the high four bits of its marker), length, and where its contents
    /// start.
    fn object(bytes: &[u8], at: usize) -> Option<(u8, usize, usize)> {
        let marker = *bytes.get(at)?;
        let kind = marker & 0xF0;
        let short = usize::from(marker & 0x0F);
        if short < 0x0F {
            return Some((kind, short, at + 1));
        }
        // A longer length follows as an integer object: 0x1n, then 2^n bytes.
        let int = *bytes.get(at + 1)?;
        if int & 0xF0 != 0x10 {
            return None;
        }
        let size = 1usize << (int & 0x0F);
        let len = usize::try_from(read_int(bytes.get(at + 2..at + 2 + size)?)?).ok()?;
        Some((kind, len, at + 2 + size))
    }

    fn marker(out: &mut Vec<u8>, kind: u8, len: usize) {
        if len < 0x0F {
            out.push(kind | u8::try_from(len).unwrap_or(0));
        } else {
            out.push(kind | 0x0F);
            let size = int_size(len as u64);
            out.push(0x10 | u8::try_from(size.trailing_zeros()).unwrap_or(3));
            push_int(out, len as u64, size);
        }
    }

    /// The fewest bytes, of 1, 2, 4 or 8, that hold `n`.
    fn int_size(n: u64) -> usize {
        match n {
            0..=0xFF => 1,
            0x100..=0xFFFF => 2,
            0x1_0000..=0xFFFF_FFFF => 4,
            _ => 8,
        }
    }

    fn push_int(out: &mut Vec<u8>, n: u64, size: usize) {
        out.extend_from_slice(&n.to_be_bytes()[8 - size..]);
    }

    fn read_int(bytes: &[u8]) -> Option<u64> {
        if bytes.is_empty() || bytes.len() > 8 {
            return None;
        }
        Some(bytes.iter().fold(0, |n, b| n << 8 | u64::from(*b)))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn tag_lists_round_trip() {
        let long = "a tag name longer than fifteen characters".to_owned();
        let many: Vec<String> = (0..300).map(|i| format!("tag {i}")).collect();
        for tags in [
            vec![],
            vec!["Red\n6".to_owned(), "Checked Out".to_owned()],
            vec![long, "Brouillon été\n2".to_owned()],
            many,
        ] {
            assert_eq!(plist::decode(&plist::encode(&tags)), Some(tags.clone()));

            // macOS's own reader agrees.
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("tags.plist");
            std::fs::write(&file, plist::encode(&tags)).unwrap();
            let json = std::process::Command::new("/usr/bin/plutil")
                .args(["-convert", "json", "-o", "-"])
                .arg(&file)
                .output()
                .unwrap();
            assert!(json.status.success(), "{json:?}");
            let read: Vec<String> = serde_json::from_slice(&json.stdout).unwrap();
            assert_eq!(read, tags);
        }
        assert_eq!(
            plist::decode(b"not a plist at all, not even close....."),
            None
        );
    }

    #[test]
    fn reads_tags_finder_wrote() {
        // `plutil -convert binary1` of ["Work\n4", "Café"], as Finder stores them.
        let bytes = [
            0x62, 0x70, 0x6C, 0x69, 0x73, 0x74, 0x30, 0x30, 0xA2, 0x01, 0x02, 0x56, 0x57, 0x6F,
            0x72, 0x6B, 0x0A, 0x34, 0x64, 0x00, 0x43, 0x00, 0x61, 0x00, 0x66, 0x00, 0xE9, 0x08,
            0x0B, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x1B,
        ];
        assert_eq!(
            plist::decode(&bytes),
            Some(vec!["Work\n4".to_owned(), "Café".to_owned()])
        );
    }

    #[test]
    fn the_tag_is_added_and_removed_and_others_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "x").unwrap();
        let folder = dir.path().join("project");
        std::fs::create_dir(&folder).unwrap();

        for path in [&file, &folder] {
            assert_eq!(tags(path).unwrap(), Vec::<String>::new());
            add_tag(path, CHECKED_OUT).unwrap();
            assert_eq!(tags(path).unwrap(), [CHECKED_OUT]);
            remove_tag(path, CHECKED_OUT).unwrap();
            assert_eq!(tags(path).unwrap(), Vec::<String>::new());

            add_tag(path, "Red\n6").unwrap();
            add_tag(path, CHECKED_OUT).unwrap();
            add_tag(path, CHECKED_OUT).unwrap();
            assert_eq!(tags(path).unwrap(), ["Red\n6", CHECKED_OUT]);
            remove_tag(path, CHECKED_OUT).unwrap();
            assert_eq!(tags(path).unwrap(), ["Red\n6"]);
        }
        remove_tag(&dir.path().join("gone"), CHECKED_OUT).unwrap();
    }
}
