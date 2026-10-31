//! Whether a compressed file is expanded or kept, and why (`docs/design/compressed.md`): the
//! plan a proposal shows in Review and filing carries out.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Inside, Limits, Shape};
use crate::card::{Extra, Kind, Sealing};
use crate::detect::{Compressed, Rules};

/// What to do with compressed files: the `compressed_files` setting of the archive and of
/// collections, and what a plan decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    /// Expand it, so what it holds can be browsed.
    #[default]
    Expand,
    /// Keep it compressed, as one file.
    Keep,
    /// Ask the owner in Review.
    Ask,
}

impl Choice {
    /// Its name in settings: `expand`, `keep` or `ask`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expand => "expand",
            Self::Keep => "keep",
            Self::Ask => "ask",
        }
    }

    /// The choice with this name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Expand, Self::Keep, Self::Ask]
            .into_iter()
            .find(|c| c.as_str() == name)
    }
}

/// A proposal's plan for a compressed file: the review entry's `compressed` (FORMAT.md §7.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The format.
    pub format: Compressed,
    /// Whether it's expanded on approval, kept, or waiting for the owner to say.
    pub action: Choice,
    /// Why it's kept or asked about, in words; absent when it's simply expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the owner chose the action in Review, which lifts the size limits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_owner: bool,
    /// How many files expanding makes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    /// Their total size in bytes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// What expanding makes: `file`, `folder` or `bundle`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// For a bundle, how it's sealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Sealing>,
    /// The name of what expanding makes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Plan {
    /// The plan in words for Review, such as `will be expanded: 440 files, 1.8 GB, into
    /// Projects/Lab/Spectrometer export` or `kept compressed: it holds an installer`.
    #[must_use]
    pub fn describe(&self, destination: &str) -> String {
        match self.action {
            Choice::Expand => {
                let mut what = Vec::new();
                if let Some(files) = self.files {
                    what.push(if files == 1 {
                        "1 file".to_owned()
                    } else {
                        format!("{files} files")
                    });
                }
                if let Some(size) = self.size {
                    what.push(crate::human_size(size));
                }
                let into = match &self.name {
                    Some(name) => format!("{destination}/{name}"),
                    None => destination.to_owned(),
                };
                if what.is_empty() {
                    format!("will be expanded into {into}")
                } else {
                    format!("will be expanded: {}, into {into}", what.join(", "))
                }
            }
            Choice::Keep => match &self.reason {
                Some(reason) => format!("kept compressed: {reason}"),
                None => "kept compressed".into(),
            },
            Choice::Ask => format!(
                "expand or keep compressed? {}",
                self.reason.as_deref().unwrap_or("the owner decides")
            ),
        }
    }
}

/// What the owner has said about compressed files, from the most specific to the least.
#[derive(Debug, Clone, Copy, Default)]
pub struct Owner<'a> {
    /// The routing rule that chose the destination, when it has `keep_compressed: true`.
    pub rule: Option<&'a str>,
    /// The destination collection's path and its `compressed_files`, when it has one.
    pub collection: Option<(&'a str, Choice)>,
    /// The archive's `compressed_files`.
    pub archive: Choice,
}

/// The plan for the compressed file at `path`, from what's `inside` it: kept when it's
/// software, encrypted, split or unreadable; then whatever the owner said, rule first; then
/// asked about when expanding looks unsafe or too big; otherwise expanded.
#[must_use]
pub fn plan(path: &Path, inside: &Inside, rules: &Rules, owner: Owner<'_>) -> Plan {
    let mut plan = Plan {
        format: inside.format,
        action: Choice::Expand,
        reason: None,
        by_owner: false,
        files: None,
        size: None,
        kind: None,
        sealed: None,
        name: None,
        extra: Extra::new(),
    };
    if let Some(listing) = &inside.listing {
        plan.files = Some(listing.file_count() as u64);
        plan.size = Some(listing.total_size());
        let stem = path
            .file_name()
            .map(|n| inside.format.stem(&n.to_string_lossy()))
            .unwrap_or_default();
        let (name, kind) = match listing.shape() {
            Shape::OneFile(file) => (base_name(&file), Kind::File),
            Shape::OneFolder(folder) => (base_name(&folder), Kind::Folder),
            Shape::Many | Shape::Empty => (stem, Kind::Folder),
        };
        plan.name = Some(name);
        plan.kind = Some(kind);
        if kind == Kind::Folder && !inside.markers(rules).is_empty() {
            plan.kind = Some(Kind::Bundle);
            plan.sealed = Some(Sealing::Hard);
        }
    }
    let (action, reason) = decide(inside, owner);
    plan.action = action;
    plan.reason = reason;
    plan
}

