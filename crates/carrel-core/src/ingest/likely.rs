//! Likely duplicates (`docs/design/duplicates.md`): an item that isn't byte-for-byte the same
//! as an archived one, but has the same file name, looks like another version of it, or was
//! drafted with the same title and date.

use std::cmp::Ordering;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::card::Id;

/// At most this many archived items are listed for one candidate.
pub const MAX_LIKELY: usize = 3;

/// Why an item looks like a duplicate of an archived one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// The file names match once copy marks are removed.
    SameName,
    /// The names match apart from a version marker, and this one's is later.
    NewerVersion,
    /// The names match apart from a version marker, and this one's is earlier.
    OlderVersion,
    /// The drafted title and date match the archived card's.
    SameTitleAndDate,
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SameName => "same file name",
            Self::NewerVersion => "looks like a newer version",
            Self::OlderVersion => "looks like an older version",
            Self::SameTitleAndDate => "same title and date",
        })
    }
}

/// An archived item a candidate is likely a duplicate of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Likely {
    /// The archived item's identifier.
    pub id: Id,
    /// Its card's path in the archive.
    pub card: String,
    /// Why it matched.
    pub why: Vec<Why>,
}

impl Likely {
    /// Whether it only looks like a newer version: the one case filed as a version by default.
    #[must_use]
    pub fn is_newer_version(&self) -> bool {
        self.why == [Why::NewerVersion]
    }
}

/// A version marker at the end of a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    /// `v2`, `version 2`, `rev 4`, `r4` or `draft 2`.
    Number(u64),
    /// A trailing date, as `YYYYMMDD`.
    Date(String),
    /// `final`, later than any number.
    Final,
}

impl Marker {
    /// How two markers compare, when they can be compared at all.
    fn order(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Some(a.cmp(b)),
            (Self::Date(a), Self::Date(b)) => Some(a.cmp(b)),
            (Self::Final, Self::Final) => Some(Ordering::Equal),
            (Self::Final, Self::Number(_)) => Some(Ordering::Greater),
            (Self::Number(_), Self::Final) => Some(Ordering::Less),
            _ => None,
        }
    }
}

/// A file name taken apart for comparison: copy marks removed, the version marker split off,
/// and everything lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    /// The name without its extension, copy marks or version marker.
    pub base: String,
    /// The extension, without the dot.
    pub extension: String,
    /// The version marker, if any.
    pub marker: Option<Marker>,
}

static COPY_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\s*\(\d+\)|\s+copy(?:\s+\d+)?)$").expect("valid regex"));
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s_.-])(?:v|version\s*|rev\s*|r|draft\s*)(\d{1,4})$").expect("valid regex")
});
static FINAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s_.-])final$").expect("valid regex"));
static DATED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s_.-])(\d{4})-?(\d{2})-?(\d{2})$").expect("valid regex"));

impl Name {
    /// Takes `file_name` apart.
    #[must_use]
    pub fn parse(file_name: &str) -> Self {
        let lower = file_name.trim().to_lowercase();
        let (stem, extension) = match lower.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && !ext.contains(' ') => (stem, ext),
            _ => (lower.as_str(), ""),
        };
        let mut stem = stem.trim_end().to_owned();
        while let Some(m) = COPY_MARK.find(&stem) {
            stem.truncate(m.start());
        }
        let (rest, marker) = if let Some(c) = NUMBERED.captures(&stem) {
            let number = c[1].parse().ok().map(Marker::Number);
            (&stem[..c.get(0).map_or(0, |m| m.start())], number)
        } else if let Some(m) = FINAL.find(&stem) {
            (&stem[..m.start()], Some(Marker::Final))
        } else if let Some(c) = DATED.captures(&stem) {
            let date = format!("{}{}{}", &c[1], &c[2], &c[3]);
            (
                &stem[..c.get(0).map_or(0, |m| m.start())],
                Some(Marker::Date(date)),
            )
        } else {
            (stem.as_str(), None)
        };
        let base = rest.trim_end_matches([' ', '_', '.', '-']);
        // A name that's only a marker, such as `v2.pdf`, keeps it as its base.
        let (base, marker) = if base.is_empty() {
            (stem.clone(), None)
        } else {
            (base.to_owned(), marker)
        };
        Self {
            base,
            extension: extension.to_owned(),
            marker,
        }
    }

    /// How a new item named `self` relates to an archived one named `archived`, if their
    /// names match.
    #[must_use]
    pub fn compare(&self, archived: &Self) -> Option<Why> {
        if self.base != archived.base || self.extension != archived.extension {
            return None;
        }
        Some(match (&self.marker, &archived.marker) {
            (None, None) => Why::SameName,
            (Some(_), None) => Why::NewerVersion,
            (None, Some(_)) => Why::OlderVersion,
            (Some(new), Some(old)) => match new.order(old) {
                Some(Ordering::Greater) => Why::NewerVersion,
                Some(Ordering::Less) => Why::OlderVersion,
                Some(Ordering::Equal) | None => Why::SameName,
            },
        })
    }
}

