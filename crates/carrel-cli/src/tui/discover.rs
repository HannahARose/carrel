//! The Discover screen (`docs/design/librarian.md`, "Discovery"): the views the core computes,
//! drawn as `carrel discover` draws them. `d` shows on this day, `w` wanders the stacks, one
//! key per step (`Space` or `Enter`), with the item's preview where the terminal draws
//! images, `t` the timeline (`p` changes its periods, `/` narrows it by words and filters and
//! `b` chooses its date), `m` the archive map as a sized tree, `h` activity as a block
//! heatmap (`r` chooses its days), and `g` the graph's strongest connections, between
//! collections or, with `c`, between items. Now and then a wander step asks a question about its item,
//! which `a` answers. `w` in Browse starts a walk from the item shown.

use carrel_core::discover::timeline::{Options, Resolution};
use carrel_core::discover::wander::{self, Surroundings, Walk};
use std::fmt::Write as _;

use carrel_core::discover::{Catalog, DateKind};
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments::wander_asks;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Tab};
use super::item::ItemView;
use super::option_lines::OptionLine;
use super::review::Handled;

/// How many connections the graph view shows, the strongest first.
const GRAPH_SHOWN: usize = 30;

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
    /// The strongest connections, between collections or items.
    Graph,
}

impl View {
    /// Every view, with its key and name, in order.
    pub const ALL: [(Self, char, &'static str); 6] = [
        (Self::Today, 'd', "on this day"),
        (Self::Wander, 'w', "wander"),
        (Self::Timeline, 't', "timeline"),
        (Self::Map, 'm', "map"),
        (Self::Activity, 'h', "activity"),
        (Self::Graph, 'g', "graph"),
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
    /// The timeline's words and filters, as `carrel discover timeline QUERY` takes them.
    pub timeline_words: String,
    /// Which date the timeline goes by; none for when each was made, else filed.
    pub timeline_by: Option<DateKind>,
    /// The first and last day activity covers; none for the last year.
    pub activity_range: Option<(jiff::civil::Date, jiff::civil::Date)>,
    /// Whether the graph shows connections between items rather than collections.
    pub graph_items: bool,
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
            View::Timeline => self.timeline_lines(&catalog),
            View::Map => {
                crate::discover::map_text(&carrel_core::discover::map::map(&catalog), None)
            }
            View::Activity => {
                let (from, to) = self
                    .stacks
                    .activity_range
                    .unwrap_or_else(|| carrel_core::discover::activity::last_year(today));
                let mut text = format!("From {from} to {to}\n\n");
                text.push_str(&crate::discover::activity_text(
                    &carrel_core::discover::activity::activity(&catalog, from, to),
                ));
                text
            }
            View::Graph => crate::discover::graph_text(
                &carrel_core::graph::Graph::build(&catalog),
                self.stacks.graph_items,
                GRAPH_SHOWN,
            ),
            View::Wander => return,
        }
        .lines()
        .map(str::to_owned)
        .collect();
    }

    /// The timeline, as `carrel discover timeline` draws it with the words, filters and date
    /// chosen, below a line saying which they are.
    fn timeline_lines(&self, catalog: &Catalog) -> String {
        let query = match carrel_core::search::query::Query::parse(&self.stacks.timeline_words) {
            Ok(query) => query,
            Err(e) => return format!("{e}; press / to change the words"),
        };
        let only = match carrel_core::discover::timeline::matching(&self.archive, &query) {
            Ok(only) => only,
            Err(e) => return e.to_string(),
        };
        let options = Options {
            by: self.stacks.timeline_by,
            resolution: self.stacks.per,
            filters: query.filters.clone(),
            only,
        };
        let mut text = format!(
            "By {}",
            self.stacks
                .timeline_by
                .map_or("when made, else filed", DateKind::name)
        );
        if !self.stacks.timeline_words.is_empty() {
            write!(text, ", for “{}”", self.stacks.timeline_words).unwrap();
        }
        text.push_str("\n\n");
        text.push_str(&crate::discover::timeline_text(
            &carrel_core::discover::timeline::timeline(catalog, &options),
            3,
        ));
        text
    }