fn base_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// The action and its reason, in the order the design gives.
fn decide(inside: &Inside, owner: Owner<'_>) -> (Choice, Option<String>) {
    if let Some(reason) = automatic(inside) {
        return (Choice::Keep, Some(reason));
    }
    if let Some(rule) = owner.rule {
        return (
            Choice::Keep,
            Some(format!(
                "the routing rule \u{201c}{rule}\u{201d} keeps it compressed"
            )),
        );
    }
    let said = match owner.collection {
        Some((path, choice)) => Some((choice, format!("{path}'s setting"))),
        None if owner.archive != Choice::Expand => {
            Some((owner.archive, "the archive's setting".to_owned()))
        }
        None => None,
    };
    match said {
        Some((Choice::Keep, whose)) => {
            return (
                Choice::Keep,
                Some(format!("{whose} keeps compressed files")),
            );
        }
        Some((Choice::Ask, whose)) => {
            return (Choice::Ask, Some(format!("{whose} asks about each one")));
        }
        _ => {}
    }
    let listing = inside.listing.as_ref().expect("automatic() covers unread");
    if let Some(why) = listing.unsafe_entry() {
        return (Choice::Ask, Some(format!("expanding looks unsafe: {why}")));
    }
    if let Some(why) = listing.too_big(&Limits::DEFAULT) {
        return (Choice::Ask, Some(format!("expanding looks too big: {why}")));
    }
    (Choice::Expand, None)
}

/// Why it's kept compressed whatever the settings say: it's split, unreadable, encrypted, or
/// software to install or run.
#[must_use]
pub fn automatic(inside: &Inside) -> Option<String> {
    if inside.split {
        return Some("it's split into parts".into());
    }
    let Some(listing) = &inside.listing else {
        return Some(
            inside
                .unread
                .clone()
                .unwrap_or_else(|| "it couldn't be read".into()),
        );
    };
    if listing.encrypted() {
        return Some("it's encrypted".into());
    }
    software(listing)
}

/// Installer files, by extension.
const INSTALLERS: [&str; 5] = ["pkg", "mpkg", "msi", "exe", "dmg"];

