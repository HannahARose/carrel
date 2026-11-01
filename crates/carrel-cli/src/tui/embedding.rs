//! The embedding index search finds items by meaning with (`docs/design/search.md`), in the
//! interactive mode: its state on Status and in Settings, and rebuilding it, switching the
//! archive's model and downloading one from Settings, as `carrel index` does.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::index::manage::Stage;
use carrel_core::index::switch;
use carrel_core::providers::Registry;

use super::Work;
use super::app::{App, Effect};
use super::review::{Picker, Purpose};
use super::settings::{Row, Shown};
use crate::Failure;

/// What the providers on this device offer, found in the background (never on the thread that
/// reads keys, since it can take a moment) and kept until the owner refreshes (`Ctrl-R`) or
/// something is downloaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Here {
    /// The embedding models providers here offer, best first: the model, the provider that
    /// runs it, and how many numbers its vectors have, if it says.
    pub embedders: Vec<(String, String, Option<u64>)>,
    /// What `carrel providers` shows.
    pub providers: Vec<String>,
    /// The text models available, best first, as `provider/offer`.
    pub text_models: Vec<String>,
    /// How many providers there are, and how many failed to start.
    pub provider_count: (usize, usize),
    /// What `carrel formats` shows.
    pub formats: Vec<String>,
    /// How many file types couldn't be read so far.
    pub unread: usize,
}

impl Here {
    /// Finds the providers on this device, as `carrel providers` does.
    #[must_use]
    pub fn find() -> Self {
        let (registry, failed) = crate::providers::registry();
        let providers = crate::providers::list_of(&registry, &failed);
        let formats = crate::formats::list_with(&registry);
        let entries = providers.json["providers"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let failed = entries.iter().filter(|p| p.get("error").is_some()).count();
        Self {
            embedders: crate::index::offered_models(&registry),
            providers: super::lines_of(&providers.text),
            text_models: crate::providers::text_models(&registry)
                .iter()
                .map(crate::providers::TextModel::name)
                .collect(),
            provider_count: (entries.len() - failed, failed),
            formats: super::lines_of(&formats.text),
            unread: formats.json["unreadable"].as_array().map_or(0, Vec::len),
        }
    }

    /// Whether a provider here offers `model`.
    fn offers(&self, model: &str) -> bool {
        self.embedders.iter().any(|(id, _, _)| id == model)
    }
}

/// How far long work in the background has got, for the bar on the help line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// What's being done, such as "Embedding with embeddinggemma-300m".
    pub what: String,
    /// How many are finished.
    pub done: usize,
    /// How many there are.
    pub total: usize,
    /// The one being worked on.
    pub item: Option<String>,
}

/// Progress shared with the thread doing the work.
pub type Shared = Arc<Mutex<Option<Progress>>>;

/// Work on the index, done in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Embedding {
    /// Embed every item again with the archive's model, as `carrel index rebuild` does.
    Rebuild,
    /// Switch the archive to this model, as `carrel index model NAME` does: or, given the
    /// archive's own model, stop a switch to another.
    Switch(String),
}

/// A choice in the index's picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Work on the index.
    Run(Embedding),
    /// Download this catalog embedding model, after asking.
    Download(String),
}

impl App {
    /// Starts finding what the providers here offer in the background, unless that's under
    /// way; what was found before stays shown meanwhile (#333).
    pub(super) fn find_here(&mut self) {
        self.find_here_with(Here::find);
    }

