//! The librarian's tools (`docs/design/librarian.md`): each reads the archive through the
//! same core functions the commands use, and words what it found for the model. None of them
//! changes anything.
//!
//! Restricted items are found by their card only: `read` refuses them, and search never
//! returns their text.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::archive::{Archive, ItemRef};
use crate::card::{Card, Id, Kind, Sensitivity};
use crate::card_file::CardFile;
use crate::history::History;
use crate::providers::Registry;
use crate::search::query::Query;
use crate::search::texts::{TextCache, fingerprint};
use crate::search::words::tokens;
use crate::search::{Found, Hit};

use super::ItemLink;

/// How many results a search gives the model.
const SEARCH_RESULTS: usize = 8;
/// How many passages `read` gives at once.
const PASSAGES_READ: usize = 3;
/// The longest passage `read` gives, in characters.
const PASSAGE_CHARS: usize = 1_500;
/// How much of a card's notes or a design document `show` and `collection` give, in
/// characters.
const NOTES_CHARS: usize = 1_200;
/// How many of a collection's items `collection` lists.
const COLLECTION_ITEMS: usize = 40;
/// How many changes and sessions `history` lists.
const HISTORY_ENTRIES: usize = 15;

/// What the tools read: the archive, its items, and how to start the providers search embeds
/// queries with.
pub struct Shelf<'a> {
    /// The archive.
    pub archive: &'a Archive,
    /// Its items, listed once per answer.
    pub items: Vec<ItemRef>,
    /// Starts the providers for search, only when it has text to read or a query to embed.
    pub providers: &'a dyn Fn() -> Registry,
}

/// What running a tool gave.
#[derive(Debug, Clone, Default)]
pub struct Ran {
    /// The result, for the model.
    pub text: String,
    /// What it did, in a few words, for people.
    pub summary: String,
    /// The items it came across.
    pub found: Vec<ItemLink>,
    /// The text it gave from items, so quoted passages can be checked against it.
    pub read: Vec<(Id, String)>,
}

impl Ran {
    fn said(text: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            summary: summary.into(),
            ..Self::default()
        }
    }
}

/// `text` cut to `chars` characters, with `…` where it was cut.
fn clip(text: &str, chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    text.chars().take(chars).collect::<String>() + "…"
}

