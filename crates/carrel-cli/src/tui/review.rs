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
    /// Downloading or deleting a model, after asking: these choices, by their words.
    Models(Vec<(String, super::model_changes::ModelChange)>),
    /// Working on the embedding index: these choices, by their words.
    Index(Vec<(String, super::embedding::Choice)>),
    /// Setting this collection's own eagerness to it.
    Eagerness(String),
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
                    // The command's hint names the command; here it's a key.
                    Ok(report) => report
                        .text
                        .lines()
                        .map(|l| {
                            if l.contains("`carrel review redraft") {
                                "Press d to draft it again with the models here now".to_owned()
                            } else if let Some((start, _)) = l.split_once(" (pick one with `") {
                                format!("{start} (press p and a number to pick one):")
                            } else {
                                l.to_owned()
                            }
                        })
                        .collect(),
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

    /// `p`: opens the correction line ready to pick one of the chosen proposal's readings of
    /// what its item is; the number typed picks it, shown before it's saved.
    fn start_pick(&mut self) -> Option<Effect> {
        let entry = self.queue.get(self.chosen)?;
        if entry.readings.is_empty() {
            self.message = Some("The model gave only one reading of this item".into());
            return None;
        }
        self.input = Some(super::screens::Input {
            purpose: super::screens::InputPurpose::Correct(super::screens::Correcting::Proposal(
                entry.id,
            )),
            text: carrel_core::ingest::readings::PICK.to_owned(),
        });
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
            KeyCode::Char(
                'a' | 'h' | 'm' | 'x' | 'e' | 'c' | 'd' | 'p' | 'A' | ' ' | 'v' | 'b' | 's'
                    | 'z'
            )
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
            KeyCode::Char('d') => Some(Effect::Redraft(targets)),
            KeyCode::Char('e') => self.queue.get(self.chosen).map(|e| Effect::Edit(e.id)),
            KeyCode::Char('c') => self.start_correction(),
            KeyCode::Char('p') => self.start_pick(),
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
                    Purpose::Models(choices) => {
                        self.confirm_model_change(&choices, &to);
                        None
                    }
                    Purpose::Index(choices) => self.index_chosen(&choices, &to),
                    Purpose::Eagerness(collection) => Some(Effect::Restructure(
                        super::review_restructure::RestructureAction::Eagerness {
                            collection,
                            level: to,
                        },
                    )),
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

/// `d`: drafts `chosen` again with the models here now, in the background, as
/// `carrel review redraft` does.
pub(super) fn redraft_work(root: std::path::PathBuf, chosen: &[Id]) -> (String, super::Work) {
    let ids: Vec<String> = chosen.iter().map(ToString::to_string).collect();
    let busy = format!(
        "Drafting {} again…",
        if ids.len() == 1 {
            "the proposal".to_owned()
        } else {
            format!("{} proposals", ids.len())
        }
    );
    (
        busy,
        Box::new(move || crate::redraft::redraft_proposals(Some(&root), &ids, false, &mut |_| {})),
    )
}

#[cfg(test)]
mod tests {
    use carrel_core::card::Id;
    use carrel_core::review::{Problem, ReviewQueue, State};
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{App, Effect};
    use super::super::tests::{key, sample, screen};

    #[test]
    fn a_proposal_drafted_without_a_model_offers_d_to_draft_it_again() {
        let (_dir, app) = sample(false);
        let queue = ReviewQueue::of(&app.archive);
        let (entries, _) = queue.list().unwrap();
        let mut failed = entries[0].clone();
        failed.id = Id::new_v7();
        failed.card.title = "Unread scan".into();
        failed.state = State::NeedsOwner;
        failed.problems = vec![Problem {
            stage: "card".into(),
            message: "no text model on this device; write the card by hand".into(),
        }];
        queue.save(&failed).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('2')));
        app.chosen = app.queue.iter().position(|e| e.id == failed.id).unwrap();
        app.load_detail();

        assert_eq!(app.keys().first(), Some(&("d", "draft again")));
        let text = screen(&app, 140, 40);
        assert!(text.contains("draft again?"), "{text}");
        assert!(
            text.contains("Press d to draft it again"),
            "the hint names the key, not the command: {text}"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::Redraft(vec![failed.id]))
        );

        // A proposal whose card is settled doesn't put it first, though d still works.
        app.chosen = app.queue.iter().position(|e| e.id != failed.id).unwrap();
        app.load_detail();
        assert_ne!(app.keys().first(), Some(&("d", "draft again")));
    }

    #[test]
    fn p_picks_one_of_a_proposals_readings_through_the_correction_pop_up() {
        use carrel_core::ingest::readings::Reading;
        let (_dir, app) = sample(false);
        let queue = ReviewQueue::of(&app.archive);
        let (entries, _) = queue.list().unwrap();
        let mut unsure = entries[0].clone();
        unsure.id = Id::new_v7();
        let reading = |title: &str, against: &[&str]| Reading {
            title: title.into(),
            summary: format!("{title}, as a print file."),
            tags: Vec::new(),
            reason: "the shape".into(),
            against: against.iter().map(|a| (*a).to_owned()).collect(),
        };
        unsure.card.summary = Some("Keychain plate, as a print file.".into());
        unsure.card.title = "Keychain plate".into();
        unsure.readings = vec![
            reading("Keychain plate", &[]),
            reading("Fish model", &["the file name (“keychain plate”)"]),
        ];
        queue.save(&unsure).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('2')));
        app.chosen = app.queue.iter().position(|e| e.id == unsure.id).unwrap();
        app.load_detail();

        assert!(app.keys().contains(&("p", "pick a reading")));
        let text = screen(&app, 140, 50);
        assert!(
            text.contains("Readings (press p and a number to pick one):"),
            "the hint names the key, not the command: {text}"
        );
        assert!(text.contains("2. Fish model"), "{text}");
        assert_eq!(app.key(key(KeyCode::Char('p'))), None);
        app.key(key(KeyCode::Char('2')));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Correction(super::super::app::Step::Work {
                target: super::super::screens::Correcting::Proposal(unsure.id),
                words: "reading: 2".into(),
                answering: None,
            }))
        );

        // One with a single reading says so.
        app.chosen = app.queue.iter().position(|e| e.id != unsure.id).unwrap();
        app.load_detail();
        assert!(!app.keys().contains(&("p", "pick a reading")));
        assert_eq!(app.key(key(KeyCode::Char('p'))), None);
        assert!(app.input.is_none());
    }
}
