//! `/` in Browse: the same search as `carrel search` (`docs/design/search.md`), with its
//! filters and the reason each result came back.
//!
//! It searches the text already read. `carrel search` reads any item's text not read yet
//! first, which starts the providers; here that would hold up the screen, so those items are
//! found by their cards until `carrel search` has read them. Items drafted since search arrived
//! kept their text when they were drafted.

use std::fmt::Write as _;

use carrel_core::search::query::Query;
use carrel_core::search::{Found, Hit, Search};

use super::app::App;

/// The most results Browse lists.
pub const LIMIT: usize = 100;

/// What the search shown in Browse found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Searched {
    /// The items found, best first, in the order Browse lists them.
    pub hits: Vec<Hit>,
    /// How many items matched, before the limit.
    pub total: usize,
    /// How many collections and glossary facts matched too, which `carrel search` lists.
    pub others: usize,
}

impl Searched {
    /// The chosen item's result, if a search is shown.
    pub fn hit(&self, index: usize) -> Option<&Hit> {
        self.hits.get(index)
    }
}

impl App {
    /// Runs the search `typed` in Browse: the items found, as card paths and titles.
    pub(super) fn find(&mut self, typed: &str) -> Vec<(String, String)> {
        self.searched = Searched::default();
        let query = match Query::parse(typed) {
            Ok(query) => query,
            Err(e) => {
                self.message = Some(e.0);
                return Vec::new();
            }
        };
        let results = match Search::build(&self.archive) {
            Ok(search) => search.search(&query, usize::MAX),
            Err(e) => {
                self.message = Some(format!("Couldn't search: {e}"));
                return Vec::new();
            }
        };
        let (items, others): (Vec<Hit>, Vec<Hit>) = results
            .hits
            .into_iter()
            .partition(|hit| matches!(hit.found, Found::Item(_)));
        self.searched.total = items.len();
        self.searched.others = others.len();
        self.searched.hits = items.into_iter().take(LIMIT).collect();
        self.searched
            .hits
            .iter()
            .filter_map(|hit| match &hit.found {
                Found::Item(item) => Some((item.card.clone(), item.title.clone())),
                _ => None,
            })
            .collect()
    }

    /// The title of Browse's list while a search is shown.
    pub(super) fn search_title(&self, typed: &str) -> String {
        let Searched {
            hits,
            total,
            others,
        } = &self.searched;
        let mut title = if hits.len() < *total {
            format!(" The best {} of {total} for “{typed}”", hits.len())
        } else {
            format!(" {total} found for “{typed}”")
        };
        if *others > 0 {
            write!(
                title,
                ", and {others} in collections or the glossary (carrel search)"
            )
            .unwrap();
        }
        title.push_str(" (Esc clears) ");
        title
    }
}
