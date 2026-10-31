//! Managed sections (FORMAT.md §5.3): the only parts of a Markdown body programs may write.
//!
//! A managed section is delimited by marker lines:
//!
//! ```markdown
//! <!-- carrel:history -->
//! ## History
//! <!-- /carrel:history -->
//! ```
//!
//! Markers only count when they are alone on their line and outside fenced code blocks, so a
//! card can show a marker as an example without creating a section. Anything ambiguous
//! (an unclosed section, a stray end marker, a repeated or nested section) is an error:
//! programs report it instead of guessing.

use std::ops::Range;

/// Why the managed sections in a body can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionError {
    /// A start marker has no matching end marker.
    Unclosed(String),
    /// An end marker has no matching start marker.
    UnexpectedEnd(String),
    /// A section starts again before it has ended, or another section starts inside it.
    Nested {
        /// The section that was open.
        outer: String,
        /// The section that started inside it.
        inner: String,
    },
    /// The same section appears more than once.
    Duplicate(String),
    /// The section doesn't exist; use [`append_section`] to create it.
    Missing(String),
    /// A section name contains characters other than lowercase letters, digits and `-`.
    BadName(String),
}

impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unclosed(n) => write!(f, "managed section `{n}` is never closed"),
            Self::UnexpectedEnd(n) => write!(f, "end marker for `{n}` without a start marker"),
            Self::Nested { outer, inner } => {
                write!(f, "managed section `{inner}` starts inside `{outer}`")
            }
            Self::Duplicate(n) => write!(f, "managed section `{n}` appears more than once"),
            Self::Missing(n) => write!(f, "managed section `{n}` doesn't exist"),
            Self::BadName(n) => write!(f, "`{n}` isn't a valid section name"),
        }
    }
}

impl std::error::Error for SectionError {}

/// One managed section's position in a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The section's name, such as `history`.
    pub name: String,
    /// Byte range of the content between the marker lines (excluding both markers).
    pub content: Range<usize>,
    /// Byte range from the start of the start marker to the end of the end marker line.
    pub whole: Range<usize>,
}

/// Whether `name` is a valid section name: lowercase letters, digits and `-`, not empty.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A managed-section marker line, with the section name it opens or closes.
enum Marker<'a> {
    Start(&'a str),
    End(&'a str),
}

/// The marker on `line`, if the line is only a marker (surrounding spaces allowed).
fn marker(line: &str) -> Option<Marker<'_>> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    if let Some(name) = inner.strip_prefix("/carrel:") {
        valid_name(name).then_some(Marker::End(name))
    } else {
        let name = inner.strip_prefix("carrel:")?;
        valid_name(name).then_some(Marker::Start(name))
    }
}

/// Tracks fenced code blocks (as in `CommonMark`: up to three spaces, then three or more backticks
/// or tildes).
#[derive(Default)]
struct Fences {
    open: Option<(u8, usize)>,
}

impl Fences {
    /// Feeds one line; returns whether it is inside (or delimits) a fenced code block.
    fn in_code(&mut self, line: &str) -> bool {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let indent = trimmed.len() - trimmed.trim_start_matches(' ').len();
        let rest = &trimmed[indent..];
        let fence = (indent <= 3)
            .then(|| {
                let ch = *rest.as_bytes().first()?;
                (ch == b'`' || ch == b'~').then_some(())?;
                let run = rest.bytes().take_while(|&b| b == ch).count();
                (run >= 3).then_some((ch, run, &rest[run..]))
            })
            .flatten();
        match (self.open, fence) {
            (None, Some((ch, run, info))) => {
                // A backtick fence's info string can't contain backticks.
                if ch == b'`' && info.contains('`') {
                    return false;
                }
                self.open = Some((ch, run));
                true
            }
            (Some((ch, run)), Some((c, r, after)))
                if c == ch && r >= run && after.trim().is_empty() =>
            {
                self.open = None;
                true
            }
            (Some(_), _) => true,
            (None, None) => false,
        }
    }
}

/// Whether `text` can sit inside a managed section without changing where sections start and
/// end: no line of it is a marker, and every fenced code block it opens is closed.
#[must_use]
pub fn safe_inside(text: &str) -> bool {
    let mut fences = Fences::default();
    for line in text.lines() {
        if !fences.in_code(line) && marker(line).is_some() {
            return false;
        }
    }
    fences.open.is_none()
}