/// What makes it software, if anything: an app, an installer, or a code signature.
fn software(listing: &super::Listing) -> Option<String> {
    for entry in &listing.entries {
        for name in entry.path.split('/').filter(|n| !n.is_empty()) {
            let lower = name.to_lowercase();
            let extension = lower.rsplit_once('.').map(|(_, e)| e);
            if extension == Some("app") {
                return Some(format!("it holds an app ({name})"));
            }
            if extension.is_some_and(|e| INSTALLERS.contains(&e)) {
                return Some(format!("it holds an installer ({name})"));
            }
            if name == "_CodeSignature" {
                return Some("it holds signed software".into());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compressed::{Entry, EntryKind, Listing};

    fn inside(paths: &[&str]) -> Inside {
        Inside {
            format: Compressed::Zip,
            listing: Some(Listing {
                format: Compressed::Zip,
                compressed_size: 100,
                entries: paths
                    .iter()
                    .map(|p| Entry {
                        path: (*p).into(),
                        kind: if p.ends_with('/') {
                            EntryKind::Folder
                        } else {
                            EntryKind::File
                        },
                        size: Some(10),
                        modified: None,
                        executable: false,
                        crc32: None,
                        encrypted: false,
                    })
                    .collect(),
                truncated: false,
            }),
            unread: None,
            no_reader: false,
            split: false,
        }
    }

    fn plan_of(paths: &[&str], owner: Owner<'_>) -> Plan {
        plan(
            Path::new("/in/Export.zip"),
            &inside(paths),
            &Rules::default(),
            owner,
        )
    }

    #[test]
    fn plain_archives_are_expanded_and_unwrapped() {
        let wrapped = plan_of(&["Export/a.csv", "Export/b.csv"], Owner::default());
        assert_eq!(wrapped.action, Choice::Expand);
        assert_eq!(wrapped.name.as_deref(), Some("Export"));
        assert_eq!(
            (wrapped.kind, wrapped.files, wrapped.size),
            (Some(Kind::Folder), Some(2), Some(20))
        );
        assert_eq!(
            wrapped.describe("Projects/Lab"),
            "will be expanded: 2 files, 20 B, into Projects/Lab/Export"
        );
        let project = plan_of(&["a/Cargo.toml", "a/src/main.rs"], Owner::default());
        assert_eq!(
            (project.kind, project.sealed),
            (Some(Kind::Bundle), Some(Sealing::Hard))
        );
        let one = plan_of(&["report.pdf"], Owner::default());
        assert_eq!(
            (one.name.as_deref(), one.kind),
            (Some("report.pdf"), Some(Kind::File))
        );
        let loose = plan_of(&["a.csv", "b.csv"], Owner::default());
        assert_eq!(loose.name.as_deref(), Some("Export"));
    }

    #[test]
    fn software_stays_compressed_whatever_the_settings() {
        for (paths, why) in [
            (&["Tool.app/Contents/MacOS/Tool"][..], "an app (Tool.app)"),
            (&["Setup.EXE"][..], "an installer (Setup.EXE)"),
            (&["Driver/Install.pkg"][..], "an installer (Install.pkg)"),
            (&["bin/_CodeSignature/CodeResources"][..], "signed software"),
        ] {
            let plan = plan_of(paths, Owner::default());
            assert_eq!(plan.action, Choice::Keep, "{paths:?}");
            assert!(plan.reason.unwrap().contains(why), "{paths:?}");
        }
    }

    #[test]
    fn the_owner_decides_rule_first_then_collection_then_archive() {
        let paths = &["Export/a.csv"][..];
        let rule = plan_of(
            paths,
            Owner {
                rule: Some("Backups"),
                ..Owner::default()
            },
        );
        assert_eq!(rule.action, Choice::Keep);
        assert!(rule.reason.unwrap().contains("\u{201c}Backups\u{201d}"));
        let collection = plan_of(
            paths,
            Owner {
                collection: Some(("Software", Choice::Keep)),
                archive: Choice::Ask,
                ..Owner::default()
            },
        );
        assert_eq!(
            collection.reason.as_deref(),
            Some("Software's setting keeps compressed files")
        );
        let expand_here = plan_of(
            paths,
            Owner {
                collection: Some(("Lab", Choice::Expand)),
                archive: Choice::Keep,
                ..Owner::default()
            },
        );
        assert_eq!(
            expand_here.action,
            Choice::Expand,
            "the collection overrides the archive"
        );
        let ask = plan_of(
            paths,
            Owner {
                archive: Choice::Ask,
                ..Owner::default()
            },
        );
        assert_eq!(ask.action, Choice::Ask);
        assert!(ask.describe("X").starts_with("expand or keep compressed?"));
    }

    #[test]
    fn unsafe_or_unreadable_archives_ask_or_stay() {
        let unsafe_plan = plan_of(&["../up.txt"], Owner::default());
        assert_eq!(unsafe_plan.action, Choice::Ask);
        assert!(unsafe_plan.reason.unwrap().contains("unsafe"));
        let mut split = inside(&[]);
        split.split = true;
        let plan = plan(
            Path::new("/a.zip"),
            &split,
            &Rules::default(),
            Owner::default(),
        );
        assert_eq!(plan.describe("X"), "kept compressed: it's split into parts");
        let mut encrypted = inside(&["a.txt"]);
        encrypted.listing.as_mut().unwrap().entries[0].encrypted = true;
        assert_eq!(automatic(&encrypted).as_deref(), Some("it's encrypted"));
        let unread = Inside {
            listing: None,
            unread: Some("nothing on this device opens 7z files".into()),
            no_reader: true,
            ..inside(&[])
        };
        assert_eq!(
            automatic(&unread).as_deref(),
            Some("nothing on this device opens 7z files")
        );
    }
}