impl Shelf<'_> {
    /// Runs the tool `name` with `input`, for the owner's `question`.
    #[must_use]
    pub fn run(&self, name: &str, input: &str, question: &str) -> Ran {
        let input = input.trim();
        if name == "flagged" {
            return self.flagged(input);
        }
        if input.is_empty() {
            return Ran::said(
                format!("{name} needs an input."),
                format!("{name}, with nothing to look up"),
            );
        }
        match name {
            "search" => self.search(input),
            "collection" => self.collection(input),
            _ => match self.resolve(input) {
                Ok(item) => match name {
                    "show" => self.show(item),
                    "related" => self.related(item),
                    "read" => self.read(item, question),
                    "history" => self.history(item),
                    _ => Ran::said(
                        format!("There's no tool called {name}."),
                        format!("asked for {name}, which isn't a tool"),
                    ),
                },
                Err(why) => Ran::said(why.clone(), format!("{name} {input}: {why}")),
            },
        }
    }

    /// The item `input` names: its identifier or the start of it, its card's or contents'
    /// path, or its title when only one item has it.
    ///
    /// # Errors
    ///
    /// Why no single item matches.
    pub fn resolve(&self, input: &str) -> Result<&ItemRef, String> {
        let input = input
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '`' | '<' | '>' | '[' | ']'))
            .trim_start_matches("id ")
            .trim_start_matches('/');
        if let Ok(uuid) = uuid::Uuid::parse_str(input) {
            let id = Id(uuid);
            return self
                .items
                .iter()
                .find(|i| i.id == id)
                .ok_or_else(|| format!("No item has the id {input}."));
        }
        if let Some(item) = self
            .items
            .iter()
            .find(|i| i.card == input || i.item.as_deref() == Some(input))
        {
            return Ok(item);
        }
        let lower = input.to_lowercase();
        let by_start: Vec<&ItemRef> =
            if lower.len() >= 8 && lower.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                self.items
                    .iter()
                    .filter(|i| i.id.to_string().starts_with(&lower))
                    .collect()
            } else {
                Vec::new()
            };
        if let [item] = by_start.as_slice() {
            return Ok(item);
        }
        let titled: Vec<&ItemRef> = self
            .items
            .iter()
            .filter(|i| {
                self.card(i)
                    .is_some_and(|c| c.title.to_lowercase() == lower)
            })
            .collect();
        match titled.as_slice() {
            [item] => Ok(item),
            [] => Err(format!(
                "No item is {input}; give an id or card path from a search."
            )),
            _ => Err(format!(
                "Several items are called {input}; give an id or card path."
            )),
        }
    }

    /// The card of `item`, if it can be read.
    #[must_use]
    pub fn card(&self, item: &ItemRef) -> Option<Card> {
        self.card_file(item).map(|f| f.card().clone())
    }

    fn card_file(&self, item: &ItemRef) -> Option<CardFile> {
        let text = std::fs::read_to_string(self.archive.root().join(&item.card)).ok()?;
        CardFile::parse(&text).ok()
    }

    /// `item` as a link, with its card's title and sensitivity.
    #[must_use]
    pub fn link(&self, item: &ItemRef) -> ItemLink {
        let card = self.card(item);
        ItemLink {
            id: item.id,
            title: card
                .as_ref()
                .map_or_else(|| item.card.clone(), |c| c.title.clone()),
            card: item.card.clone(),
            collection: folder(&item.card).to_owned(),
            sensitivity: card.map_or(Sensitivity::Personal, |c| c.sensitivity),
        }
    }

    fn search(&self, input: &str) -> Ran {
        let query = match Query::parse(input) {
            Ok(query) => query,
            Err(e) => {
                return Ran::said(
                    format!("That search can't be read: {e}."),
                    format!("search “{input}”: {e}"),
                );
            }
        };
        let searched = crate::search::run::search(
            self.archive,
            &query,
            SEARCH_RESULTS,
            || (self.providers)(),
            &mut |_, _, _| {},
        );
        let searched = match searched {
            Ok(searched) => searched,
            Err(e) => {
                return Ran::said(
                    format!("The search failed: {e}."),
                    format!("search “{input}” failed"),
                );
            }
        };
        let results = &searched.results;
        let mut ran = Ran::default();
        if results.hits.is_empty() {
            ran.text = "Nothing matched.".into();
        } else {
            writeln!(
                ran.text,
                "{} matched; the best {}:",
                results.total,
                results.hits.len()
            )
            .unwrap();
        }
        for hit in &results.hits {
            self.word_hit(hit, &mut ran);
        }
        ran.summary = format!("searched for “{input}”: {} found", results.total);
        ran
    }

    fn word_hit(&self, hit: &Hit, ran: &mut Ran) {
        match &hit.found {
            Found::Item(found) => {
                let restricted = found.sensitivity == Sensitivity::Restricted;
                writeln!(
                    ran.text,
                    "- {} (id {}, card {}, in {}{}{}){}",
                    found.title,
                    found.id,
                    found.card,
                    found.collection,
                    found
                        .date
                        .as_ref()
                        .map(|d| format!(", dated {d}"))
                        .unwrap_or_default(),
                    if found.tags.is_empty() {
                        String::new()
                    } else {
                        format!(", tags {}", found.tags.join(" "))
                    },
                    if restricted {
                        " [restricted: card only]"
                    } else {
                        ""
                    }
                )
                .unwrap();
                if let Some(passage) = hit.passage.as_ref().filter(|_| !restricted) {
                    writeln!(ran.text, "  matched: {}", passage.text).unwrap();
                    ran.read.push((found.id, passage.text.clone()));
                }
                if let Some(item) = self.items.iter().find(|i| i.id == found.id) {
                    ran.found.push(self.link(item));
                }
            }
            Found::Collection { path, title } => {
                writeln!(ran.text, "- the collection {path} ({title})").unwrap();
            }
            Found::Glossary { term } => {
                writeln!(ran.text, "- a glossary fact about {term}").unwrap();
            }
        }
    }

    fn show(&self, item: &ItemRef) -> Ran {
        let Some(file) = self.card_file(item) else {
            return Ran::said(
                "Its card can't be read.",
                format!("couldn't read {}", item.card),
            );
        };
        let card = file.card();
        let mut text = format!(
            "{} (id {}, card {})\nkind: {}\ncollection: {}\nsensitivity: {}\n",
            card.title,
            item.id,
            item.card,
            kind_name(card.kind),
            folder(&item.card),
            crate::correct::level(card.sensitivity)
        );
        if let Some(summary) = &card.summary {
            writeln!(text, "summary: {summary}").unwrap();
        }
        if !card.tags.is_empty() {
            let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
            writeln!(text, "tags: {}", tags.join(", ")).unwrap();
        }
        if let Some(dates) = &card.dates {
            for (name, date) in [
                ("created", &dates.created),
                ("modified", &dates.modified),
                ("added", &dates.added),
                ("worked", &dates.worked),
            ] {
                if let Some(date) = date {
                    writeln!(text, "{name}: {}", date.0).unwrap();
                }
            }
        }
        if let Some(contents) = &item.item {
            writeln!(text, "contents: {contents}").unwrap();
        }
        if let Some(out) = &card.checked_out {
            writeln!(text, "checked out since {}", out.since.0).unwrap();
        }
        let written: Vec<&str> = card
            .generated
            .as_ref()
            .map(|g| g.fields.iter().map(String::as_str).collect())
            .unwrap_or_default();
        writeln!(
            text,
            "written by a model: {}",
            if written.is_empty() {
                "none of it".to_owned()
            } else {
                written.join(", ")
            }
        )
        .unwrap();
        for link in &card.links {
            let target = self.items.iter().find(|i| i.id == link.id);
            writeln!(
                text,
                "link ({}): {}",
                serde_json::to_value(&link.link_type)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                target.map_or_else(|| link.id.to_string(), |t| t.card.clone())
            )
            .unwrap();
        }
        let notes = file.body().trim();
        if !notes.is_empty() {
            writeln!(text, "notes on the card:\n{}", clip(notes, NOTES_CHARS)).unwrap();
        }
        Ran {
            text,
            summary: format!("looked at the card of {}", card.title),
            found: vec![self.link(item)],
            read: vec![(item.id, notes.to_owned())],
        }
    }

    fn related(&self, item: &ItemRef) -> Ran {
        let Some(card) = self.card(item) else {
            return Ran::said(
                "Its card can't be read.",
                format!("couldn't read {}", item.card),
            );
        };
        let mut ran = Ran::default();
        writeln!(ran.text, "Related to {}:", card.title).unwrap();
        for link in &card.links {
            if let Some(other) = self.items.iter().find(|i| i.id == link.id) {
                let other = self.link(other);
                writeln!(
                    ran.text,
                    "- linked: {} (id {}, card {})",
                    other.title, other.id, other.card
                )
                .unwrap();
                ran.found.push(other);
            }
        }
        let near = crate::index::related::open(self.archive).and_then(|index| {
            crate::index::related::nearest(
                self.archive,
                &index,
                (item.id, &card, &item.card),
                &self.items,
            )
        });
        match near {
            Ok(near) => {
                for neighbour in near {
                    if let Some(other) = self.items.iter().find(|i| i.id == neighbour.id) {
                        let other = self.link(other);
                        writeln!(
                            ran.text,
                            "- near in meaning: {} (id {}, card {})",
                            other.title, other.id, other.card
                        )
                        .unwrap();
                        ran.found.push(other);
                    }
                }
            }
            Err(why) => writeln!(ran.text, "{why}").unwrap(),
        }
        if ran.found.is_empty() {
            ran.text.push_str("Nothing is related to it yet.\n");
        }
        ran.summary = format!(
            "looked for items related to {}: {} found",
            card.title,
            ran.found.len()
        );
        ran
    }

    fn read(&self, item: &ItemRef, question: &str) -> Ran {
        let Some(card) = self.card(item) else {
            return Ran::said(
                "Its card can't be read.",
                format!("couldn't read {}", item.card),
            );
        };
        if card.sensitivity == Sensitivity::Restricted {
            return Ran {
                text: format!(
                    "{} is restricted: only its card can be seen. Don't quote or guess its contents.",
                    card.title
                ),
                summary: format!("didn't read {}, which is restricted", card.title),
                found: vec![self.link(item)],
                read: Vec::new(),
            };
        }
        let text = fingerprint(&card).and_then(|c| TextCache::of(self.archive).get(c));
        let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
            return Ran {
                text: format!(
                    "{} has no text Carrel has read; its card is all there is.",
                    card.title
                ),
                summary: format!("found no text in {}", card.title),
                found: vec![self.link(item)],
                read: Vec::new(),
            };
        };
        let ranges = crate::index::chunk::passages(&text);
        let wanted: HashSet<String> = tokens(question).into_iter().map(|t| t.term).collect();
        let mut scored: Vec<(usize, usize)> = ranges
            .iter()
            .enumerate()
            .map(|(n, range)| {
                let hits = tokens(&text[range.clone()])
                    .into_iter()
                    .filter(|t| wanted.contains(&t.term))
                    .count();
                (n, hits)
            })
            .collect();
        // The best-matching passages, the start breaking ties, then in their order.
        scored.sort_by_key(|&(n, hits)| (std::cmp::Reverse(hits), n));
        let mut chosen: Vec<usize> = scored.iter().take(PASSAGES_READ).map(|(n, _)| *n).collect();
        chosen.sort_unstable();
        let mut ran = Ran {
            found: vec![self.link(item)],
            ..Ran::default()
        };
        writeln!(
            ran.text,
            "From {} ({} passages in all):",
            card.title,
            ranges.len()
        )
        .unwrap();
        for n in &chosen {
            let passage = clip(&text[ranges[*n].clone()], PASSAGE_CHARS);
            writeln!(
                ran.text,
                "\nPassage {} of {}:\n{passage}",
                n + 1,
                ranges.len()
            )
            .unwrap();
            ran.read.push((item.id, passage));
        }
        ran.summary = format!(
            "read {} of {} passages of {}",
            chosen.len(),
            ranges.len(),
            card.title
        );
        ran
    }

    fn history(&self, item: &ItemRef) -> Ran {
        let Some(file) = self.card_file(item) else {
            return Ran::said(
                "Its card can't be read.",
                format!("couldn't read {}", item.card),
            );
        };
        let title = file.card().title.clone();
        let mut text = format!("The history of {title}, newest first:\n");
        let mut listed = 0;
        if let Ok(log) = file.history() {
            for entry in log.entries.iter().take(HISTORY_ENTRIES) {
                writeln!(
                    text,
                    "- {} work session{}{}: {}",
                    entry.date,
                    entry
                        .duration
                        .as_ref()
                        .map(|d| format!(" of {d}"))
                        .unwrap_or_default(),
                    if entry.pending {
                        " (not yet approved)"
                    } else {
                        ""
                    },
                    clip(&entry.lines.join(" "), 300)
                )
                .unwrap();
                listed += 1;
            }
        }
        if let Ok(history) = History::open(self.archive)
            && let Ok(entries) = history.item_log(item.id)
        {
            for entry in entries.iter().take(HISTORY_ENTRIES) {
                writeln!(
                    text,
                    "- {}: {}{}",
                    crate::status::local_time(entry.time),
                    entry.summary,
                    entry
                        .agent
                        .as_ref()
                        .map(|a| format!(" (by the {a})"))
                        .unwrap_or_default()
                )
                .unwrap();
                listed += 1;
            }
        }
        if listed == 0 {
            text.push_str("Nothing is recorded yet.\n");
        }
        Ran {
            text,
            summary: format!("looked at the history of {title}"),
            found: vec![self.link(item)],
            read: Vec::new(),
        }
    }

    fn collection(&self, input: &str) -> Ran {
        let wanted = input.trim().trim_matches('/').to_lowercase();
        let collections = self.archive.collections().unwrap_or_default();
        let Some(collection) = collections
            .iter()
            .find(|c| c.path.to_lowercase() == wanted)
            .or_else(|| {
                collections
                    .iter()
                    .find(|c| c.doc.title.to_lowercase() == wanted)
            })
        else {
            return Ran::said(
                format!("There's no collection {input}."),
                format!("looked for a collection {input}, which isn't there"),
            );
        };
        let mut text = format!(
            "The collection {} ({})",
            collection.path, collection.doc.title
        );
        if let Some(description) = &collection.doc.description {
            write!(text, ": {description}").unwrap();
        }
        text.push('\n');
        let document = std::fs::read_to_string(
            self.archive
                .root()
                .join(&collection.path)
                .join("_collection.md"),
        )
        .unwrap_or_default();
        if let Ok(split) = crate::frontmatter::split(&document) {
            let body = split.body.trim();
            if !body.is_empty() {
                writeln!(text, "Its design document:\n{}", clip(body, NOTES_CHARS)).unwrap();
            }
        }
        let below: Vec<&str> = collections
            .iter()
            .filter(|c| folder(&c.path) == collection.path)
            .map(|c| c.path.as_str())
            .collect();
        if !below.is_empty() {
            writeln!(text, "Collections in it: {}", below.join(", ")).unwrap();
        }
        let items: Vec<&ItemRef> = self
            .items
            .iter()
            .filter(|i| folder(&i.card) == collection.path && i.kind != Kind::Tombstone)
            .collect();
        writeln!(text, "{} items in it directly:", items.len()).unwrap();
        let mut found = Vec::new();
        for item in items.iter().take(COLLECTION_ITEMS) {
            let link = self.link(item);
            writeln!(
                text,
                "- {} (id {}, card {})",
                link.title, link.id, link.card
            )
            .unwrap();
            found.push(link);
        }
        if items.len() > COLLECTION_ITEMS {
            writeln!(text, "- and {} more", items.len() - COLLECTION_ITEMS).unwrap();
        }
        Ran {
            text,
            summary: format!("looked at the collection {}", collection.path),
            found,
            read: Vec::new(),
        }
    }
}

