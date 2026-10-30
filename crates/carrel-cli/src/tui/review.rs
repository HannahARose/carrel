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
        self.detail = match self.queue.get(self.chosen) {
            Some(entry) => {
                match crate::review::show(Some(self.archive.root()), &entry.id.to_string()) {
                    Ok(report) => report.text.lines().map(str::to_owned).collect(),
                    Err(crate::Failure(e)) => vec![e],
                }
            }
            None => Vec::new(),
        };
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

    /// Handles a key on the Review screen.
    pub(super) fn review_key(&mut self, key: KeyEvent) -> Handled {
        let decision = matches!(
            key.code,
            KeyCode::Char('a' | 'h' | 'm' | 'x' | 'e' | 'A' | ' ')
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
            KeyCode::Char('x') => {
                self.confirm = Some(Confirm::Reject(targets));
                None
            }
            KeyCode::Char('e') => self.queue.get(self.chosen).map(|e| Effect::Edit(e.id)),
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
                return to.map(|to| match purpose {
                    Purpose::Move(ids) => Effect::Move { ids, to },
                    Purpose::Answer(id) => Effect::Answer { id, answer: to },
                });
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
