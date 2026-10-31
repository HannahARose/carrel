//! `carrel search QUERY`: finds items by the words in their cards and text, narrowed by
//! filters, with the passage that matched (`docs/design/search.md`).

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;

use carrel_core::card::Sensitivity;
use carrel_core::search::query::Query;
use carrel_core::search::texts::{fill, tidy};
use carrel_core::search::{Found, Hit, Meaning, Search};
use serde_json::json;

use crate::commands::{locate, plural};
use crate::{Failure, Report};

/// How many results are shown unless `--limit` says otherwise.
pub const DEFAULT_LIMIT: usize = 20;

/// Reads the text of items not in the text cache yet, showing progress on standard error.
/// Providers are only started when there's something to read.
fn catch_up(archive: &carrel_core::archive::Archive, json: bool) -> Result<(), Failure> {
    let (wanted, _) = tidy(archive)?;
    if wanted.is_empty() {
        return Ok(());
    }
    let (registry, _) = crate::providers::registry();
    let mut display = crate::progress::Display::new("Reading text", json);
    fill(archive, &registry, wanted, &mut |done, total, item| {
        display.count("items", done, total, item);
    })?;
    display.finish();
    Ok(())
}

/// `carrel search WORDS... [--limit N]`.
///
/// # Errors
///
/// [`Failure`] if there's no archive, the query has a filter that can't be used, or the
/// archive can't be read.
pub fn search(
    archive: Option<&Path>,
    words: &[String],
    limit: usize,
    json: bool,
) -> Result<Report, Failure> {
    let typed = words.join(" ");
    let query = Query::parse(&typed)?;
    if query.text.trim().is_empty() && query.filters.is_empty() {
        return Err(Failure::new(
            "search for some words, a filter such as tag:receipts, or both",
        ));
    }
    let archive = locate(archive)?;
    catch_up(&archive, json)?;
    let results = Search::build(&archive)?.search(&query, limit);
    let marks = if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        ("\x1b[1m", "\x1b[0m")
    } else {
        ("**", "**")
    };
    let mut text = String::new();
    if results.hits.is_empty() {
        writeln!(text, "Nothing found for “{typed}”.").unwrap();
    } else if results.total > results.hits.len() {
        writeln!(
            text,
            "The best {} of {} for “{typed}”:",
            results.hits.len(),
            plural(results.total, "result")
        )
        .unwrap();
    } else {
        writeln!(text, "{} for “{typed}”:", plural(results.total, "result")).unwrap();
    }
    for hit in &results.hits {
        text.push('\n');
        describe(&mut text, hit, marks);
    }
    if let Meaning::Behind { items } = results.meaning {
        writeln!(
            text,
            "\nThe meaning index is behind by {}; they're found by their words until it catches up.",
            plural(items, "item")
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "query": typed,
            "total": results.total,
            "meaning": results.meaning,
            "results": results.hits,
        }),
    ))
}

/// One result: its title and sensitivity mark, where it is, when, and the passage that
/// matched.
fn describe(text: &mut String, hit: &Hit, (open, close): (&str, &str)) {
    match &hit.found {
        Found::Item(item) => {
            let mark = match item.sensitivity {
                Sensitivity::Restricted => "  [restricted]",
                Sensitivity::Sensitive => "  [sensitive]",
                _ => "",
            };
            writeln!(text, "{}{mark}", item.title).unwrap();
            let mut about = vec![item.collection.clone()];
            about.extend(
                item.date
                    .as_ref()
                    .map(|d| d.get(..10).unwrap_or(d).to_owned()),
            );
            about.push(serde_json::to_value(item.kind).map_or_else(
                |_| String::new(),
                |v| v.as_str().unwrap_or_default().to_owned(),
            ));
            if item.checked_out {
                about.push("checked out".into());
            }
            writeln!(text, "  {}", about.join(" · ")).unwrap();
        }
        Found::Collection { path, title } => {
            writeln!(text, "{title}\n  the collection {path}").unwrap();
        }
        Found::Glossary { term } => writeln!(text, "{term}\n  in the glossary").unwrap(),
    }
    if let Some(passage) = &hit.passage {
        writeln!(text, "  {}", passage.marked(open, close)).unwrap();
    }
    let why: Vec<String> = hit.why.iter().map(ToString::to_string).collect();
    writeln!(text, "  matched: {}", why.join(", ")).unwrap();
}
