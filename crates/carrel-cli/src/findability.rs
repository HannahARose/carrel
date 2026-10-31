//! `carrel bench --search`: the findability test (`docs/design/search.md`, "Measuring it").
//! It files the test set into a copy of its archive, then looks up every hand-written
//! description in `descriptions.yaml` by words alone, by meaning alone and by both, with each
//! embedding model on offer, and writes `results.json` and `report.md` to `search/` in the
//! output folder.
//!
//! The filed archive is kept in `search/archive`, and each model's index inside it, so a
//! second run embeds only what changed; `--fresh` starts over.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use carrel_core::archive::Archive;
use carrel_core::bench::findability::{
    self, Descriptions, Drafted, Findability, Lookup, by_meaning, by_search, keep_places, look_up,
    meaning_places, names, word_places,
};
use carrel_core::bench::{Answers, check, percent};
use carrel_core::card::Id;
use carrel_core::index::{ByMeaning, Embedder, Index, embed_query, plan};
use carrel_core::providers::{Capability, Registry};
use carrel_core::search::{Merge, Search};
use serde_json::{Value, json};

use crate::bench::BenchArgs;
use crate::{Failure, Report};

/// How the items were looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Words,
    Meaning,
    Both,
}

impl Way {
    fn label(self) -> &'static str {
        match self {
            Self::Words => "Words alone",
            Self::Meaning => "Meaning alone",
            Self::Both => "Words and meaning",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Words => "words",
            Self::Meaning => "meaning",
            Self::Both => "both",
        }
    }
}

/// One way of looking, with one model, and where each description's item came back.
struct Row {
    way: Way,
    model: Option<String>,
    score: Findability,
    lookups: Vec<Lookup>,
}

impl Row {
    fn new(way: Way, model: Option<&str>, lookups: Vec<Lookup>) -> Self {
        Self {
            way,
            model: model.map(str::to_owned),
            score: findability::score(&lookups),
            lookups,
        }
    }
}

