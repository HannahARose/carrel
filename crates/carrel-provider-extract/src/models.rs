//! What CAD and slicer files say about themselves (`docs/design/models-3d.md`): a 3MF's
//! metadata, a G-code file's slicer comments, and the headers of STEP and IGES files, read
//! without parsing any geometry.

mod gcode;
mod iges;
mod step;
mod threemf;

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::providers::{Extraction, Picture, ProviderError};

/// The offer for 3D printing and CAD files.
pub const MODEL_DETAILS: &str = "model-details";

/// The media types it reads.
pub const MEDIA_TYPES: [&str; 4] = ["model/3mf", "text/x-gcode", "model/step", "model/iges"];

/// The most names listed, of parts or objects.
const MAX_NAMES: usize = 40;

/// Reads the file at `path`, of `media_type`.
pub fn read(path: &Path, media_type: &str) -> Result<Extraction, ProviderError> {
    let failed = |e: std::io::Error| ProviderError::Failed(format!("{}: {e}", path.display()));
    let details = match media_type {
        "model/3mf" => threemf::read(path)?,
        "text/x-gcode" => gcode::read(path).map_err(failed)?,
        "model/step" => step::read(path).map_err(failed)?,
        "model/iges" => iges::read(path).map_err(failed)?,
        other => {
            return Err(ProviderError::Unsupported(format!(
                "{MODEL_DETAILS} doesn't read {other}"
            )));
        }
    };
    if details.lines.is_empty() {
        return Err(ProviderError::Failed(format!(
            "{} says nothing about itself",
            path.display()
        )));
    }
    Ok(Extraction {
        text: details.text(),
        method: "embedded".into(),
        pages: None,
        created: details.created,
        located: false,
        pictures: details.pictures,
    })
}

/// What a file says, as labelled lines under a heading.
#[derive(Debug, Default)]
struct Details {
    heading: String,
    lines: Vec<(&'static str, String)>,
    /// When it was exported, as the card's `created`: a date.
    created: Option<String>,
    /// Pictures in the file, such as a 3MF's thumbnail, for an image reader to describe.
    pictures: Vec<Picture>,
}

impl Details {
    fn new(heading: impl Into<String>) -> Self {
        Self {
            heading: heading.into(),
            ..Self::default()
        }
    }

    /// Adds `label: value`, unless the value is empty or a placeholder, or the label is
    /// already there.
    fn add(&mut self, label: &'static str, value: impl AsRef<str>) {
        let value = value.as_ref().trim();
        let placeholder = ["", "none", "unknown", "n/a", "$", "''"]
            .iter()
            .any(|p| value.eq_ignore_ascii_case(p));
        if !placeholder && !self.lines.iter().any(|(l, _)| *l == label) {
            self.lines.push((label, value.to_owned()));
        }
    }

    /// Adds a list of names, the first [`MAX_NAMES`] of them.
    fn add_names(&mut self, label: &'static str, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let mut list = names
            .iter()
            .take(MAX_NAMES)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if names.len() > MAX_NAMES {
            write!(list, ", and {} more", names.len() - MAX_NAMES).unwrap();
        }
        self.add(label, list);
    }

    fn text(&self) -> String {
        let mut text = self.heading.clone();
        for (label, value) in &self.lines {
            write!(text, "\n{label}: {value}").unwrap();
        }
        text
    }
}

/// `name` added to `names` when it's new and not blank.
fn push_unique(names: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if !name.is_empty() && !names.iter().any(|n| n == name) {
        names.push(name.to_owned());
    }
}

/// `5430` seconds as `1h 30m`.
fn duration(seconds: u64) -> String {
    format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
}

/// The date at the start of `text`, such as `2026-10-11` in `2026-10-11T14:22:05`, when it's
/// a real one.
fn date(text: &str) -> Option<String> {
    let day = text.trim().get(..10)?;
    let bytes = day.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let (month, dom) = (day.get(5..7)?, day.get(8..10)?);
    (digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && day[..4] != *"0000"
        && ("01"..="12").contains(&month)
        && ("01"..="31").contains(&dom))
    .then(|| day.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_and_repeats_are_left_out() {
        let mut details = Details::new("CAD model (STEP)");
        details.add("Author", "Unknown");
        details.add("Organisation", "  coherent ");
        details.add("Organisation", "again");
        details.add_names("Parts", &[]);
        assert_eq!(details.text(), "CAD model (STEP)\nOrganisation: coherent");
    }

    #[test]
    fn long_lists_are_cut() {
        let names: Vec<String> = (0..45).map(|i| format!("p{i}")).collect();
        let mut details = Details::new("x");
        details.add_names("Parts", &names);
        assert!(
            details.text().ends_with("p39, and 5 more"),
            "{}",
            details.text()
        );
    }

    #[test]
    fn only_real_dates_are_kept() {
        assert_eq!(date("2026-10-11T14:22:05").as_deref(), Some("2026-10-11"));
        assert_eq!(date("0000-00-00"), None);
        assert_eq!(date("2026-13-01"), None);
        assert_eq!(date("yesterday"), None);
    }
}