/// A title reduced for comparison: lowercase letters and digits, single spaces.
#[must_use]
pub fn title_key(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Adds `why` for the archived item `id` to `found`, keeping the list in order and short.
pub fn add(found: &mut Vec<Likely>, id: Id, card: &str, why: Why) {
    if let Some(existing) = found.iter_mut().find(|l| l.id == id) {
        if !existing.why.contains(&why) {
            existing.why.push(why);
        }
        return;
    }
    if found.len() < MAX_LIKELY {
        found.push(Likely {
            id,
            card: card.to_owned(),
            why: vec![why],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn why(new: &str, old: &str) -> Option<Why> {
        Name::parse(new).compare(&Name::parse(old))
    }

    #[test]
    fn copy_marks_and_case_are_ignored() {
        assert_eq!(why("Report (1).PDF", "report.pdf"), Some(Why::SameName));
        assert_eq!(why("report copy.pdf", "report.pdf"), Some(Why::SameName));
        assert_eq!(why("report copy 2.pdf", "Report.pdf"), Some(Why::SameName));
        assert_eq!(
            why("report (2) copy.pdf", "report.pdf"),
            Some(Why::SameName)
        );
        assert_eq!(why("report.pdf", "report.docx"), None);
        assert_eq!(
            why("report-1.pdf", "report.pdf"),
            None,
            "-1 is often a real number"
        );
        assert_eq!(why("chapter 1.pdf", "chapter 2.pdf"), None);
    }

    #[test]
    fn version_markers_order_versions() {
        assert_eq!(
            why("thesis-v3.docx", "thesis-v2.docx"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("thesis-v2.docx", "thesis-v3.docx"),
            Some(Why::OlderVersion)
        );
        assert_eq!(
            why("thesis v10.docx", "thesis_v9.docx"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("Plan rev 4.pdf", "plan r3.pdf"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("plan draft 2.pdf", "plan version 2.pdf"),
            Some(Why::SameName)
        );
        assert_eq!(
            why("thesis final.docx", "thesis-v7.docx"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("thesis-v2.docx", "thesis.docx"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("thesis.docx", "thesis-v2.docx"),
            Some(Why::OlderVersion)
        );
        assert_eq!(
            why("statement 2026-10-30.pdf", "statement_20260930.pdf"),
            Some(Why::NewerVersion)
        );
        assert_eq!(
            why("notes 2026-10-30.md", "notes v2.md"),
            Some(Why::SameName)
        );
    }

    #[test]
    fn a_name_that_is_only_a_marker_is_its_own_base() {
        assert_eq!(Name::parse("v2.pdf").base, "v2");
        assert_eq!(Name::parse("v2.pdf").marker, None);
        assert_eq!(why("v2.pdf", "v3.pdf"), None);
    }

    #[test]
    fn words_ending_in_r_or_v_are_not_markers() {
        assert_eq!(Name::parse("order 66.pdf").marker, None);
        assert_eq!(Name::parse("letter4.pdf").marker, None);
        assert_eq!(Name::parse("report_r4.pdf").marker, Some(Marker::Number(4)));
    }

    #[test]
    fn titles_compare_without_case_or_punctuation() {
        assert_eq!(title_key("Lease — 2026, Renewal!"), "lease 2026 renewal");
        assert_eq!(
            title_key("lease 2026 renewal"),
            title_key("Lease: 2026 Renewal")
        );
    }

    #[test]
    fn matches_merge_and_stay_short() {
        let mut found = Vec::new();
        let ids: Vec<Id> = (0..5).map(|_| Id::new_v7()).collect();
        add(&mut found, ids[0], "a.md", Why::SameName);
        add(&mut found, ids[0], "a.md", Why::SameTitleAndDate);
        add(&mut found, ids[0], "a.md", Why::SameName);
        for id in &ids[1..] {
            add(&mut found, *id, "b.md", Why::SameName);
        }
        assert_eq!(found.len(), MAX_LIKELY);
        assert_eq!(found[0].why, [Why::SameName, Why::SameTitleAndDate]);
    }
}
