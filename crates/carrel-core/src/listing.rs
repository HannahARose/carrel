//! The generated listings that make an archive browsable (FORMAT.md §6.2, §6.3): each
//! collection's `listing` section, linking its parent, subcollections and items, and
//! `ARCHIVE.md`'s `collections` section, linking the top-level collections.
//!
//! Listings are rebuilt from the cards, and a document is rewritten only when its section
//! actually changed (table padding doesn't count). Nothing outside the sections is touched, and
//! a document whose markers are broken is reported, not guessed at (§5.3).

use std::fmt::Write as _;
use std::path::Path;

use crate::archive::{Archive, ArchiveError, Collection, escape_link_text};
use crate::card::Card;
use crate::sections;

/// What refreshing the listings did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Refreshed {
    /// The documents rewritten, relative to the archive root.
    pub changed: Vec<String>,
    /// Documents that couldn't be updated, and why.
    pub problems: Vec<String>,
}

/// A path percent-encoded for a Markdown link destination (FORMAT.md §2.5).
#[must_use]
pub fn link_destination(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            // Not required by §2.5, but a bare `|` would end a table cell.
            '|' => out.push_str("%7C"),
            _ => out.push(ch),
        }
    }
    out
}

/// A table cell's text: `|` escaped and line breaks flattened, so it stays one cell.
fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// The collection `path` belongs to: the nearest one above it, if any.
fn parent_of<'a>(collections: &'a [Collection], path: &str) -> Option<&'a Collection> {
    collections
        .iter()
        .filter(|c| path.starts_with(&format!("{}/", c.path)))
        .max_by_key(|c| c.path.len())
}

/// A relative link from the folder `from` to `to`, both relative to the archive root.
fn relative(from: &str, to: &str) -> String {
    let ups = if from.is_empty() {
        0
    } else {
        from.split('/').count()
    };
    let common = from
        .split('/')
        .zip(to.split('/'))
        .take_while(|(a, b)| a == b)
        .count();
    let common = if from.is_empty() { 0 } else { common };
    let mut out = "../".repeat(ups - common);
    out.push_str(&to.split('/').skip(common).collect::<Vec<_>>().join("/"));
    out
}

/// Each collection's item count: those directly in it, and those in its subcollections.
fn counts(collections: &[Collection], cards: &[Vec<(String, Card)>]) -> Vec<(usize, usize)> {
    collections
        .iter()
        .map(|c| {
            let inside: usize = collections
                .iter()
                .zip(cards)
                .filter(|(other, _)| other.path.starts_with(&format!("{}/", c.path)))
                .map(|(_, items)| items.len())
                .sum();
            let own = collections
                .iter()
                .position(|o| o.path == c.path)
                .map_or(0, |i| cards[i].len());
            (own, inside)
        })
        .collect()
}

fn count_cell((own, inside): (usize, usize)) -> String {
    if inside == 0 {
        own.to_string()
    } else {
        format!("{own} (+{inside} in subcollections)")
    }
}

/// The date an item joined its collection: `dates.added`, or for a tombstone, when it was
/// removed.
fn added(card: &Card) -> String {
    let date = card
        .dates
        .as_ref()
        .and_then(|d| d.added.as_ref())
        .or(card.removed.as_ref().map(|r| &r.at))
        .map(|d| d.0.as_str())
        .unwrap_or_default();
    date.get(..10).unwrap_or(date).to_owned()
}

