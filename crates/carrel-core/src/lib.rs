//! The Carrel core: reads and writes archives that follow `FORMAT.md`.
//!
//! This crate is shared by the `carrel` CLI and, through `carrel-ffi`, the Apple apps.
//! It has no platform-specific code; anything that needs the operating system (models,
//! text extraction, sync) is supplied by the caller.

pub mod archive;
pub mod card;
pub mod card_file;
pub mod documents;
pub mod frontmatter;
pub mod sections;

/// The version of the archive format this build reads and writes (`FORMAT.md`).
pub const FORMAT_VERSION: FormatVersion = FormatVersion { major: 1, minor: 0 };

/// The version of this crate, which is also the Carrel version.
pub const CARREL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An archive format version, `MAJOR.MINOR` (FORMAT.md §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatVersion {
    /// Changes that may alter meaning. Newer majors are opened read-only.
    pub major: u16,
    /// Additive changes only. Newer minors are opened normally, preserving unknown data.
    pub minor: u16,
}

impl FormatVersion {
    /// Parses a version written as `"MAJOR.MINOR"`, such as `"1.0"`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = text.split_once('.')?;
        Some(Self {
            major: major.parse().ok()?,
            minor: minor.parse().ok()?,
        })
    }

    /// How this build may open an archive written with `self` (FORMAT.md §12).
    #[must_use]
    pub fn access(self) -> Access {
        if self.major > FORMAT_VERSION.major {
            Access::ReadOnly
        } else {
            Access::ReadWrite
        }
    }
}

impl std::fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

// Written as a quoted string ("1.0") so YAML never reads it as a number.
impl serde::Serialize for FormatVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for FormatVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("format version must be MAJOR.MINOR, got {text:?}"))
        })
    }
}

impl schemars::JsonSchema for FormatVersion {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FormatVersion".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9]+\\.[0-9]+$",
            "description": "Archive format version, MAJOR.MINOR (FORMAT.md §12)."
        })
    }
}

/// Whether an archive may be changed by this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The archive's format is understood; it may be read and written.
    ReadWrite,
    /// The archive uses a newer major format version; it must not be modified.
    ReadOnly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays_format_versions() {
        let v = FormatVersion::parse("1.0").unwrap();
        assert_eq!(v, FORMAT_VERSION);
        assert_eq!(v.to_string(), "1.0");
        assert_eq!(
            FormatVersion::parse("2.13"),
            Some(FormatVersion {
                major: 2,
                minor: 13
            })
        );
        for bad in ["", "1", "1.", ".0", "a.b", "1.0.0", "-1.0"] {
            assert_eq!(FormatVersion::parse(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn newer_major_versions_open_read_only() {
        assert_eq!(
            FormatVersion { major: 1, minor: 9 }.access(),
            Access::ReadWrite
        );
        assert_eq!(
            FormatVersion { major: 2, minor: 0 }.access(),
            Access::ReadOnly
        );
    }
}