    /// Starts finding what the providers here offer with `find`, as [`App::find_here`] does.
    fn find_here_with(&mut self, find: impl FnOnce() -> Here + Send + 'static) {
        if self.finding_here.is_some() {
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(find());
        });
        self.finding_here = Some(receive);
    }

    /// Shows what the providers here offer, once it's been found.
    pub(super) fn poll_here(&mut self) {
        let Some(finding) = &self.finding_here else {
            return;
        };
        match finding.try_recv() {
            Ok(here) => {
                self.finding_here = None;
                self.here = Some(here);
                self.load_settings();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.finding_here = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// The Settings row for the index: its model and how far it's embedded, a switch in
    /// progress, and the models this device can embed with.
    pub(super) fn index_row(&self) -> Shown {
        let report = crate::index::status(Some(self.archive.root()));
        let mut detail = report
            .as_ref()
            .map_or_else(|Failure(e)| vec![e.clone()], |r| super::lines_of(&r.text));
        let value = report.map_or_else(|_| "can't be read".into(), |r| index_value(&r.json));
        detail.push(String::new());
        match &self.here {
            None => detail.push("Finding the models this device can embed with…".into()),
            Some(here) if here.embedders.is_empty() => {
                detail.push("No provider on this device embeds.".into());
            }
            Some(here) => {
                detail.push("This device can embed with:".into());
                let current = self.archive.settings().embedding_model();
                for (id, provider, dimensions) in &here.embedders {
                    let numbers = dimensions.map_or(String::new(), |d| format!(", {d} numbers"));
                    let using = if current == Some(id) { " (in use)" } else { "" };
                    detail.push(format!("  {id}  through {provider}{numbers}{using}"));
                }
            }
        }
        if let Some(here) = &self.here {
            let elsewhere: Vec<String> = carrel_core::models::embedding_models()
                .filter(|m| !here.offers(m.name))
                .filter_map(|m| {
                    Some(format!(
                        "  {}  ({})",
                        m.name,
                        crate::index::how_to_get(m.name)?
                    ))
                })
                .collect();
            if !elsewhere.is_empty() {
                detail.push(String::new());
                detail.push("Not on this device:".into());
                detail.extend(elsewhere);
            }
        }
        detail.push(String::new());
        detail.push(
            "Enter to rebuild it, switch the archive's model, or download one. Search keeps \
             using the current index while another is built."
                .into(),
        );
        Shown {
            row: Row::Index,
            label: "Search index".into(),
            value,
            detail,
        }
    }

    /// The choices for the index, each with its words: rebuilding, carrying on or stopping
    /// a switch, switching to each model offered here, and downloading those that aren't.
    #[must_use]
    pub fn index_choices(&self) -> Vec<(String, Choice)> {
        let mut choices = Vec::new();
        let here = self.here.clone().unwrap_or_default();
        let current = self.archive.settings().embedding_model().map(str::to_owned);
        let pending = switch::pending(&self.archive).map(|s| s.to);
        if let Some(model) = current.as_deref().filter(|m| here.offers(m)) {
            choices.push((
                format!("Rebuild the index with {model}"),
                Choice::Run(Embedding::Rebuild),
            ));
        }
        if let Some(to) = &pending {
            if here.offers(to) {
                choices.push((
                    format!("Carry on switching to {to}"),
                    Choice::Run(Embedding::Switch(to.clone())),
                ));
            }
            if let Some(model) = &current {
                choices.push((
                    format!("Stop switching to {to}, keeping {model}"),
                    Choice::Run(Embedding::Switch(model.clone())),
                ));
            }
        }
        for (id, provider, dimensions) in &here.embedders {
            if current.as_ref() == Some(id) || pending.as_ref() == Some(id) {
                continue;
            }
            let numbers = dimensions.map_or(String::new(), |d| format!(", {d} numbers"));
            let verb = if current.is_some() {
                "Switch to"
            } else {
                "Search with"
            };
            choices.push((
                format!("{verb} {id} (through {provider}{numbers})"),
                Choice::Run(Embedding::Switch(id.clone())),
            ));
        }
        let folder = carrel_core::local::models_dir();
        for model in carrel_core::models::embedding_models() {
            let downloaded = folder
                .as_ref()
                .is_some_and(|d| d.join(model.file_name()).is_file());
            if model.is_pinned() && !downloaded && !here.offers(model.name) {
                choices.push((
                    format!(
                        "Download {} ({})…",
                        model.name,
                        crate::ingest::human_size(model.size)
                    ),
                    Choice::Download(model.name.to_owned()),
                ));
            }
        }
        choices
    }

    /// Opens the index's picker.
    pub(super) fn index_picker(&mut self) {
        let choices = self.index_choices();
        if choices.is_empty() {
            self.message =
                Some("No provider on this device embeds, and there's nothing to download".into());
            return;
        }
        self.picker = Some(Picker {
            collections: choices.iter().map(|(words, _)| words.clone()).collect(),
            filter: String::new(),
            chosen: 0,
            purpose: Purpose::Index(choices),
        });
    }

    /// What choosing `words` in the index's picker does.
    pub(super) fn index_chosen(
        &mut self,
        choices: &[(String, Choice)],
        words: &str,
    ) -> Option<Effect> {
        match choices.iter().find(|(w, _)| w == words).map(|(_, c)| c)? {
            Choice::Run(work) => Some(Effect::Index(work.clone())),
            Choice::Download(name) => {
                self.confirm_download(name);
                None
            }
        }
    }

    /// The line for work in the background: what it is, and a bar when it says how far it
    /// has got.
    #[must_use]
    pub fn busy_line(&self) -> Option<String> {
        let busy = self.busy.as_ref()?;
        let progress = self.progress.lock().ok().and_then(|p| p.clone());
        Some(match progress {
            Some(p) => {
                let mut line = format!(
                    "{}: {} {}/{}",
                    p.what,
                    crate::progress::bar(p.done, p.total),
                    p.done,
                    p.total
                );
                if let Some(item) = p.item {
                    let shown: String = item.chars().take(30).collect();
                    line.push_str("  ");
                    line.push_str(&shown);
                }
                line
            }
            None => busy.clone(),
        })
    }

    /// The index's state for Status, as `carrel index status` reports it.
    pub(super) fn index_status(&self) -> Vec<String> {
        let mut lines = vec![String::new(), "Search index:".to_owned()];
        match crate::index::status(Some(self.archive.root())) {
            Ok(report) => lines.extend(report.text.lines().map(|l| format!("  {l}"))),
            Err(Failure(e)) => lines.push(format!("  {e}")),
        }
        lines
    }
}

/// The index's state in a few words, from `carrel index status --json`.
fn index_value(status: &serde_json::Value) -> String {
    let switching = status["switch"]
        .as_object()
        .map(|s| {
            format!(
                "; switching to {}, {} of {}",
                s["to"].as_str().unwrap_or_default(),
                s["embedded"],
                s["items"]
            )
        })
        .unwrap_or_default();
    match status["model"].as_str() {
        None => format!("no model{switching}"),
        Some(model) if status["exists"] == false => format!("{model}, not built{switching}"),
        Some(model) => format!(
            "{model}, {} of {} embedded{switching}",
            status["embedded"], status["items"]
        ),
    }
}

/// The work for [`Effect::Index`], in the background, reporting how far it has got to
/// `progress`. `registry` finds the providers and `holder` says who takes the writer lease,
/// both in the background, since they can take a moment.
pub(super) fn index_work(
    root: PathBuf,
    work: Embedding,
    progress: Shared,
    registry: Box<dyn FnOnce() -> Registry + Send>,
    holder: Box<dyn FnOnce() -> Result<Holder, Failure> + Send>,
) -> (String, Work) {
    let busy = match &work {
        Embedding::Rebuild => "Rebuilding the search index…".to_owned(),
        Embedding::Switch(model) => format!("Switching to {model}…"),
    };
    let run: Work = Box::new(move || {
        let archive = carrel_core::archive::Archive::open(&root)?;
        let registry = registry();
        let model = match &work {
            Embedding::Rebuild => crate::models::archive_model(&archive)?,
            Embedding::Switch(model) => model.clone(),
        };
        let embedding = format!("Embedding with {model}");
        let mut report = |stage: Stage, done: usize, total: usize, item: Option<&str>| {
            if let Ok(mut shown) = progress.lock() {
                *shown = Some(Progress {
                    what: match stage {
                        Stage::Reading => "Reading text".to_owned(),
                        Stage::Embedding => embedding.clone(),
                    },
                    done,
                    total,
                    item: item.map(str::to_owned),
                });
            }
        };
        let result = match work {
            Embedding::Rebuild => {
                crate::index::rebuild_with(&archive, &registry, &model, &mut report)
            }
            Embedding::Switch(model) => {
                let holder = holder()?;
                crate::index::switch_with(
                    &archive,
                    &registry,
                    &model,
                    &mut || {
                        DiskLock::for_archive(&archive).acquire(
                            holder,
                            "index model",
                            DEFAULT_LEASE,
                        )
                    },
                    &mut report,
                )
            }
        };
        if let Ok(mut shown) = progress.lock() {
            *shown = None;
        }
        result
    });
    (busy, run)
}

#[cfg(test)]
mod tests {
    use carrel_core::card::Id;
    use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
    use carrel_core::index::Index;
    use carrel_core::providers::{
        Capability, EmbeddingRequest, Embeddings, Offer, Provider, ProviderError, Registry,
    };
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::app::{App, Confirm, Effect, Tab};
    use super::super::review::Purpose;
    use super::super::tests::{key, sample, screen, word_vector};
    use super::{Choice, Embedding, Here, Progress, index_work};

    /// The sample archive's embedding model, and another to switch to.
    const SAMPLE: &str = "sample/embedding-model-v1";
    const WIDE: &str = "word-embed-wide";

    /// An embedder offering both models, with vectors counting each text's words as the fake
    /// embedder in the core's index tests makes them: 32 numbers for the wide model, the
    /// first 16 of them for the sample's.
    struct WordEmbed {
        offers: Vec<Offer>,
    }

    impl WordEmbed {
        fn new() -> Self {
            let offer = |id: &str, dimensions: u64| {
                let mut extra = serde_json::Map::new();
                extra.insert("dimensions".into(), dimensions.into());
                Offer {
                    capability: Capability::Embed,
                    id: id.into(),
                    label: None,
                    priority: 10,
                    available: true,
                    reason: None,
                    media_types: Vec::new(),
                    extra,
                }
            };
            Self {
                offers: vec![offer(SAMPLE, 16), offer(WIDE, 32)],
            }
        }
    }

    impl Provider for WordEmbed {
        fn name(&self) -> &'static str {
            "words"
        }
        fn offers(&self) -> &[Offer] {
            &self.offers
        }
        fn embed(
            &self,
            offer: &str,
            request: &EmbeddingRequest,
        ) -> Result<Embeddings, ProviderError> {
            let dimensions = if offer == WIDE { 32 } else { 16 };
            let vectors = request
                .texts
                .iter()
                .map(|t| {
                    let mut v = word_vector(t);
                    v.truncate(dimensions);
                    let length = v.iter().map(|n| n * n).sum::<f32>().sqrt().max(1e-6);
                    v.iter().map(|n| n / length).collect()
                })
                .collect();
            Ok(Embeddings {
                vectors,
                model: offer.to_owned(),
                dimensions,
            })
        }
    }

    fn registry() -> Registry {
        let mut registry = Registry::with_built_ins();
        registry.add(Box::new(WordEmbed::new()));
        registry
    }

    fn holder() -> Holder {
        Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        }
    }

    /// The app with the fake embedder as this device's only one, on Settings' index row.
    fn on_the_index_row(app: &mut App) {
        app.here = Some(Here {
            embedders: vec![
                (SAMPLE.into(), "words".into(), Some(16)),
                (WIDE.into(), "words".into(), Some(32)),
            ],
            ..Here::default()
        });
        app.key(key(KeyCode::Char('8')));
        assert_eq!(app.tab, Tab::Settings);
        app.load_settings();
        app.settings_chosen = app
            .settings
            .iter()
            .position(|s| s.label == "Search index")
            .unwrap();
    }

    /// Runs `work` as the run loop would, with the fake embedder, and reads everything again.
    fn run(app: &mut App, work: Embedding) -> String {
        let (_, run) = index_work(
            app.archive.root().to_owned(),
            work,
            app.progress.clone(),
            Box::new(registry),
            Box::new(|| Ok(holder())),
        );
        let report = run().map_err(|crate::Failure(e)| e).unwrap();
        assert_eq!(
            *app.progress.lock().unwrap(),
            None,
            "the bar goes when done"
        );
        app.reload().unwrap();
        app.load_settings();
        report.text
    }

    fn index_value(app: &App) -> String {
        app.settings
            .iter()
            .find(|s| s.label == "Search index")
            .unwrap()
            .value
            .clone()
    }

    #[test]
    fn status_and_settings_show_the_index_and_rebuild_it() {
        let (_dir, mut app) = sample(false);
        let text = screen(&app, 120, 60);
        assert!(text.contains("Search index:"), "{text}");
        assert!(
            text.contains(&format!("There's no meaning index for {SAMPLE} yet")),
            "{text}"
        );

        on_the_index_row(&mut app);
        assert_eq!(index_value(&app), format!("{SAMPLE}, not built"));
        let text = screen(&app, 160, 50);
        assert!(text.contains("This device can embed with:"), "{text}");
        assert!(
            text.contains(&format!("{WIDE}  through words, 32 numbers")),
            "{text}"
        );
        assert!(
            !app.settings.iter().any(|s| s.label == "embedding.model"),
            "the index's row stands in for the setting"
        );

        // Enter lists what can be done, the rebuild first.
        app.key(key(KeyCode::Enter));
        let picker = app.picker.clone().unwrap();
        let Purpose::Index(choices) = &picker.purpose else {
            panic!("{picker:?}");
        };
        assert_eq!(
            picker.collections[0],
            format!("Rebuild the index with {SAMPLE}")
        );
        assert!(
            picker
                .collections
                .contains(&format!("Switch to {WIDE} (through words, 32 numbers)")),
            "{picker:?}"
        );
        assert!(choices.iter().all(|(_, c)| match c {
            Choice::Download(name) =>
                carrel_core::models::find(name).is_some_and(|m| m.embedding.is_some()),
            Choice::Run(_) => true,
        }));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Index(Embedding::Rebuild))
        );

        // It runs in the background, and Status and Settings show the result.
        let report = run(&mut app, Embedding::Rebuild);
        let items = carrel_core::index::plan(&app.archive, None).unwrap().items;
        assert!(
            report.starts_with(&format!("Embedded {items} items with {SAMPLE}")),
            "{report}"
        );
        assert_eq!(
            index_value(&app),
            format!("{SAMPLE}, {items} of {items} embedded")
        );
        app.tab = Tab::Status;
        let text = screen(&app, 120, 60);
        assert!(
            text.contains(&format!(
                "The meaning index uses {SAMPLE}: {items} of {items}"
            )),
            "{text}"
        );
        assert!(text.contains("; 0 behind."), "{text}");
    }

    #[test]
    fn the_help_line_shows_a_bar_while_work_says_how_far_it_has_got() {
        let (_dir, mut app) = sample(false);
        app.busy = Some("Rebuilding the search index…".into());
        assert_eq!(
            app.busy_line().as_deref(),
            Some("Rebuilding the search index…")
        );
        *app.progress.lock().unwrap() = Some(Progress {
            what: format!("Embedding with {SAMPLE}"),
            done: 6,
            total: 24,
            item: Some("Projects/Hardware/notes.md".into()),
        });
        let line = app.busy_line().unwrap();
        assert!(
            line.starts_with(&format!("Embedding with {SAMPLE}: ██████░")),
            "{line}"
        );
        assert!(line.contains(" 6/24  Projects/Hardware/notes.md"), "{line}");
        let text = screen(&app, 200, 30);
        assert!(text.contains(" 6/24 "), "{text}");
    }

    #[test]
    fn settings_switches_the_archives_model_and_shows_a_switch_in_progress() {
        let (_dir, mut app) = sample(true);
        on_the_index_row(&mut app);
        let items = carrel_core::index::plan(&app.archive, None).unwrap().items;

        // Part way through a switch, as when one stops: Settings says so, and offers to carry
        // on or to stop.
        let lease = DiskLock::for_archive(&app.archive)
            .acquire(holder(), "test", DEFAULT_LEASE)
            .unwrap();
        carrel_core::index::switch::begin(&app.archive, WIDE, &lease).unwrap();
        lease.release().unwrap();
        app.reload().unwrap();
        assert_eq!(
            index_value(&app),
            format!("{SAMPLE}, not built; switching to {WIDE}, 0 of {items}")
        );
        let text = screen(&app, 200, 50);
        assert!(
            text.contains(&format!("Switching to {WIDE}: 0 of")),
            "{text}"
        );
        let choices: Vec<String> = app.index_choices().into_iter().map(|(w, _)| w).collect();
        assert!(
            choices.contains(&format!("Carry on switching to {WIDE}")),
            "{choices:?}"
        );
        assert!(
            choices.contains(&format!("Stop switching to {WIDE}, keeping {SAMPLE}")),
            "{choices:?}"
        );

        // Carrying on finishes it: the archive is searched with the new model.
        app.key(key(KeyCode::Enter));
        app.picker.as_mut().unwrap().filter = "carry on".into();
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Index(Embedding::Switch(WIDE.into())))
        );
        let report = run(&mut app, Embedding::Switch(WIDE.into()));
        assert!(
            report.contains(&format!("This archive is now searched with {WIDE}")),
            "{report}"
        );
        assert_eq!(app.archive.settings().embedding_model(), Some(WIDE));
        assert_eq!(
            index_value(&app),
            format!("{WIDE}, {items} of {items} embedded")
        );
        assert!(Index::open(&app.archive, WIDE).unwrap().is_some());
        assert!(
            app.index_choices()
                .iter()
                .any(|(w, _)| *w == format!("Switch to {SAMPLE} (through words, 16 numbers)")),
            "and back again"
        );
    }

    #[test]
    fn a_model_not_here_is_downloaded_after_asking() {
        let (_dir, mut app) = sample(false);
        on_the_index_row(&mut app);
        let choices = app.index_choices();
        let Some((words, Choice::Download(name))) = choices
            .iter()
            .find(|(_, c)| matches!(c, Choice::Download(_)))
            .cloned()
        else {
            // Every catalog embedding model is downloaded on this device.
            return;
        };
        let model = carrel_core::models::find(&name).unwrap();
        assert_eq!(
            words,
            format!(
                "Download {name} ({})…",
                crate::ingest::human_size(model.size)
            )
        );
        app.key(key(KeyCode::Enter));
        app.picker.as_mut().unwrap().filter.clone_from(&words);
        assert_eq!(app.key(key(KeyCode::Enter)), None);
        let Some(Confirm::Download { name: asked, about }) = app.confirm.clone() else {
            panic!("{:?}", app.confirm);
        };
        assert_eq!(asked, name);
        assert!(
            about.contains(&crate::ingest::human_size(model.size)),
            "{about}"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Download(name))
        );
    }

    /// Switching screens, refreshing and opening Settings' pickers never builds a registry on
    /// the thread that reads keys, which starting the providers once froze for seconds (#333):
    /// the providers are found in the background, and what was found before stays shown.
    #[test]
    fn keys_never_build_a_registry_on_their_thread() {
        let (_dir, mut app) = sample(false);
        // Stands in for finding them in the background, so the test starts no providers.
        let (_send, finding) = std::sync::mpsc::channel();
        app.finding_here = Some(finding);
        let before = carrel_core::providers::device::registries_built_here();

        let mut keys: Vec<KeyEvent> = "123456789LD"
            .chars()
            .map(|c| key(KeyCode::Char(c)))
            .collect();
        keys.extend(
            [
                KeyCode::Tab,
                KeyCode::BackTab,
                KeyCode::Right,
                KeyCode::Left,
            ]
            .map(key),
        );
        keys.push(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        keys.push(key(KeyCode::Char('8')));
        for _ in 0..12 {
            keys.extend([KeyCode::Enter, KeyCode::Esc, KeyCode::Down].map(key));
        }
        for event in keys {
            app.key(event);
            app.confirm = None;
            app.input = None;
        }
        assert_eq!(app.tab, Tab::Settings);
        assert!(app.here.is_none(), "still being found");

        app.here = Some(Here::default());
        app.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        app.load_settings();
        assert!(
            app.here.is_some(),
            "what was found stays shown while it's found again"
        );
        assert_eq!(
            carrel_core::providers::device::registries_built_here(),
            before,
            "no registry was built on the thread that reads keys"
        );
    }

    /// A provider that never answers `initialize` holds up finding the providers, but no
    /// screen switch waits for it (#333): Settings says it's still finding them, and shows
    /// them once they're found.
    #[test]
    fn a_hung_provider_holds_up_no_screen_switch() {
        let (_dir, mut app) = sample(false);
        let (answer, hung) = std::sync::mpsc::channel::<()>();
        app.find_here_with(move || {
            let _ = hung.recv();
            Here {
                provider_count: (3, 1),
                ..Here::default()
            }
        });

        let started = std::time::Instant::now();
        for c in "8123456789LD8".chars() {
            app.key(key(KeyCode::Char(c)));
            app.poll_here();
        }
        assert_eq!(app.tab, Tab::Settings);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert!(screen(&app, 140, 40).contains("finding them…"));

        answer.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.here.is_none() && std::time::Instant::now() < deadline {
            app.poll_here();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(app.here.as_ref().map(|h| h.provider_count), Some((3, 1)));
        assert!(screen(&app, 140, 40).contains("3 providers, 1 couldn't start"));
    }
}
