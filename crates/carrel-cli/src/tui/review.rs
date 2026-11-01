//! The Review screen: the queue beside the chosen proposal, a key for each decision, marks for
//! deciding several at once, and the collection picker for moving.

use carrel_core::card::Id;
use carrel_core::review::{ReviewEntry, State};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect};

/// Choosing a collection to move proposals to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// Every collection's path.
    pub collections: Vec<String>,
    /// What's been typed to narrow them.
    pub filter: String,
    /// The chosen one, among those that match.
    pub chosen: usize,
    /// What the choice is for.
    pub purpose: Purpose,
}

/// What a collection is being chosen for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// Moving these proposals there.
    Move(Vec<Id>),
    /// Answering this question with it.
    Answer(Id),
    /// Setting this setting to it.
    Setting(String),
    /// Downloading this catalog model, after asking.
    Download,
    /// Working on the embedding index: these choices, by their words.
    Index(Vec<(String, super::embedding::Choice)>),
    /// Switching archives: these choices, by their words.
    Archive(Vec<(String, super::archives::Choice)>),
}

impl Picker {
    /// The collections that match what's been typed, ignoring case.
    pub fn matching(&self) -> Vec<&str> {
        let filter = self.filter.to_lowercase();
        self.collections
            .iter()
            .filter(|c| c.to_lowercase().contains(&filter))
            .map(String::as_str)
            .collect()
    }
}

/// Whether a key was the Review screen's.
pub enum Handled {
    /// It was, and this is the work it asks for, if any.
    Yes(Option<Effect>),
    /// It wasn't; the keys every screen shares apply.
    No,
}

impl App {
    /// Keeps the proposals still waiting for a decision or for their device, grouped by
    /// destination, and drops marks on any that have gone.
    pub(super) fn load_queue(&mut self, entries: Vec<ReviewEntry>) {
        let chosen = self.queue.get(self.chosen).map(|e| e.id);
        let mut queue: Vec<ReviewEntry> = entries
            .into_iter()
            .filter(|e| !matches!(e.state, State::Rejected | State::Archived))
            .collect();
        queue.sort_by(|a, b| {
            a.destination
                .path
                .cmp(&b.destination.path)
                .then(a.id.0.cmp(&b.id.0))
        });
        self.marked.retain(|id| queue.iter().any(|e| e.id == *id));
        self.chosen = chosen
            .and_then(|id| queue.iter().position(|e| e.id == id))
            .unwrap_or(self.chosen)
            .min(queue.len().saturating_sub(1));
        self.queue = queue;
        self.load_detail();
    }

    /// Reads the chosen proposal in full.
    pub(super) fn load_detail(&mut self) {
        let entry = self.queue.get(self.chosen);
        self.detail = match entry {
            Some(entry) => {
                match crate::review::detail(Some(self.archive.root()), &entry.id.to_string(), false)
                {
                    Ok(report) => report.text.lines().map(str::to_owned).collect(),
                    Err(crate::Failure(e)) => vec![e],
                }
            }
            None => Vec::new(),
        };
        let shown = self.comparisons.get(self.compared).map(|c| c.id);
        self.comparisons = entry
            .and_then(|e| carrel_core::actions::compare(&self.archive, e).ok())
            .unwrap_or_default();
        self.compared = shown
            .and_then(|id| self.comparisons.iter().position(|c| c.id == id))
            .unwrap_or(0);
    }

    /// Moves the choice in the queue.
    pub(super) fn choose(&mut self, delta: i32) {
        let last = self.queue.len().saturating_sub(1);
        let next = i64::try_from(self.chosen).unwrap_or(0) + i64::from(delta);
        self.chosen = usize::try_from(next.max(0)).unwrap_or(0).min(last);
        self.load_detail();
    }

    /// The proposals a decision applies to: the marked ones, or else the chosen one.
    fn targets(&self) -> Vec<Id> {
        if self.marked.is_empty() {
            self.queue
                .get(self.chosen)
                .map(|e| e.id)
                .into_iter()
                .collect()
        } else {
            self.marked.iter().copied().collect()
        }
    }

    /// Opens the line for correcting the chosen proposal in words.
    fn start_correction(&mut self) -> Option<Effect> {
        if let Some(entry) = self.queue.get(self.chosen) {
            self.input = Some(super::screens::Input {
                purpose: super::screens::InputPurpose::Correct(
                    super::screens::Correcting::Proposal(entry.id),
                ),
                text: String::new(),
            });
        }
        None
    }

    /// `z`: keeps the chosen compressed file compressed if it was to be expanded, or expands
    /// it otherwise, along with the other marked ones.
    fn compressed_choice(&mut self, targets: Vec<carrel_core::card::Id>) -> Option<Effect> {
        use carrel_core::compressed::plan::Choice;
        let Some(plan) = self
            .queue
            .get(self.chosen)
            .and_then(|e| e.compressed.as_ref())
        else {
            self.message = Some("Not a compressed file".into());
            return None;
        };
        Some(Effect::Compressed {
            ids: targets,
            choice: if plan.action == Choice::Expand {
                Choice::Keep
            } else {
                Choice::Expand
            },
        })
    }

