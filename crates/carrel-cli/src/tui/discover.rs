//! The Discover screen (`docs/design/librarian.md`, "Discovery"): the views the core computes,
//! drawn as `carrel discover` draws them. `d` shows on this day, `w` wanders the stacks, one
//! key per step (`Space` or `Enter`), with the item's preview where the terminal draws
//! images, `t` the timeline (`p` changes its periods), `m` the archive map as a sized tree,
//! and `h` activity as a block heatmap. Now and then a wander step asks a question about its
//! item, which `a` answers.

use carrel_core::discover::Catalog;
use carrel_core::discover::timeline::{Options, Resolution};
use carrel_core::discover::wander::{self, Surroundings, Walk};
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments::wander_asks;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Tab};
use super::item::ItemView;
use super::review::Handled;

/// Which discovery view is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// What was made, filed or worked on on this day in other years.
    #[default]
    Today,
    /// A walk through the stacks, one item at a time.
    Wander,
    /// Items by date.
    Timeline,
    /// Collections by size, as a tree.
    Map,
    /// When things were filed and worked on, as a heatmap.
    Activity,
}

impl View {
    /// Every view, with its key and name, in order.
    pub const ALL: [(Self, char, &'static str); 5] = [
        (Self::Today, 'd', "on this day"),
        (Self::Wander, 'w', "wander"),
        (Self::Timeline, 't', "timeline"),
        (Self::Map, 'm', "map"),
        (Self::Activity, 'h', "activity"),
    ];
}

/// The Discover screen's state.
#[derive(Debug, Default)]
pub struct Stacks {
    /// The view shown.
    pub view: View,
    /// The view's lines, for every view but a wander.
    pub lines: Vec<String>,
    /// How far they're scrolled.
    pub scroll: u16,
    /// The timeline's periods.
    pub per: Resolution,
    /// The walk, once one has started.
    pub walk: Option<Walk>,
    /// The step shown, as its lines.
    pub step: Vec<String>,
    /// The item the step is at, for the preview and Browse.
    pub item: ItemView,
    /// The step at which the walk last asked a question.
    pub asked_at: Option<usize>,
    /// The question it's asking now, if it is.
    pub question: Option<QuestionEntry>,
    /// Whether the walk has visited everything it can reach.
    pub done: bool,
}

/// A walk's seed, from the clock: different each time, short enough to type back in.
fn new_seed() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (now.as_secs() ^ u64::from(now.subsec_nanos())) % 1_000_000
}

impl App {
    /// Draws the chosen view's lines again, as the archive is now.
    pub(super) fn load_discover(&mut self) {
        let catalog = match Catalog::read(&self.archive) {
            Ok(catalog) => catalog,
            Err(e) => {
                self.stacks.lines = vec![e.to_string()];
                return;
            }
        };
        let today = crate::discover::today();
        self.stacks.lines = match self.stacks.view {
            View::Today => crate::discover::today_text(
                &carrel_core::discover::today::on_this_day(&catalog, today),
                today,
            ),
            View::Timeline => {
                let options = Options {
                    by: None,
                    resolution: self.stacks.per,
                    filters: carrel_core::search::query::Filters::default(),
                    only: None,
                };
                crate::discover::timeline_text(
                    &carrel_core::discover::timeline::timeline(&catalog, &options),
                    3,
                )
            }
            View::Map => {
                crate::discover::map_text(&carrel_core::discover::map::map(&catalog), None)
            }
            View::Activity => {
                let (from, to) = carrel_core::discover::activity::last_year(today);
                crate::discover::activity_text(&carrel_core::discover::activity::activity(
                    &catalog, from, to,
                ))
            }
            View::Wander => return,
        }
        .lines()
        .map(str::to_owned)
        .collect();
    }