/// `carrel bench --search`.
///
/// # Errors
///
/// [`Failure`] if the test set or its descriptions are inconsistent, a model asked for isn't
/// on offer, or the archive or results can't be written.
pub fn findability(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let corpus = args.corpus.as_path();
    let (answers, mut descriptions) = load(corpus)?;
    if let Some(limit) = args.limit {
        descriptions.items = descriptions.items.into_iter().take(limit).collect();
    }
    let dir = args.out.join("search");
    if args.fresh {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let drafts = load_drafts(&args.cards)?;
    let archive = findability::build(
        &corpus.join("archive"),
        &corpus.join("items"),
        &answers,
        &drafts,
        &dir.join("archive"),
    )
    .map_err(Failure::new)?;
    let registry = Rc::new(crate::providers::registry().0);
    crate::search::read_texts(&archive, &registry, json)?;
    let models = embedding_models(&registry, &args.models)?;
    let names = names(&archive).map_err(Failure::new)?;

    let words = Search::build(&archive)?;
    let mut by_words = look_up(&descriptions, &mut |d| by_search(&words, d));
    keep_places(&mut by_words, &mut |d| word_places(&words, d));
    let mut rows = vec![Row::new(Way::Words, None, by_words)];
    let mut notes = Vec::new();
    if models.is_empty() {
        notes.push(
            "No embedding model is on offer on this device, so only words were measured; \
             see `carrel models`."
                .to_owned(),
        );
    }
    for model in &models {
        match measure(&archive, &registry, model, &descriptions, &names, json) {
            Ok((note, measured)) => {
                notes.push(note);
                rows.extend(measured);
            }
            Err(e) => notes.push(format!("{model} wasn't measured: {}", e.0)),
        }
    }

    let setup = Setup {
        items: names.len(),
        descriptions: descriptions.len(),
        cards: &args.cards,
        drafted: drafts.len(),
    };
    let results = results_json(&rows, &notes, &setup);
    let results_path = dir.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = dir.join("report.md");
    std::fs::write(&report_path, report_markdown(&rows, &notes, &setup))
        .map_err(|e| Failure::io(&report_path, &e))?;

    let mut text = summary_table(&rows);
    for note in &notes {
        writeln!(text, "\n{note}").unwrap();
    }
    writeln!(
        text,
        "\nWrote {} and {}.",
        results_path.display(),
        report_path.display()
    )
    .unwrap();
    Ok(Report::new(text, results))
}

/// The answers and descriptions in `corpus`, once both are known to agree with the test set.
fn load(corpus: &Path) -> Result<(Answers, Descriptions), Failure> {
    let answers = Answers::load(&corpus.join("answers.yaml")).map_err(Failure::new)?;
    let descriptions =
        Descriptions::load(&corpus.join("descriptions.yaml")).map_err(Failure::new)?;
    let fixture = Archive::open(&corpus.join("archive"))?;
    let mut problems = check(&answers, &fixture, &corpus.join("items")).map_err(Failure::new)?;
    problems.extend(findability::check(&descriptions, &answers));
    if problems.is_empty() {
        Ok((answers, descriptions))
    } else {
        Err(Failure::new(format!(
            "the test set is inconsistent:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// The cards drafted in each of `paths`, earlier benchmark runs' `results.json`; an item in
/// more than one takes the first.
fn load_drafts(paths: &[PathBuf]) -> Result<BTreeMap<String, Drafted>, Failure> {
    let mut drafts = BTreeMap::new();
    for path in paths {
        let text = std::fs::read_to_string(path).map_err(|e| Failure::io(path, &e))?;
        let results: Value = serde_json::from_str(&text)
            .map_err(|e| Failure::new(format!("{}: {e}", path.display())))?;
        for (item, draft) in findability::drafts(&results) {
            drafts.entry(item).or_insert(draft);
        }
    }
    Ok(drafts)
}

/// The embedding models to measure: those named, each of which must be on offer, or every
/// one on offer.
fn embedding_models(registry: &Registry, wanted: &[String]) -> Result<Vec<String>, Failure> {
    let mut offered: Vec<String> = Vec::new();
    for (_, offer) in registry.ranked(Capability::Embed) {
        if !offered.contains(&offer.id) {
            offered.push(offer.id.clone());
        }
    }
    if wanted.is_empty() {
        return Ok(offered);
    }
    for name in wanted {
        if !offered.contains(name) {
            return Err(Failure::new(format!(
                "no embedding model {name} is on offer; on offer: {}",
                if offered.is_empty() {
                    "none (see `carrel models`)".to_owned()
                } else {
                    offered.join(", ")
                }
            )));
        }
    }
    Ok(wanted.to_vec())
}

/// Brings `model`'s index of `archive` up to date, reusing what an earlier run embedded, then
/// looks up every description by meaning alone and by both. Returns what was reused, and the
/// two rows.
fn measure(
    archive: &Archive,
    registry: &Rc<Registry>,
    model: &str,
    descriptions: &Descriptions,
    names: &HashMap<Id, String>,
    json: bool,
) -> Result<(String, Vec<Row>), Failure> {
    let existing = Index::open(archive, model).map_err(|e| crate::index::failure(e, model))?;
    let reused = existing.as_ref().is_some_and(|i| !i.is_empty());
    let mut index = existing.unwrap_or_else(|| Index::new(archive, model));
    let current = plan(archive, Some(&index))
        .map_err(|e| crate::index::failure(e, model))?
        .current();
    let embedded = crate::index::catch_up(archive, registry, &mut index, json)?;
    let place = Index::dir_of(archive, model);
    let note = if reused {
        format!(
            "Reused the index for {model} from an earlier run ({}): {current} items were \
             current, {embedded} embedded again.",
            place.display()
        )
    } else {
        format!(
            "Built an index for {model} ({}), embedding {embedded} items; the next run reuses it.",
            place.display()
        )
    };

    let embed = shared_embedder(registry, model);
    let meaning = |archive: &Archive| -> Result<Box<ByMeaning>, Failure> {
        let index = Index::open(archive, model)
            .map_err(|e| crate::index::failure(e, model))?
            .unwrap_or_else(|| Index::new(archive, model));
        let embed = Rc::clone(&embed);
        Ok(Box::new(ByMeaning::new(
            archive,
            Box::new(index),
            0,
            Box::new(move |q: &str| embed(q)),
        )))
    };
    let alone = meaning(archive)?;
    let mut by_meaning_alone = look_up(descriptions, &mut |d| by_meaning(&*alone, names, d));
    keep_places(&mut by_meaning_alone, &mut |d| {
        meaning_places(&*alone, names, d)
    });
    let both = Search::build(archive)?
        .with_meaning(meaning(archive)?)
        .with_merge(Merge::for_model(model));
    let by_both = look_up(descriptions, &mut |d| by_search(&both, d));
    Ok((
        note,
        vec![
            Row::new(Way::Meaning, Some(model), by_meaning_alone),
            Row::new(Way::Both, Some(model), by_both),
        ],
    ))
}

/// Embeds queries with `model`, each only once however many rankings ask for it.
fn shared_embedder(registry: &Rc<Registry>, model: &str) -> Rc<Embedder> {
    let registry = Rc::clone(registry);
    let model = model.to_owned();
    let asked: RefCell<HashMap<String, Option<Vec<f32>>>> = RefCell::new(HashMap::new());
    Rc::new(Box::new(move |query: &str| {
        if let Some(vector) = asked.borrow().get(query) {
            return vector.clone();
        }
        let vector = embed_query(&registry, &model, query).ok();
        asked.borrow_mut().insert(query.to_owned(), vector.clone());
        vector
    }))
}

/// What the run looked through.
struct Setup<'a> {
    items: usize,
    descriptions: usize,
    cards: &'a [PathBuf],
    drafted: usize,
}

impl Setup<'_> {
    fn cards(&self) -> String {
        if self.cards.is_empty() {
            "titled by their file names, with the expected tags, dates and sensitivity".to_owned()
        } else {
            format!(
                "with the titles and summaries drafted in {} ({} items; the rest titled by \
                 their file names), and the expected tags, dates and sensitivity",
                self.cards
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                self.drafted
            )
        }
    }
}

fn results_json(rows: &[Row], notes: &[String], setup: &Setup<'_>) -> Value {
    json!({
        "carrel": carrel_core::CARREL_VERSION,
        "date": jiff::Zoned::now().date().to_string(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "items": setup.items,
        "descriptions": setup.descriptions,
        "cards": setup.cards,
        "top": findability::TOP,
        "depth": findability::DEPTH,
        "notes": notes,
        "rankings": rows.iter().map(|r| json!({
            "way": r.way.key(),
            "model": r.model,
            "score": r.score,
            "lookups": r.lookups,
        })).collect::<Vec<_>>(),
    })
}

/// One row per ranking and model: top-five recall and mean reciprocal rank.
fn summary_table(rows: &[Row]) -> String {
    let mut text = String::from(
        "| Ranking | Model | Top five | Mean reciprocal rank | Missed |\n| --- | --- | --- | --- | --- |\n",
    );
    for row in rows {
        writeln!(
            text,
            "| {} | {} | {} | {:.2} | {} |",
            row.way.label(),
            row.model.as_deref().unwrap_or("none"),
            percent(row.score.top_five),
            row.score.reciprocal_rank,
            row.lookups.iter().filter(|l| !l.found()).count(),
        )
        .unwrap();
    }
    text
}

/// A table cell's text: one line, with pipes escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn report_markdown(rows: &[Row], notes: &[String], setup: &Setup<'_>) -> String {
    let mut text = format!(
        "# Findability report\n\n{} on {} {}, carrel {}.\n\n{} descriptions of {} items, \
         written by hand (`descriptions.yaml`), each looked up as a search. The items were \
         filed in their expected collections, {}. A description counts as found when its item \
         is among the first {} items that come back; the mean reciprocal rank counts every \
         place up to {}.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION,
        setup.descriptions,
        setup.items,
        setup.cards(),
        findability::TOP,
        findability::DEPTH,
    );
    text.push_str(&summary_table(rows));
    for note in notes {
        write!(text, "\n{note}\n").unwrap();
    }
    text.push_str("\n## Not in the top five\n");
    for row in rows {
        let missed: Vec<&Lookup> = row.lookups.iter().filter(|l| !l.found()).collect();
        let model = row
            .model
            .as_deref()
            .map(|m| format!(", {m}"))
            .unwrap_or_default();
        write!(text, "\n### {}{model}\n\n", row.way.label()).unwrap();
        if missed.is_empty() {
            text.push_str("Every description found its item.\n");
            continue;
        }
        text.push_str(
            "| Item | Description | Place | Came back first |\n| --- | --- | --- | --- |\n",
        );
        for lookup in missed {
            writeln!(
                text,
                "| {} | {} | {} | {} |",
                cell(&lookup.item),
                cell(&lookup.description),
                lookup
                    .rank
                    .map_or_else(|| "not found".to_owned(), |r| r.to_string()),
                cell(&lookup.top.join(", ")),
            )
            .unwrap();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(item: &str, rank: Option<usize>) -> Lookup {
        Lookup {
            item: item.into(),
            description: format!("the {item} | with a pipe"),
            rank,
            top: vec!["other.txt".into()],
            places: Vec::new(),
        }
    }

    #[test]
    fn the_report_lists_what_was_missed_for_each_ranking() {
        let rows = vec![
            Row::new(
                Way::Words,
                None,
                vec![lookup("a.txt", Some(1)), lookup("b.txt", None)],
            ),
            Row::new(Way::Both, Some("m"), vec![lookup("a.txt", Some(2))]),
        ];
        let setup = Setup {
            items: 2,
            descriptions: 2,
            cards: &[],
            drafted: 0,
        };
        let report = report_markdown(&rows, &["Reused the index for m.".into()], &setup);
        assert!(
            report.contains("| Words alone | none | 50% | 0.50 | 1 |"),
            "{report}"
        );
        assert!(
            report.contains("| Words and meaning | m | 100% | 0.50 | 0 |"),
            "{report}"
        );
        assert!(report.contains("Reused the index for m."), "{report}");
        assert!(
            report.contains("| b.txt | the b.txt \\| with a pipe | not found | other.txt |"),
            "{report}"
        );
        assert!(report.contains("### Words and meaning, m\n\nEvery description found"));
    }
}