/// The content of a collection's `listing` section.
fn listing(
    archive: &Archive,
    collections: &[Collection],
    cards: &[Vec<(String, Card)>],
    counts: &[(usize, usize)],
    index: usize,
) -> String {
    let collection = &collections[index];
    let mut out = String::from("## Contents\n\n");
    match parent_of(collections, &collection.path) {
        Some(parent) => writeln!(
            out,
            "Part of [{}]({})",
            escape_link_text(&parent.doc.title),
            link_destination(&relative(
                &collection.path,
                &format!("{}/_collection.md", parent.path)
            ))
        ),
        None => writeln!(
            out,
            "Part of [{}]({})",
            escape_link_text(&archive.doc().name),
            link_destination(&relative(&collection.path, "ARCHIVE.md"))
        ),
    }
    .unwrap();

    let children: Vec<usize> = collections
        .iter()
        .enumerate()
        .filter(|(_, c)| parent_of(collections, &c.path).is_some_and(|p| p.path == collection.path))
        .map(|(i, _)| i)
        .collect();
    if !children.is_empty() {
        out.push_str(
            "\n### Subcollections\n\n| Collection | Items | Description |\n| --- | --- | --- |\n",
        );
        for i in children {
            let child = &collections[i];
            writeln!(
                out,
                "| [{}]({}) | {} | {} |",
                cell(&escape_link_text(&child.doc.title)),
                link_destination(&relative(
                    &collection.path,
                    &format!("{}/_collection.md", child.path)
                )),
                count_cell(counts[i]),
                cell(child.doc.description.as_deref().unwrap_or(""))
            )
            .unwrap();
        }
    }

    let mut items: Vec<&(String, Card)> = cards[index].iter().collect();
    items.sort_by(|(_, a), (_, b)| {
        added(a)
            .cmp(&added(b))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    if !items.is_empty() {
        out.push_str("\n### Items\n\n| Item | Kind | Added |\n| --- | --- | --- |\n");
        for (name, card) in items {
            let kind = serde_json::to_value(card.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let added = added(card);
            writeln!(
                out,
                "| [{}]({}) | {kind} | {added} |",
                cell(&escape_link_text(&card.title)),
                link_destination(name)
            )
            .unwrap();
        }
    }
    out
}

/// The content of `ARCHIVE.md`'s `collections` section.
fn top_level(collections: &[Collection], counts: &[(usize, usize)]) -> String {
    let mut out = String::from(
        "## Collections\n\n| Collection | Items | Description |\n| --- | --- | --- |\n",
    );
    for (i, collection) in collections.iter().enumerate() {
        if parent_of(collections, &collection.path).is_some() {
            continue;
        }
        writeln!(
            out,
            "| [{}]({}) | {} | {} |",
            cell(&escape_link_text(&collection.doc.title)),
            link_destination(&format!("{}/_collection.md", collection.path)),
            count_cell(counts[i]),
            cell(collection.doc.description.as_deref().unwrap_or(""))
        )
        .unwrap();
    }
    out
}

/// Replaces section `name` of the document at `path` with `content` when it differs. Returns
/// whether the document changed.
fn update(root: &Path, path: &str, name: &str, content: &str) -> Result<bool, String> {
    let file = root.join(path);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
    let current = sections::get(&text, name).map_err(|e| format!("{path}: {e}"))?;
    let updated = match current {
        Some(current) if sections::equivalent(current, content) => return Ok(false),
        Some(_) => sections::replace(&text, name, content),
        None => sections::append_section(&text, name, content),
    }
    .map_err(|e| format!("{path}: {e}"))?;
    let temp = file.with_extension(format!("tmp-{}", crate::card::Id::new_v7()));
    std::fs::write(&temp, updated).map_err(|e| format!("{path}: {e}"))?;
    std::fs::rename(&temp, &file).map_err(|e| format!("{path}: {e}"))?;
    Ok(true)
}

/// Rebuilds every listing in the archive, rewriting only those that changed.
///
/// # Errors
///
/// [`ArchiveError`] if the collections or cards can't be read. Documents that can't be
/// updated are reported in [`Refreshed::problems`].
pub fn refresh(archive: &Archive) -> Result<Refreshed, ArchiveError> {
    let collections = archive.collections()?;
    let cards = collections
        .iter()
        .map(|c| archive.cards_in(c))
        .collect::<Result<Vec<_>, _>>()?;
    let counts = counts(&collections, &cards);
    let mut refreshed = Refreshed::default();
    let mut note = |path: String, result: Result<bool, String>| match result {
        Ok(true) => refreshed.changed.push(path),
        Ok(false) => {}
        Err(problem) => refreshed.problems.push(problem),
    };
    for (i, collection) in collections.iter().enumerate() {
        let path = format!("{}/_collection.md", collection.path);
        let content = listing(archive, &collections, &cards, &counts, i);
        let result = update(archive.root(), &path, "listing", &content);
        note(path, result);
    }
    let result = update(
        archive.root(),
        "ARCHIVE.md",
        "collections",
        &top_level(&collections, &counts),
    );
    note("ARCHIVE.md".into(), result);
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::relative;

    #[test]
    fn link_destinations_encode_what_would_break_a_link_or_a_table() {
        assert_eq!(
            super::link_destination("a b (1) #2?<x>|100%.md"),
            "a%20b%20%281%29%20%232%3F%3Cx%3E%7C100%25.md"
        );
    }

    #[test]
    fn relative_links_climb_to_the_common_folder() {
        assert_eq!(relative("Projects", "ARCHIVE.md"), "../ARCHIVE.md");
        assert_eq!(
            relative("Projects/Hardware", "Projects/_collection.md"),
            "../_collection.md"
        );
        assert_eq!(
            relative("Projects", "Projects/Hardware/_collection.md"),
            "Hardware/_collection.md"
        );
        assert_eq!(
            relative("Research/2026/Papers", "Research/_collection.md"),
            "../../_collection.md"
        );
    }
}