impl Shelf<'_> {
    /// What the owner flagged as needing work, in the collection `input` names and below, or
    /// everywhere for `all` or nothing.
    fn flagged(&self, input: &str) -> Ran {
        let within = input.trim().trim_matches('/');
        let within = if within.is_empty() || within.eq_ignore_ascii_case("all") {
            None
        } else {
            Some(within.to_lowercase())
        };
        let inside = |path: &str| {
            within.as_deref().is_none_or(|w| {
                let path = path.to_lowercase();
                path == w || path.starts_with(&format!("{w}/"))
            })
        };
        let mut flagged = crate::flags::flagged_collections(self.archive).unwrap_or_default();
        for item in &self.items {
            if let Some(card) = self.card(item) {
                flagged.extend(crate::flags::flagged_item(item, &card));
            }
        }
        flagged.retain(|f| inside(f.what.path()));
        let place = within
            .as_deref()
            .map_or_else(|| "the archive".to_owned(), str::to_owned);
        if flagged.is_empty() {
            return Ran::said(
                format!("Nothing in {place} is flagged as needing work."),
                format!("looked for flags in {place}, and found none"),
            );
        }
        let mut text = format!("Flagged as needing work in {place}:\n");
        let mut found = Vec::new();
        for f in &flagged {
            let what = crate::flags::describe(&f.flag);
            match &f.what {
                crate::flags::Flaggable::Collection { path } => {
                    writeln!(text, "- the collection {path} ({}): {what}", f.title).unwrap();
                }
                crate::flags::Flaggable::Item { id, card } => {
                    writeln!(text, "- {} (id {id}, card {card}): {what}", f.title).unwrap();
                    if let Some(item) = self.items.iter().find(|i| i.id == *id) {
                        found.push(self.link(item));
                    }
                }
            }
        }
        Ran {
            text,
            summary: format!("listed {} flagged in {place}", flagged.len()),
            found,
            read: Vec::new(),
        }
    }
}

/// The folder a path is in, relative to the archive root.
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(f, _)| f)
}

fn kind_name(kind: Kind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
