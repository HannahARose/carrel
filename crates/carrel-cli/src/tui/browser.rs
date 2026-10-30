//! The built-in file browser, for choosing what to ingest when no file picker is installed
//! (`docs/design/tui-ingest.md`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// A folder's contents, with what's marked to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    /// The folder shown.
    pub dir: PathBuf,
    /// Its entries: name, and whether it's a folder. Folders first, then by name.
    pub entries: Vec<(String, bool)>,
    /// The chosen entry.
    pub chosen: usize,
    /// What's marked, from any folder.
    pub marked: BTreeSet<PathBuf>,
    /// Whether names starting with `.` are shown.
    pub hidden: bool,
    /// Why the folder couldn't be read, if it couldn't.
    pub problem: Option<String>,
}

/// What a key in the browser comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep browsing.
    Browsing,
    /// Add these.
    Add(Vec<PathBuf>),
    /// Type a path instead.
    Type,
    /// Close it.
    Cancel,
}

impl Browser {
    /// The browser on `dir`.
    #[must_use]
    pub fn open(dir: PathBuf) -> Self {
        let mut browser = Self {
            dir,
            entries: Vec::new(),
            chosen: 0,
            marked: BTreeSet::new(),
            hidden: false,
            problem: None,
        };
        browser.read();
        browser
    }

    fn read(&mut self) {
        self.entries.clear();
        self.problem = None;
        match std::fs::read_dir(&self.dir) {
            Ok(entries) => {
                for entry in entries.filter_map(Result::ok) {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !self.hidden && name.starts_with('.') {
                        continue;
                    }
                    let folder = entry.path().is_dir();
                    self.entries.push((name, folder));
                }
                self.entries.sort_by(|a, b| {
                    b.1.cmp(&a.1)
                        .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                });
            }
            Err(e) => self.problem = Some(format!("{}: {e}", self.dir.display())),
        }
        self.chosen = self.chosen.min(self.entries.len().saturating_sub(1));
    }

    fn chosen_path(&self) -> Option<PathBuf> {
        self.entries
            .get(self.chosen)
            .map(|(name, _)| self.dir.join(name))
    }

    fn go(&mut self, dir: &Path) {
        dir.clone_into(&mut self.dir);
        self.chosen = 0;
        self.read();
    }

    /// Handles a key.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.chosen = self.chosen.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.chosen = (self.chosen + 1).min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some((name, true)) = self.entries.get(self.chosen) {
                    let dir = self.dir.join(name);
                    self.go(&dir);
                }
            }
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => {
                if let Some(parent) = self.dir.parent().map(Path::to_owned) {
                    let came_from = self
                        .dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                    self.go(&parent);
                    if let Some(i) =
                        came_from.and_then(|n| self.entries.iter().position(|e| e.0 == n))
                    {
                        self.chosen = i;
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(path) = self.chosen_path() {
                    if !self.marked.remove(&path) {
                        self.marked.insert(path);
                    }
                    self.chosen = (self.chosen + 1).min(self.entries.len().saturating_sub(1));
                }
            }
            KeyCode::Char('.') => {
                self.hidden = !self.hidden;
                self.read();
            }
            KeyCode::Char('i' | 'a') => {
                let chosen: Vec<PathBuf> = if self.marked.is_empty() {
                    self.chosen_path().into_iter().collect()
                } else {
                    self.marked.iter().cloned().collect()
                };
                if !chosen.is_empty() {
                    return Outcome::Add(chosen);
                }
            }
            KeyCode::Char('/') => return Outcome::Type,
            KeyCode::Esc | KeyCode::Char('q') => return Outcome::Cancel,
            _ => {}
        }
        Outcome::Browsing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn folders_open_and_marked_files_are_added() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Scans")).unwrap();
        std::fs::write(dir.path().join("Scans/receipt.pdf"), "x").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join(".hidden"), "h").unwrap();
        let mut browser = Browser::open(dir.path().to_owned());
        let names: Vec<&str> = browser.entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(
            names,
            ["Scans", "a.txt", "b.txt"],
            "folders first; hidden left out"
        );

        browser.key(key(KeyCode::Enter));
        assert_eq!(browser.dir, dir.path().join("Scans"));
        browser.key(key(KeyCode::Char(' ')));
        browser.key(key(KeyCode::Left));
        assert_eq!(browser.dir, dir.path());
        assert_eq!(browser.chosen, 0, "back on the folder it came from");
        browser.key(key(KeyCode::Down));
        browser.key(key(KeyCode::Char(' ')));
        assert_eq!(
            browser.key(key(KeyCode::Char('i'))),
            Outcome::Add(vec![
                dir.path().join("Scans/receipt.pdf"),
                dir.path().join("a.txt")
            ])
        );
        browser.key(key(KeyCode::Char('.')));
        assert!(browser.entries.iter().any(|e| e.0 == ".hidden"));
        assert_eq!(browser.key(key(KeyCode::Char('/'))), Outcome::Type);
        assert_eq!(browser.key(key(KeyCode::Esc)), Outcome::Cancel);
    }
}
