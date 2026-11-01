//! A note the owner writes, or shares as text, to be filed as an item of its own: kept as a
//! Markdown file in a folder of its own on this device, then added to the intake queue as any
//! file is, or put in this device's inbox while another device is the writer (FORMAT.md §7.8).
//! The share sheet, the app's Add Note, `carrel ingest --text` and the interactive mode's
//! Ingest all add notes this way.

use std::path::{Path, PathBuf};

use crate::actions::ActionError;
use crate::archive::Archive;
use crate::card::Id;
use crate::coordinator::Holder;
use crate::ingest::IntakeOptions;
use crate::intake::{self, Queued};
use crate::{inbox, local};

/// Where this device keeps written notes until they're filed: `notes` in the configuration
/// folder. `None` if there's no configuration folder.
#[must_use]
pub fn written_notes_dir() -> Option<PathBuf> {
    local::config_dir().map(|dir| dir.join("notes"))
}

/// What adding a written note did.
#[derive(Debug)]
pub enum WrittenNoteAdded {
    /// It joined this device's intake queue, as the file `path`.
    Queued {
        /// The note's file.
        path: PathBuf,
        /// What joined the queue, and what was left out.
        queued: Queued,
    },
    /// Another device is the writer, so it went into this device's inbox instead.
    Sent {
        /// The inbox entry.
        inbox: PathBuf,
        /// The writing device.
        writer: Id,
    },
}

/// The first few words of `text`, to name a note by when it has no title.
#[must_use]
pub fn first_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(8).collect();
    if words.is_empty() {
        "Note".to_owned()
    } else {
        words.join(" ")
    }
}

/// The file name for a note titled `title`, or named by its first words: portable, at most
/// 80 characters, ending in `.md` unless it ends in `.markdown` or `.txt`.
#[must_use]
pub fn written_note_file_name(text: &str, title: Option<&str>) -> String {
    let title = title.map(str::trim).filter(|t| !t.is_empty());
    let name = crate::names::portable(&title.map_or_else(|| first_words(text), str::to_owned));
    let mut name: String = name
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .take(80)
        .collect();
    if name.is_empty() {
        name = "Note".to_owned();
    }
    let lower = name.to_ascii_lowercase();
    if ![".md", ".markdown", ".txt"].iter().any(|e| lower.ends_with(e)) {
        name.push_str(".md");
    }
    name
}

/// Saves `text` as a Markdown file named for `title` (or its first words) in a new folder of
/// its own in `folder`, for the intake queue to take as it takes any file. Returns the file.
///
/// # Errors
///
/// [`ActionError::Invalid`] if `text` is empty, or [`ActionError::Io`] if it can't be saved.
pub fn keep_written_note(
    folder: &Path,
    text: &str,
    title: Option<&str>,
) -> Result<PathBuf, ActionError> {
    if text.trim().is_empty() {
        return Err(ActionError::Invalid("The note is empty.".into()));
    }
    let own = folder.join(Id::new_v7().to_string());
    std::fs::create_dir_all(&own).map_err(|e| ActionError::Io {
        path: own.clone(),
        source: e,
    })?;
    let path = own.join(written_note_file_name(text, title));
    let mut contents = text.trim_end().to_owned();
    contents.push('\n');
    std::fs::write(&path, contents).map_err(|e| ActionError::Io {
        path: path.clone(),
        source: e,
    })?;
    Ok(path)
}

/// Adds `text` as a note item for `holder`, kept in `folder`, with its `title` and the owner's
/// `note` for its draft and card. While another device is the writer, it goes into this
/// device's inbox instead, and the kept copy is removed.
///
/// # Errors
///
/// As [`keep_written_note`], or [`ActionError`] if the queue or inbox can't be written.
pub fn add_written_note(
    archive: &Archive,
    folder: &Path,
    text: &str,
    title: Option<&str>,
    note: Option<&str>,
    holder: Holder,
) -> Result<WrittenNoteAdded, ActionError> {
    let path = keep_written_note(folder, text, title)?;
    if let Some(writer) = inbox::writer_elsewhere(archive, holder.device) {
        let sent = inbox::send_paths(archive, holder, std::slice::from_ref(&path), false, note);
        if let Some(own) = path.parent() {
            let _ = std::fs::remove_dir_all(own);
        }
        return Ok(WrittenNoteAdded::Sent {
            inbox: sent?,
            writer: writer.device,
        });
    }
    let one = IntakeOptions {
        each: false,
        whole: true,
    };
    let queued =
        intake::add_with_note(archive, std::slice::from_ref(&path), one, holder.device, note)?;
    Ok(WrittenNoteAdded::Queued { path, queued })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_notes_are_named_by_their_title_or_first_words() {
        assert_eq!(
            written_note_file_name("Buy more lamp oil before winter", None),
            "Buy more lamp oil before winter.md"
        );
        assert_eq!(
            written_note_file_name("text", Some(" Lamp: oil/wick ")),
            "Lamp- oil-wick.md"
        );
        assert_eq!(written_note_file_name("   ", None), "Note.md");
        assert_eq!(written_note_file_name("x", Some("notes.txt")), "notes.txt");
        assert_eq!(written_note_file_name("x", Some("Dr. Smith")), "Dr. Smith.md");
        let long = "word ".repeat(40);
        assert!(written_note_file_name(&long, Some(&long)).chars().count() <= 83);
    }

    #[test]
    fn an_empty_note_is_refused_and_a_written_one_is_kept_in_a_folder_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            keep_written_note(dir.path(), " \n", None),
            Err(ActionError::Invalid(_))
        ));
        let a = keep_written_note(dir.path(), "Same\n\n", Some("Same")).unwrap();
        let b = keep_written_note(dir.path(), "Same", Some("Same")).unwrap();
        assert_ne!(a.parent(), b.parent());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "Same\n");
    }
}
