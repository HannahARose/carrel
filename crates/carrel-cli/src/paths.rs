//! Paths and commands as people type them.

use std::path::PathBuf;

/// Words as a shell splits them: separated by spaces, with quotes or backslashes keeping a
/// space inside a word. On Windows a backslash is the path separator, not an escape, so only
/// quotes keep spaces, as Windows shells and File Explorer use them.
#[must_use]
pub fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None | Some('"'), '\\') if cfg!(not(windows)) => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (_, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// Paths as typed, pasted or dropped from a file manager: [`split_words`], with `~` for the
/// home folder.
#[must_use]
pub fn split_paths(text: &str) -> Vec<PathBuf> {
    split_words(text).iter().map(|w| expand(w)).collect()
}

/// `path` with a leading `~` replaced by the home folder.
#[must_use]
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => carrel_core::local::home_dir()
            .map_or_else(
                || PathBuf::from(path),
                |home| home.join(rest.trim_start_matches('/')),
            ),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_split_as_a_shell_would() {
        assert_eq!(
            split_paths(r"/a/b.txt '/c/with space.pdf'"),
            [
                PathBuf::from("/a/b.txt"),
                PathBuf::from("/c/with space.pdf")
            ]
        );
        if cfg!(not(windows)) {
            assert_eq!(
                split_paths(r"/d/escaped\ name.md"),
                [PathBuf::from("/d/escaped name.md")]
            );
        }
        assert_eq!(split_paths("  \"/x y\"  "), [PathBuf::from("/x y")]);
        assert_eq!(split_paths(""), Vec::<PathBuf>::new());
        let home = carrel_core::local::home_dir().unwrap();
        assert_eq!(split_paths("~/Downloads"), [home.join("Downloads")]);
        assert_eq!(
            split_words("yazi --chooser-file={out}"),
            ["yazi", "--chooser-file={out}"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn backslashes_are_separators_on_windows() {
        assert_eq!(
            split_paths(r#"C:\Users\me\a.txt "C:\Program Files\b.pdf""#),
            [
                PathBuf::from(r"C:\Users\me\a.txt"),
                PathBuf::from(r"C:\Program Files\b.pdf"),
            ]
        );
    }
}
