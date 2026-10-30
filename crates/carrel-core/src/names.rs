//! Portable names (FORMAT.md §2.5): names that work on macOS, Linux, Windows and exFAT drives,
//! so an archive can move between them. Carrel only creates portable names; existing names
//! that aren't are reported, and renamed only when the owner says so.

use std::collections::BTreeMap;

use crate::archive::{Archive, ArchiveError};

/// Characters Windows and exFAT don't allow in names, besides `/`, `:` and control characters,
/// which no archive name may contain.
const UNPORTABLE: [char; 7] = ['<', '>', '"', '\\', '|', '?', '*'];

/// Names Windows reserves for devices, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// How long a path from the archive root should be, in characters, so the archive fits
/// Windows' 260-character limit in a typical location.
pub const PATH_LIMIT: usize = 200;

/// What keeps a name from being portable.
#[must_use]
pub fn problems(name: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bad: Vec<char> = name
        .chars()
        .filter(|c| UNPORTABLE.contains(c) || *c == ':' || *c == '/' || c.is_control())
        .collect();
    if !bad.is_empty() {
        let mut shown: Vec<String> = bad
            .iter()
            .map(|c| {
                if c.is_control() {
                    format!("U+{:04X}", u32::from(*c))
                } else {
                    c.to_string()
                }
            })
            .collect();
        shown.dedup();
        found.push(format!("contains {}", shown.join(" ")));
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        found.push(format!(
            "{} is a name Windows reserves",
            stem.to_uppercase()
        ));
    }
    if name.ends_with('.') {
        found.push("ends with a dot".into());
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        found.push("starts or ends with a space".into());
    }
    if name.len() > 200 {
        found.push("is longer than 200 bytes".into());
    }
    found
}

/// Whether `name` is portable.
#[must_use]
pub fn is_portable(name: &str) -> bool {
    problems(name).is_empty()
}

/// A portable name close to `name`: characters that aren't allowed become `-`, spaces and dots
/// at the ends are trimmed, a reserved name gets `_` added, and an overlong name is shortened
/// before its extension. A portable name comes back unchanged.
#[must_use]
pub fn portable(name: &str) -> String {
    if is_portable(name) {
        return name.to_owned();
    }
    let mut out: String = name
        .chars()
        .map(|c| {
            if UNPORTABLE.contains(&c) || c == ':' || c == '/' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    out = out
        .trim_matches(' ')
        .trim_end_matches('.')
        .trim_end()
        .to_owned();
    if out.is_empty() {
        out = "untitled".into();
    }
    // Windows reserves a device name before the first dot, whatever follows (`con.txt.md`).
    let first = out.find('.').unwrap_or(out.len());
    if RESERVED
        .iter()
        .any(|r| r.eq_ignore_ascii_case(out[..first].trim_end()))
    {
        out.insert(first, '_');
    }
    let (stem, extension) = match out.rfind('.') {
        Some(dot) if dot > 0 => (out[..dot].to_owned(), out[dot..].to_owned()),
        _ => (out.clone(), String::new()),
    };
    let mut stem = stem;
    while stem.len() + extension.len() > 200 {
        stem.pop();
    }
    format!("{}{extension}", stem.trim_end())
}

/// A name in the archive that isn't portable, or a path that's too long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The path, relative to the archive root.
    pub path: String,
    /// What's wrong.
    pub problems: Vec<String>,
    /// A portable name to rename it to, when renaming would fix it.
    pub suggestion: Option<String>,
}

/// Every collection and item in the archive whose name isn't portable, siblings that differ
/// only by case, and paths longer than [`PATH_LIMIT`].
///
/// # Errors
///
/// [`ArchiveError`] if the archive can't be read.
pub fn check(archive: &Archive) -> Result<Vec<Finding>, ArchiveError> {
    let mut findings = Vec::new();
    let mut paths: Vec<String> = archive.collections()?.into_iter().map(|c| c.path).collect();
    // A card's name follows its item's, so it's the item that's reported; a card-only item
    // has only its card.
    for item in archive.items()? {
        paths.push(item.item.unwrap_or(item.card));
    }
    paths.sort();
    paths.dedup();

    let mut siblings: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for path in &paths {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        siblings
            .entry((parent.to_owned(), name.to_lowercase()))
            .or_default()
            .push(path.clone());
        let mut problems = problems(name);
        if path.chars().count() > PATH_LIMIT {
            problems.push(format!(
                "the path is longer than {PATH_LIMIT} characters, too long for some Windows locations"
            ));
        }
        if !problems.is_empty() {
            let suggestion = Some(portable(name)).filter(|s| s != name);
            findings.push(Finding {
                path: path.clone(),
                problems,
                suggestion,
            });
        }
    }
    for clash in siblings.into_values().filter(|v| v.len() > 1) {
        for path in &clash {
            let others: Vec<&str> = clash
                .iter()
                .filter(|p| *p != path)
                .map(String::as_str)
                .collect();
            findings.push(Finding {
                path: path.clone(),
                problems: vec![format!(
                    "differs only by case from {}, which some file systems can't hold side by side",
                    others.join(", ")
                )],
                suggestion: None,
            });
        }
    }
    findings.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_names_pass_and_others_say_why() {
        for name in [
            "Paper.pdf",
            "notes (2).md",
            "RFC 9562.md",
            "été.txt",
            "_private",
            "CONSOLE.txt",
        ] {
            assert!(is_portable(name), "{name}");
        }
        assert_eq!(problems("a<b>?.txt"), ["contains < > ?"]);
        assert_eq!(problems("con.txt"), ["CON is a name Windows reserves"]);
        assert_eq!(problems("Lpt1"), ["LPT1 is a name Windows reserves"]);
        assert_eq!(problems("draft."), ["ends with a dot"]);
        assert_eq!(problems(" draft"), ["starts or ends with a space"]);
        assert_eq!(problems("tab\there"), ["contains U+0009"]);
    }

    #[test]
    fn suggestions_are_portable_and_close() {
        assert_eq!(portable("Q3: plan?.pdf"), "Q3- plan-.pdf");
        assert_eq!(portable("nul.txt"), "nul_.txt");
        assert_eq!(portable("CON"), "CON_");
        assert_eq!(portable("con.txt.md"), "con_.txt.md");
        assert_eq!(portable("draft. "), "draft");
        assert_eq!(portable("a|b"), "a-b");
        assert_eq!(portable("???"), "---");
        assert_eq!(portable("Paper.pdf"), "Paper.pdf");
        let long = format!("{}.pdf", "x".repeat(300));
        assert_eq!(portable(&long).len(), 200);
        assert_eq!(&portable(&long)[196..], ".pdf");
        for name in ["Q3: plan?.pdf", "nul.txt", " . ", "a\u{7}b"] {
            assert!(is_portable(&portable(name)), "{name}");
        }
    }
}