/// Finds every managed section in `body`, in order.
///
/// # Errors
///
/// Any of [`SectionError`]'s structural errors if the markers don't pair up cleanly.
pub fn find_all(body: &str) -> Result<Vec<Section>, SectionError> {
    let mut sections: Vec<Section> = Vec::new();
    let mut open: Option<(&str, usize, usize)> = None; // name, start of marker, start of content
    let mut fences = Fences::default();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if fences.in_code(line) {
            continue;
        }
        match (marker(line), open) {
            (Some(Marker::Start(name)), None) => {
                if sections.iter().any(|s| s.name == name) {
                    return Err(SectionError::Duplicate(name.into()));
                }
                open = Some((name, start, offset));
            }
            (Some(Marker::Start(inner)), Some((outer, ..))) => {
                return Err(SectionError::Nested {
                    outer: outer.into(),
                    inner: inner.into(),
                });
            }
            (Some(Marker::End(name)), Some((open_name, whole_start, content_start)))
                if name == open_name =>
            {
                sections.push(Section {
                    name: name.into(),
                    content: content_start..start,
                    whole: whole_start..offset,
                });
                open = None;
            }
            (Some(Marker::End(name)), _) => return Err(SectionError::UnexpectedEnd(name.into())),
            (None, _) => {}
        }
    }
    match open {
        Some((name, ..)) => Err(SectionError::Unclosed(name.into())),
        None => Ok(sections),
    }
}

/// The content of section `name`, if it exists.
///
/// # Errors
///
/// A structural [`SectionError`] if the body's markers don't pair up cleanly.
pub fn get<'a>(body: &'a str, name: &str) -> Result<Option<&'a str>, SectionError> {
    Ok(find_all(body)?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| &body[s.content]))
}

/// Replaces the content of an existing section. Only the bytes between its markers change.
/// `content` gets a trailing newline if it is non-empty and lacks one.
///
/// ```
/// use carrel_core::sections;
///
/// let body = "Notes.\n<!-- carrel:history -->\nold\n<!-- /carrel:history -->\nMore.\n";
/// let updated = sections::replace(body, "history", "new")?;
/// assert_eq!(updated, "Notes.\n<!-- carrel:history -->\nnew\n<!-- /carrel:history -->\nMore.\n");
/// # Ok::<(), sections::SectionError>(())
/// ```
///
/// # Errors
///
/// [`SectionError::Missing`] if the section doesn't exist, or a structural error.
pub fn replace(body: &str, name: &str, content: &str) -> Result<String, SectionError> {
    let section = find_all(body)?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| SectionError::Missing(name.into()))?;
    let mut out = String::with_capacity(body.len() + content.len());
    out.push_str(&body[..section.content.start]);
    push_content(&mut out, content);
    out.push_str(&body[section.content.end..]);
    Ok(out)
}

/// Appends a new section at the end of `body`, for creating cards and documents. Existing
/// text is unchanged; at most a newline is added so the section starts on its own line,
/// preceded by a blank line.
///
/// # Errors
///
/// [`SectionError::Duplicate`] if the section already exists, [`SectionError::BadName`] for
/// an invalid name, or a structural error.
pub fn append_section(body: &str, name: &str, content: &str) -> Result<String, SectionError> {
    if !valid_name(name) {
        return Err(SectionError::BadName(name.into()));
    }
    if find_all(body)?.iter().any(|s| s.name == name) {
        return Err(SectionError::Duplicate(name.into()));
    }
    let mut out = String::from(body);
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str("<!-- carrel:");
    out.push_str(name);
    out.push_str(" -->\n");
    push_content(&mut out, content);
    out.push_str("<!-- /carrel:");
    out.push_str(name);
    out.push_str(" -->\n");
    Ok(out)
}

/// Appends section content, adding a final newline if it is non-empty and lacks one, so the
/// end marker always starts its own line.
fn push_content(out: &mut String, content: &str) {
    out.push_str(content);
    if !content.is_empty() && !content.ends_with('\n') {
        out.push('\n');
    }
}

/// Rewrites Markdown table rows with canonical padding, so two texts that differ only in how
/// an editor aligned their tables compare equal (FORMAT.md §5.3). Other lines are unchanged.
#[must_use]
pub fn normalize_tables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fences = Fences::default();
    for line in text.split_inclusive('\n') {
        let code = fences.in_code(line);
        let ending = &line[line.trim_end_matches(['\n', '\r']).len()..];
        let row = line.trim_end_matches(['\n', '\r']).trim();
        if code || !(row.starts_with('|') && row.ends_with('|') && row.len() >= 2) {
            out.push_str(line);
            continue;
        }
        let cells: Vec<String> = split_cells(&row[1..row.len() - 1])
            .into_iter()
            .map(|c| canonical_cell(c.trim()))
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |");
        out.push_str(ending);
    }
    out
}

/// Whether two texts are the same apart from table padding.
#[must_use]
pub fn equivalent(a: &str, b: &str) -> bool {
    a == b || normalize_tables(a) == normalize_tables(b)
}