    /// Starts a new walk from the item at `card`, as `carrel discover wander ITEM` does, and
    /// shows its first step on the Discover screen.
    pub(super) fn wander_from(&mut self, card: &str) {
        let Some(id) = self
            .archive
            .items()
            .ok()
            .and_then(|items| items.into_iter().find(|i| i.card == card))
            .map(|i| i.id)
        else {
            self.message = Some("It isn't in the archive any more".into());
            return;
        };
        self.stacks.walk = Some(Walk::new(new_seed(), Some(id)).remembering(&self.records.visits));
        self.stacks.view = View::Wander;
        self.stacks.done = false;
        self.stacks.asked_at = None;
        self.tab = Tab::Discover;
        self.wander_step();
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
                Some(question) => self.answer_moment(&question),
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
            (View::Timeline, KeyCode::Char('/')) => {
                self.ask_option(OptionLine::TimelineWords);
                if let Some(input) = self.input.as_mut() {
                    input.text.clone_from(&self.stacks.timeline_words);
                }
            }
            (View::Timeline, KeyCode::Char('b')) => {
                self.stacks.timeline_by = match self.stacks.timeline_by {
                    None => Some(DateKind::Created),
                    Some(DateKind::Created) => Some(DateKind::Filed),
                    Some(DateKind::Filed) => Some(DateKind::Worked),
                    Some(DateKind::Worked) => None,
                };
                self.load_discover();
            }
            (View::Activity, KeyCode::Char('r')) => self.ask_option(OptionLine::ActivityRange),
            (View::Graph, KeyCode::Char('c')) => {
                self.stacks.graph_items = !self.stacks.graph_items;
                self.stacks.scroll = 0;
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
            ("g", "graph"),
        ];
        match self.stacks.view {
            View::Wander => {
                keys.push(("Space", "next step"));
                if self.stacks.question.is_some() {
                    keys.push(("a", "answer"));
                }
                keys.extend([("o", "open in Browse"), ("n", "new walk")]);
            }
            View::Timeline => keys.extend([
                ("p", "periods"),
                ("/", "words"),
                ("b", "by date"),
                ("↑↓", "scroll"),
            ]),
            View::Activity => keys.extend([("r", "days"), ("↑↓", "scroll")]),
            View::Graph => keys.extend([
                (
                    "c",
                    if self.stacks.graph_items {
                        "collections"
                    } else {
                        "items"
                    },
                ),
                ("↑↓", "scroll"),
            ]),
            _ => keys.push(("↑↓", "scroll")),
        }
        keys.push(("?", "help"));
        keys
    }
}

#[cfg(test)]
pub(super) mod tests {
    use carrel_core::questions::QuestionEntry;
    use carrel_core::questions::moments::WANDER_EVERY;
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{App, Effect, Step, Tab};
    use super::super::item::Pane;
    use super::super::screens::{Correcting, InputPurpose};
    use super::super::tests::{key, sample, screen};
    use super::View;

    /// A sample item Carrel has a question about, and the question.
    pub(in super::super) fn asked_item(app: &App) -> (String, QuestionEntry) {
        app.archive
            .items()
            .unwrap()
            .into_iter()
            .find_map(|item| {
                let asked = super::super::asking::questions_about(&app.archive, &item.card);
                asked.into_iter().next().map(|q| (item.card.clone(), q))
            })
            .expect("the sample archive has an item to ask about")
    }

