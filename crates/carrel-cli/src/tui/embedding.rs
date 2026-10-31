//! The embedding index search finds items by meaning with (`docs/design/search.md`), in the
//! interactive mode: its state on Status and in Settings, and rebuilding it, switching the
//! archive's model and downloading one from Settings, as `carrel index` does.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use carrel_core::coordinator::Holder;
use carrel_core::index::switch;
use carrel_core::providers::Registry;

use super::Work;
use super::app::{App, Effect};
use super::review::{Picker, Purpose};
use super::settings::{Row, Shown};
use crate::Failure;

/// What the providers on this device offer, read once and kept until the owner refreshes
/// (`Ctrl-R`) or something is downloaded, since finding the providers starts each one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Here {
    /// The embedding models providers here offer, best first: the model, the provider that
    /// runs it, and how many numbers its vectors have, if it says.
    pub embedders: Vec<(String, String, Option<u64>)>,
    /// What `carrel providers` shows.
    pub providers: Vec<String>,
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
        let (registry, _) = crate::providers::registry();
        let providers = crate::providers::list();
        let formats = crate::formats::list();
        let entries = providers.json["providers"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let failed = entries.iter().filter(|p| p.get("error").is_some()).count();
        Self {
            embedders: crate::index::offered_models(&registry),
            providers: super::lines_of(&providers.text),
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
        let what = format!("Embedding with {model}");
        let mut report = |done: usize, total: usize, item: Option<&str>| {
            if let Ok(mut shown) = progress.lock() {
                *shown = Some(Progress {
                    what: what.clone(),
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
                crate::index::switch_with(&archive, &registry, holder()?, &model, &mut report)
            }
        };
        if let Ok(mut shown) = progress.lock() {
            *shown = None;
        }
        result
    });
    (busy, run)
}