    /// Takes the walk's next step, starting one if there's none: the item, why it came next,
    /// and now and then a question about it.
    pub(super) fn wander_step(&mut self) {
        let catalog = match Catalog::read(&self.archive) {
            Ok(catalog) => catalog,
            Err(e) => {
                self.message = Some(e.to_string());
                return;
            }
        };
        let visits = &self.records.visits;
        let walk = self
            .stacks
            .walk
            .get_or_insert_with(|| Walk::new(new_seed(), None).remembering(visits));
        let index = carrel_core::index::related::open(&self.archive).ok();
        let near = index.as_ref().map(wander::near_in);
        let texts = carrel_core::search::texts::TextCache::of(&self.archive);
        let mut around = Surroundings::new().with_texts(&texts);
        if let Some(near) = &near {
            around = around.with_near(near);
        }
        let Some(step) = walk.step(&catalog, &around) else {
            self.stacks.done = true;
            self.message = Some("The walk has visited every item it can reach".into());
            return;
        };
        let seed = walk.seed;
        self.records.opened(&step.item.collection);
        self.records.visits.walk = self.stacks.walk.clone();
        self.records.save_visits();
        self.stacks.step = crate::discover::step_text(&step, &catalog)
            .lines()
            .map(str::to_owned)
            .collect();
        self.stacks.step.push(String::new());
        self.stacks.step.push(format!(
            "Walk {seed}; `carrel discover wander --next` goes on with it."
        ));
        self.stacks.item = ItemView::load(
            &self.archive,
            &step.item.card,
            &self.history,
            &mut self.index,
        );
        self.stacks.question = None;
        if wander_asks(step.number, self.stacks.asked_at) {
            let asked = super::asking::questions_about(&self.archive, &step.item.card);
            if let Some(question) = asked.into_iter().next() {
                self.stacks.asked_at = Some(step.number);
                self.stacks.question = Some(question);
            }
        }
        self.previews.want(&self.archive, &self.stacks.item);
    }

    /// Handles a key on the Discover screen.
    pub(super) fn discover_key(&mut self, key: KeyEvent) -> Handled {
        if let KeyCode::Char(c) = key.code
            && let Some((view, _, _)) = View::ALL.iter().find(|(_, k, _)| *k == c)
        {
            self.stacks.view = *view;
            self.stacks.scroll = 0;
            if *view == View::Wander {
                if self.stacks.walk.is_none() {
                    self.wander_step();
                }
            } else {
                self.load_discover();
            }
            return Handled::Yes(None);
        }
        match (self.stacks.view, key.code) {
            (View::Wander, KeyCode::Char(' ') | KeyCode::Enter) if !self.stacks.done => {
                self.wander_step();
            }
            (View::Wander, KeyCode::Char('a')) => match self.stacks.question.take() {
                Some(question) => self.answer_moment(question),
                None => self.message = Some("No question on this step".into()),
            },
            (View::Wander, KeyCode::Char('o')) => {
                if let Some(card) = self.stacks.item.card.clone() {
                    self.tab = Tab::Browse;
                    self.go_to(&card);
                }
            }
            (View::Wander, KeyCode::Char('n')) => {
                self.stacks.walk = None;
                self.stacks.done = false;
                self.stacks.asked_at = None;
                self.wander_step();
            }
            (View::Timeline, KeyCode::Char('p')) => {
                self.stacks.per = match self.stacks.per {
                    Resolution::Year => Resolution::Month,
                    Resolution::Month => Resolution::Day,
                    Resolution::Day => Resolution::Year,
                };
                self.load_discover();
            }
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// Scrolls the view shown.
    pub(super) fn discover_move(&mut self, delta: i32) {
        let lines = u16::try_from(self.stacks.lines.len()).unwrap_or(u16::MAX);
        let next = i32::from(self.stacks.scroll) + delta;
        self.stacks.scroll = u16::try_from(next.max(0))
            .unwrap_or(0)
            .min(lines.saturating_sub(1));
    }

    /// The Discover screen's keys, for the help line.
    pub(super) fn discover_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![
            ("d", "today"),
            ("w", "wander"),
            ("t", "timeline"),
            ("m", "map"),
            ("h", "activity"),
        ];
        match self.stacks.view {
            View::Wander => {
                keys.push(("Space", "next step"));
                if self.stacks.question.is_some() {
                    keys.push(("a", "answer"));
                }
                keys.extend([("o", "open in Browse"), ("n", "new walk")]);
            }
            View::Timeline => keys.extend([("p", "periods"), ("↑↓", "scroll")]),
            _ => keys.push(("↑↓", "scroll")),
        }
        keys.push(("?", "help"));
        keys
    }
}
