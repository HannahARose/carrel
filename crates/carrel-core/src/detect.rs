//! Detecting what kind of item a file or folder is (FORMAT.md §4) and which folders inside an
//! item are regenerable (§4.3), using the rules in `_System/rules/` (§7.5).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card::{Extra, Kind, Sealing};

/// Files and folders whose presence at a folder's top level makes it a hard-sealed bundle.
pub const DEFAULT_MARKERS: [&str; 9] = [
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "Package.swift",
    "*.xcodeproj",
    "*.xcworkspace",
    "*.kicad_pro",
];

/// Folder names treated as regenerable: kept, but left out of backups.
pub const DEFAULT_REGENERABLE: [&str; 9] = [
    "node_modules",
    ".venv",
    "venv",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".ipynb_checkpoints",
    "DerivedData",
];

/// A name pattern where `*` matches any run of characters within a name (FORMAT.md §7.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NamePattern(pub String);

impl NamePattern {
    /// Whether `name` matches. Matching is case-insensitive, like the file systems on Apple
    /// platforms (FORMAT.md §2.5).
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        glob(&self.0.to_lowercase(), &name.to_lowercase())
    }
}

fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// `_System/rules/markers.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerRules {
    /// The marker patterns.
    pub markers: Vec<NamePattern>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `_System/rules/regenerable.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegenerableRules {
    /// The regenerable folder-name patterns.
    pub regenerable: Vec<NamePattern>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The detection rules an archive uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Patterns that make a folder a hard-sealed bundle.
    pub markers: Vec<NamePattern>,
    /// Patterns for regenerable folder names.
    pub regenerable: Vec<NamePattern>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            markers: DEFAULT_MARKERS
                .iter()
                .map(|p| NamePattern((*p).into()))
                .collect(),
            regenerable: DEFAULT_REGENERABLE
                .iter()
                .map(|p| NamePattern((*p).into()))
                .collect(),
        }
    }
}

/// Why rules couldn't be loaded.
#[derive(Debug)]
pub struct RulesError {
    /// The rules file.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for RulesError {}

impl Rules {
    /// Loads the rules from an archive's `_System/rules/`. A missing file means that file's
    /// defaults.
    ///
    /// # Errors
    ///
    /// [`RulesError`] if a rules file exists but can't be read or parsed.
    pub fn load(archive_root: &Path) -> Result<Self, RulesError> {
        let defaults = Self::default();
        let markers = read_yaml::<MarkerRules>(&archive_root.join("_System/rules/markers.yaml"))?
            .map_or(defaults.markers, |r| r.markers);
        let regenerable =
            read_yaml::<RegenerableRules>(&archive_root.join("_System/rules/regenerable.yaml"))?
                .map_or(defaults.regenerable, |r| r.regenerable);
        Ok(Self {
            markers,
            regenerable,
        })
    }

    /// The YAML for a `markers.yaml` holding these rules' markers.
    #[must_use]
    pub fn markers_yaml(&self) -> String {
        flow_list("markers", &self.markers)
    }

    /// The YAML for a `regenerable.yaml` holding these rules' regenerable names.
    #[must_use]
    pub fn regenerable_yaml(&self) -> String {
        flow_list("regenerable", &self.regenerable)
    }

    /// Whether a folder name is regenerable.
    #[must_use]
    pub fn is_regenerable(&self, name: &str) -> bool {
        self.regenerable.iter().any(|p| p.matches(name))
    }

    /// What kind of item `path` is, and why.
    ///
    /// A file (or a symbolic link, which is never followed) is a [`Kind::File`]. A folder is a
    /// hard-sealed [`Kind::Bundle`] if any entry at its top level matches a marker, and an
    /// open [`Kind::Folder`] otherwise. Soft sealing is the owner's choice, never detected.
    ///
    /// # Errors
    ///
    /// Any error reading `path` or listing the folder.
    pub fn detect(&self, path: &Path) -> std::io::Result<Detected> {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.is_dir() {
            return Ok(Detected {
                kind: Kind::File,
                sealed: None,
                markers: Vec::new(),
            });
        }
        let mut markers = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if is_ignored(&name) {
                continue;
            }
            if self.markers.iter().any(|p| p.matches(&name)) {
                markers.push(name);
            }
        }
        markers.sort();
        Ok(if markers.is_empty() {
            Detected {
                kind: Kind::Folder,
                sealed: None,
                markers,
            }
        } else {
            Detected {
                kind: Kind::Bundle,
                sealed: Some(Sealing::Hard),
                markers,
            }
        })
    }

    /// Every regenerable folder inside the item at `root`, relative to it, sorted. A
    /// regenerable folder's own contents aren't searched, and neither are `.git` folders.
    ///
    /// # Errors
    ///
    /// Any error listing a folder.
    pub fn regenerable_folders(&self, root: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                if self.is_regenerable(&name) {
                    found.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
                } else if name != ".git" {
                    stack.push(path);
                }
            }
        }
        found.sort();
        Ok(found)
    }
}

/// The result of [`Rules::detect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// [`Kind::File`], [`Kind::Folder`] or [`Kind::Bundle`].
    pub kind: Kind,
    /// `Some(Sealing::Hard)` for detected bundles.
    pub sealed: Option<Sealing>,
    /// The top-level entries that matched a marker, sorted.
    pub markers: Vec<String>,
}

/// Files programs never treat as items (FORMAT.md §2.8).
#[must_use]
pub fn is_ignored(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name == "Icon\r"
}

fn read_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, RulesError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(RulesError {
                path: path.to_owned(),
                message: e.to_string(),
            });
        }
    };
    serde_saphyr::from_str(&text)
        .map(Some)
        .map_err(|e| RulesError {
            path: path.to_owned(),
            message: e.to_string(),
        })
}

fn flow_list(key: &str, patterns: &[NamePattern]) -> String {
    let items: Vec<String> = patterns
        .iter()
        .map(|p| serde_json::to_string(&p.0).expect("strings serialize"))
        .collect();
    format!("{key}: [{}]\n", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NamePattern {
        NamePattern(s.into())
    }

    #[test]
    fn patterns_match_whole_names() {
        assert!(p("*.kicad_pro").matches("board.kicad_pro"));
        assert!(p("*.kicad_pro").matches(".kicad_pro"));
        assert!(!p("*.kicad_pro").matches("board.kicad_pro.bak"));
        assert!(p("Cargo.toml").matches("cargo.TOML"), "case-insensitive");
        assert!(!p("Cargo.toml").matches("Cargo.toml.orig"));
        assert!(p("a*b*c").matches("aXbYc"));
        assert!(p("a*b*c").matches("abc"));
        assert!(!p("a*b*c").matches("acb"));
        assert!(
            !p("ab*ba").matches("aba"),
            "prefix and suffix can't overlap"
        );
        assert!(p("*").matches("anything"));
        assert!(p("**").matches(""));
    }

    #[test]
    fn default_rules_round_trip_through_yaml() {
        let rules = Rules::default();
        let markers: MarkerRules = serde_saphyr::from_str(&rules.markers_yaml()).unwrap();
        let regenerable: RegenerableRules =
            serde_saphyr::from_str(&rules.regenerable_yaml()).unwrap();
        assert_eq!(markers.markers, rules.markers);
        assert_eq!(regenerable.regenerable, rules.regenerable);
    }
}