    /// Handles a key on the Review screen.
    pub(super) fn review_key(&mut self, key: KeyEvent) -> Handled {
        if let Some(handled) = self.review_list_key(key) {
            return handled;
        }
        if matches!(key.code, KeyCode::Char('[' | ']')) && !self.comparisons.is_empty() {
            let count = self.comparisons.len();
            self.compared = if key.code == KeyCode::Char(']') {
                (self.compared + 1) % count
            } else {
                (self.compared + count - 1) % count
            };
            return Handled::Yes(None);
        }
        let decision = matches!(
            key.code,
            KeyCode::Char('a' | 'h' | 'm' | 'x' | 'e' | 'c' | 'A' | ' ' | 'v' | 'b' | 's' | 'z')
        );
        if !decision {
            return Handled::No;
        }
        if let Some(busy) = &self.busy {
            self.message = Some(format!("Still busy: {busy}"));
            return Handled::Yes(None);
        }
        if self.queue.is_empty() && key.code != KeyCode::Char('A') {
            self.message = Some("Nothing to review".into());
            return Handled::Yes(None);
        }
        let targets = self.targets();
        Handled::Yes(match key.code {
            KeyCode::Char(' ') => {
                if let Some(entry) = self.queue.get(self.chosen) {
                    let id = entry.id;
                    if !self.marked.remove(&id) {
                        self.marked.insert(id);
                    }
                    self.choose(1);
                }
                None
            }
            KeyCode::Char('a') => Some(Effect::Decide {
                ids: targets,
                holding: false,
            }),
            KeyCode::Char('h') => Some(Effect::Decide {
                ids: targets,
                holding: true,
            }),
            KeyCode::Char('A') => Some(Effect::ApproveAllHigh),
            KeyCode::Char(c @ ('v' | 'b' | 's')) => {
                let (Some(entry), Some(comparison)) = (
                    self.queue.get(self.chosen),
                    self.comparisons.get(self.compared),
                ) else {
                    self.message = Some("Not a duplicate of anything archived".into());
                    return Handled::Yes(None);
                };
                match c {
                    's' => {
                        self.confirm = Some(Confirm::Reject(vec![entry.id]));
                        None
                    }
                    _ => Some(Effect::Keep {
                        id: entry.id,
                        kept: if c == 'v' {
                            carrel_core::actions::Kept::AsVersion
                        } else {
                            carrel_core::actions::Kept::Both
                        },
                        target: comparison.id,
                    }),
                }
            }
            KeyCode::Char('x') => {
                self.confirm = Some(Confirm::Reject(targets));
                None
            }
            KeyCode::Char('z') => self.compressed_choice(targets),
            KeyCode::Char('e') => self.queue.get(self.chosen).map(|e| Effect::Edit(e.id)),
            KeyCode::Char('c') => self.start_correction(),
            KeyCode::Char('m') => {
                match self.archive.collections() {
                    Ok(collections) => {
                        self.picker = Some(Picker {
                            collections: collections.into_iter().map(|c| c.path).collect(),
                            filter: String::new(),
                            chosen: 0,
                            purpose: Purpose::Move(targets),
                        });
                    }
                    Err(e) => self.message = Some(e.to_string()),
                }
                None
            }
            _ => None,
        })
    }

    /// Handles a key while the collection picker is open.
    pub(super) fn picker_key(&mut self, key: KeyEvent) -> Option<Effect> {
        let picker = self.picker.as_mut()?;
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Up => picker.chosen = picker.chosen.saturating_sub(1),
            KeyCode::Down => {
                picker.chosen = (picker.chosen + 1).min(picker.matching().len().saturating_sub(1));
            }
            KeyCode::Backspace => {
                picker.filter.pop();
                picker.chosen = 0;
            }
            KeyCode::Char(c) => {
                picker.filter.push(c);
                picker.chosen = 0;
            }
            KeyCode::Enter => {
                let to = picker
                    .matching()
                    .get(picker.chosen)
                    .map(|c| (*c).to_owned());
                let purpose = picker.purpose.clone();
                self.picker = None;
                let to = to?;
                return match purpose {
                    Purpose::Move(ids) => Some(Effect::Move { ids, to }),
                    Purpose::Answer(id) => Some(Effect::Answer {
                        id,
                        answer: to,
                        learn: false,
                    }),
                    Purpose::Setting(key) => Some(super::settings::set(&key, &to)),
                    Purpose::Download => {
                        self.confirm_download(&to);
                        None
                    }
                    Purpose::Index(choices) => self.index_chosen(&choices, &to),
                    Purpose::Archive(choices) => self.archive_chosen(&choices, &to),
                };
            }
            _ => {}
        }
        None
    }

    /// What a decision applies to, for the help line: a count when several are marked.
    pub fn marked_note(&self) -> Option<String> {
        (!self.marked.is_empty()).then(|| format!("{} marked", self.marked.len()))
    }
}
