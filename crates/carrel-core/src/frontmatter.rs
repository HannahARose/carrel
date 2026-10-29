//! Splitting a Markdown document into its YAML frontmatter and body (FORMAT.md §2.2).
//!
//! The body is kept exactly as it appears in the file, so writing a document back with the
//! same body never changes a byte of it.

/// A Markdown document split at its frontmatter delimiters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split<'a> {
    /// The YAML between the `---` lines, ending with a newline (or empty).
    pub yaml: &'a str,
    /// Everything after the closing `---` line, exactly as in the file.
    pub body: &'a str,
}

/// Why a document couldn't be split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitError {
    /// The file starts with a UTF-8 byte-order mark, which FORMAT.md §2.1 forbids.
    ByteOrderMark,
    /// The file doesn't start with a line containing exactly `---`.
    NoFrontmatter,
    /// The opening `---` line has no matching closing line.
    Unterminated,
    /// The frontmatter uses CRLF line endings; FORMAT.md §2.1 requires LF.
    CrLf,
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ByteOrderMark => "the file starts with a byte-order mark",
            Self::NoFrontmatter => "the file doesn't start with a `---` line",
            Self::Unterminated => "the frontmatter has no closing `---` line",
            Self::CrLf => "the frontmatter uses CRLF line endings; LF is required",
        })
    }
}

impl std::error::Error for SplitError {}

/// Splits `text` into frontmatter and body.
///
/// The frontmatter ends at the first line, after the opening one, that contains exactly
/// `---`. That line may be the last in the file, with or without a trailing newline.
///
/// # Errors
///
/// See [`SplitError`].
pub fn split(text: &str) -> Result<Split<'_>, SplitError> {
    if text.starts_with('\u{feff}') {
        return Err(SplitError::ByteOrderMark);
    }
    if text.starts_with("---\r\n") {
        return Err(SplitError::CrLf);
    }
    let rest = text
        .strip_prefix("---\n")
        .ok_or(SplitError::NoFrontmatter)?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        if content == "---" {
            return Ok(Split {
                yaml: &rest[..offset],
                body: &rest[offset + line.len()..],
            });
        }
        if content == "---\r" || content.ends_with('\r') {
            return Err(SplitError::CrLf);
        }
        offset += line.len();
    }
    Err(SplitError::Unterminated)
}

/// Joins frontmatter and a body into a document. `yaml` gets a trailing newline if it lacks
/// one; `body` is written exactly as given.
#[must_use]
pub fn join(yaml: &str, body: &str) -> String {
    let mut out = String::with_capacity(yaml.len() + body.len() + 9);
    out.push_str("---\n");
    out.push_str(yaml);
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_the_first_closing_line() {
        let s = split("---\na: 1\n---\nbody\n---\nmore\n").unwrap();
        assert_eq!(s.yaml, "a: 1\n");
        assert_eq!(s.body, "body\n---\nmore\n");
    }

    #[test]
    fn handles_empty_bodies_and_missing_final_newline() {
        assert_eq!(split("---\na: 1\n---\n").unwrap().body, "");
        assert_eq!(split("---\na: 1\n---").unwrap().body, "");
        assert_eq!(
            split("---\n---\nx").unwrap(),
            Split {
                yaml: "",
                body: "x"
            }
        );
    }

    #[test]
    fn rejects_what_format_md_forbids() {
        assert_eq!(split("no frontmatter"), Err(SplitError::NoFrontmatter));
        assert_eq!(split("--- \na: 1\n---\n"), Err(SplitError::NoFrontmatter));
        assert_eq!(split("---\na: 1\n"), Err(SplitError::Unterminated));
        assert_eq!(
            split("\u{feff}---\na: 1\n---\n"),
            Err(SplitError::ByteOrderMark)
        );
        assert_eq!(split("---\r\na: 1\r\n---\r\n"), Err(SplitError::CrLf));
        assert_eq!(split("---\na: 1\r\n---\n"), Err(SplitError::CrLf));
    }

    #[test]
    fn a_dashed_line_with_extra_text_does_not_close() {
        let s = split("---\na: \"---x\"\n----\n---\nbody").unwrap();
        assert_eq!(s.yaml, "a: \"---x\"\n----\n");
        assert_eq!(s.body, "body");
    }

    #[test]
    fn join_inverts_split() {
        for text in ["---\na: 1\n---\nbody\n", "---\n---\n", "---\na: 1\n---\n"] {
            let s = split(text).unwrap();
            assert_eq!(join(s.yaml, s.body), text);
        }
        assert_eq!(join("a: 1", "x"), "---\na: 1\n---\nx");
    }
}