/// Splits the inside of a table row at `|`, except where the `|` is escaped (`\|`) or inside a
/// code span, which are both part of the cell's text in GitHub Flavored Markdown.
fn split_cells(inner: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let (mut start, mut escaped, mut in_code) = (0, false, false);
    for (i, ch) in inner.char_indices() {
        match ch {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '`' => in_code = !in_code,
            '|' if !in_code => {
                cells.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    cells.push(&inner[start..]);
    cells
}

/// A cell with canonical padding. Delimiter cells (only `-` and `:`) become `---`, keeping any
/// `:` that sets the column's alignment; other cells are returned trimmed, unchanged.
fn canonical_cell(cell: &str) -> String {
    let is_delimiter =
        !cell.is_empty() && cell.contains('-') && cell.bytes().all(|b| b == b'-' || b == b':');
    if !is_delimiter {
        return cell.to_owned();
    }
    let (left, right) = (cell.starts_with(':'), cell.len() > 1 && cell.ends_with(':'));
    format!(
        "{}---{}",
        if left { ":" } else { "" },
        if right { ":" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD_BODY: &str = "Owner text.\n\n<!-- carrel:history -->\n## History\n- entry\n<!-- /carrel:history -->\n\nMore owner text.\n";

    #[test]
    fn finds_and_reads_sections() {
        let s = &find_all(CARD_BODY).unwrap()[0];
        assert_eq!(s.name, "history");
        assert_eq!(&CARD_BODY[s.content.clone()], "## History\n- entry\n");
        assert!(CARD_BODY[s.whole.clone()].starts_with("<!-- carrel:history -->"));
        assert!(CARD_BODY[s.whole.clone()].ends_with("<!-- /carrel:history -->\n"));
        assert_eq!(get(CARD_BODY, "listing").unwrap(), None);
    }

    #[test]
    fn replace_changes_only_the_content() {
        let out = replace(CARD_BODY, "history", "## History\n- new").unwrap();
        assert_eq!(
            out,
            CARD_BODY.replace("## History\n- entry\n", "## History\n- new\n")
        );
        assert_eq!(
            replace(CARD_BODY, "listing", "x"),
            Err(SectionError::Missing("listing".into()))
        );
    }

    #[test]
    fn markers_in_code_blocks_are_ignored() {
        let body = "```markdown\n<!-- carrel:history -->\n```\n~~~~\n<!-- /carrel:x -->\n~~~~\n";
        assert_eq!(find_all(body).unwrap(), vec![]);
        // A shorter closing fence doesn't close the block.
        let body = "````\n```\n<!-- carrel:history -->\n````\n";
        assert_eq!(find_all(body).unwrap(), vec![]);
    }

    #[test]
    fn markers_must_be_alone_on_their_line() {
        let body =
            "See <!-- carrel:history --> inline.\n  <!-- carrel:x -->  \n<!-- /carrel:x -->\n";
        let found = find_all(body).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "x");
    }

    #[test]
    fn ambiguous_markers_are_errors() {
        let open = "<!-- carrel:history -->\n";
        let close = "<!-- /carrel:history -->\n";
        assert_eq!(
            find_all(open),
            Err(SectionError::Unclosed("history".into()))
        );
        assert_eq!(
            find_all(close),
            Err(SectionError::UnexpectedEnd("history".into()))
        );
        assert_eq!(
            find_all(&format!("{open}<!-- carrel:listing -->\n")),
            Err(SectionError::Nested {
                outer: "history".into(),
                inner: "listing".into()
            })
        );
        assert_eq!(
            find_all(&format!("{open}{close}{open}{close}")),
            Err(SectionError::Duplicate("history".into()))
        );
        assert_eq!(
            find_all("<!-- carrel:a -->\n<!-- /carrel:b -->\n"),
            Err(SectionError::UnexpectedEnd("b".into()))
        );
    }

    #[test]
    fn append_adds_a_section_without_touching_existing_text() {
        assert_eq!(
            append_section("Text", "history", "## History").unwrap(),
            "Text\n\n<!-- carrel:history -->\n## History\n<!-- /carrel:history -->\n"
        );
        assert_eq!(
            append_section("", "history", "").unwrap(),
            "<!-- carrel:history -->\n<!-- /carrel:history -->\n"
        );
        assert_eq!(
            append_section(CARD_BODY, "history", "x"),
            Err(SectionError::Duplicate("history".into()))
        );
        assert_eq!(
            append_section("", "Bad Name", ""),
            Err(SectionError::BadName("Bad Name".into()))
        );
    }

    #[test]
    fn table_padding_is_insignificant() {
        let plain = "| Item | Kind |\n| --- | :---: |\n| [a](a.md) | file |\n";
        let padded = "| Item      | Kind  |\n| --------- | :---: |\n| [a](a.md) | file  |\n";
        assert!(equivalent(plain, padded));
        assert!(!equivalent(plain, &padded.replace("file", "folder")));
        assert!(
            !equivalent("| a |\n| --- |\n", "| a |\n| :--- |\n"),
            "alignment is meaningful"
        );
        assert_eq!(
            normalize_tables("| a \\| b | `x|y` |\n"),
            "| a \\| b | `x|y` |\n"
        );
        let fenced = "```\n|  not   a table  |\n```\n";
        assert_eq!(normalize_tables(fenced), fenced);
    }
}