    #[test]
    fn discover_shows_each_view_as_the_command_draws_it() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('D')));
        assert_eq!(app.tab, Tab::Discover);
        let text = screen(&app, 120, 30);
        assert!(text.contains("d on this day"), "{text}");
        assert!(
            text.contains("On this day") || text.contains("in other years"),
            "{text}"
        );
        app.key(key(KeyCode::Char('m')));
        let text = screen(&app, 120, 30);
        assert!(text.contains("items in"), "the map's total: {text}");
        assert!(
            text.contains("├─ ") || text.contains("└─ "),
            "a sized tree: {text}"
        );
        app.key(key(KeyCode::Char('h')));
        let text = screen(&app, 120, 30);
        assert!(
            text.contains("Mon") && text.contains("Less"),
            "a heatmap: {text}"
        );
        app.key(key(KeyCode::Char('t')));
        let month = app.stacks.lines.clone();
        app.key(key(KeyCode::Char('p')));
        assert_ne!(app.stacks.lines, month, "p groups by day instead");
        assert!(app.keys().contains(&("p", "periods")));

        // The graph: collections, then items with c.
        app.key(key(KeyCode::Char('g')));
        let text = screen(&app, 120, 30);
        assert!(text.contains("Projects ↔ Reading"), "{text}");
        assert!(app.keys().contains(&("c", "items")));
        app.key(key(KeyCode::Char('c')));
        let text = screen(&app, 120, 30);
        assert!(text.contains("between items"), "{text}");
        assert!(text.contains("oscillator-sim (Projects)"), "{text}");
        assert!(app.keys().contains(&("c", "collections")));
    }

    #[test]
    fn wander_takes_a_step_a_key_and_asks_now_and_then() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('D')));
        app.key(key(KeyCode::Char('w')));
        assert_eq!(app.stacks.view, View::Wander);
        let first = app.stacks.item.card.clone().expect("the first step");
        assert!(
            screen(&app, 120, 30).contains("1. "),
            "{}",
            screen(&app, 120, 30)
        );
        let collection = first.rsplit_once('/').unwrap().0;
        assert!(
            app.records.visits.opened.contains_key(collection),
            "a step opens its collection"
        );
        // Every item the walk can reach, a step a key; questions never on two steps close
        // together, and one about the item Carrel has a question about once it's reached
        // with room to ask.
        let mut asked: Vec<usize> = Vec::new();
        let mut steps = 1;
        while !app.stacks.done && steps < 100 {
            app.key(key(KeyCode::Char(' ')));
            // The key that finds the walk is over takes no step, and leaves the last step's
            // question showing.
            if app.stacks.done {
                break;
            }
            steps += 1;
            if app.stacks.question.is_some() {
                asked.push(steps);
                assert!(screen(&app, 120, 30).contains("Carrel asks:"));
            }
        }
        assert!(app.stacks.done, "the walk ends");
        for pair in asked.windows(2) {
            assert!(pair[1] - pair[0] >= WANDER_EVERY, "{asked:?}");
        }
        if let Some(n) = app.stacks.asked_at {
            assert!(asked.contains(&n));
        }
        app.key(key(KeyCode::Char('o')));
        assert_eq!(app.tab, Tab::Browse);
    }

    #[test]
    fn browse_asks_about_the_item_beside_it_and_the_answer_is_a_correction() {
        let (_dir, mut app) = sample(true);
        let (card, question) = asked_item(&app);
        let item = super::super::asking::item_at(&app.archive, &card)
            .unwrap()
            .id;
        app.key(key(KeyCode::Char('5')));
        app.go_to(&card);
        assert_eq!(app.browse_pane, Pane::Items);
        let text = screen(&app, 140, 34);
        assert!(
            text.contains(&format!("Carrel asks: {}", question.question)),
            "{text}"
        );
        assert!(app.keys().contains(&("a", "answer its question")));
        app.key(key(KeyCode::Char('a')));
        let Some(InputPurpose::Answer { id, .. }) = app.input.as_ref().map(|i| i.purpose.clone())
        else {
            panic!("a line for the answer");
        };
        let (queued, _) = carrel_core::questions::QuestionQueue::of(&app.archive)
            .list()
            .unwrap();
        assert!(queued.iter().any(|q| q.id == id), "it joins the queue");
        for c in "A photo of the bench".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Correction(Step::Work {
                target: Correcting::Item(item.to_string()),
                words: format!(
                    "Asked “{}”, the owner said: A photo of the bench",
                    question.question
                ),
                answering: Some(id),
            }))
        );
    }

    #[test]
    fn status_names_the_days_questions() {
        let (_dir, app) = sample(true);
        let text = screen(&app, 120, 40);
        assert!(text.contains("Today's questions"), "{text}");
    }
}
