//! The timeline: items by their date, by year, month or day, narrowed like search.

use std::collections::{BTreeMap, HashSet};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::{Catalog, DateKind, Entry, Item};
use crate::archive::{Archive, ArchiveError};
use crate::card::Id;
use crate::search::query::{Filters, Query};
use crate::search::{Found, FoundItem, Search};

/// How finely the timeline groups items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    /// By year: `2025`.
    Year,
    /// By month: `2025-03`.
    #[default]
    Month,
    /// By day: `2025-03-14`.
    Day,
}

impl Resolution {
    /// Reads `year`, `month` or `day`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_lowercase().as_str() {
            "year" | "years" => Some(Self::Year),
            "month" | "months" => Some(Self::Month),
            "day" | "days" => Some(Self::Day),
            _ => None,
        }
    }

    /// The period `date` falls in, as written: `2025`, `2025-03` or `2025-03-14`.
    #[must_use]
    pub fn period(self, date: Date) -> String {
        let text = date.to_string();
        let len = match self {
            Self::Year => 4,
            Self::Month => 7,
            Self::Day => 10,
        };
        text.get(..len).unwrap_or(&text).to_owned()
    }
}

/// What the timeline shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Which date it goes by. With none, each item is placed once, by when it was made, else
    /// when it was filed, as search dates it.
    pub by: Option<DateKind>,
    /// How finely it groups.
    pub resolution: Resolution,
    /// Search's filters; `after:` and `before:` apply to the date the timeline goes by.
    pub filters: Filters,
}

/// One item on the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// The item.
    pub item: Item,
    /// The date it's placed by, as `YYYY-MM-DD`.
    pub date: String,
    /// Which of its dates that is.
    pub what: DateKind,
}

/// One period's items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Period {
    /// The period: `2025`, `2025-03` or `2025-03-14`.
    pub period: String,
    /// Its items, oldest first.
    pub items: Vec<Placed>,
}

/// Items by date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    /// How finely it's grouped.
    pub resolution: Resolution,
    /// Which date it goes by; none for each item's own.
    pub by: Option<DateKind>,
    /// The periods that have items, oldest first.
    pub periods: Vec<Period>,
    /// How many items have no date of the kind it goes by, and so aren't on it.
    pub undated: usize,
}

impl Timeline {
    /// How many places on the timeline there are in all: an item worked on in three months
    /// is counted in each.
    #[must_use]
    pub fn total(&self) -> usize {
        self.periods.iter().map(|p| p.items.len()).sum()
    }
}

/// The dates `entry` is placed by.
fn placings(entry: &Entry, by: Option<DateKind>) -> Vec<(Date, DateKind)> {
    match by {
        Some(kind) => entry.dates(kind).into_iter().map(|d| (d, kind)).collect(),
        None => entry
            .created
            .map(|d| (d, DateKind::Created))
            .or_else(|| entry.filed.map(|d| (d, DateKind::Filed)))
            .into_iter()
            .collect(),
    }
}

fn as_found(entry: &Entry, date: Date) -> FoundItem {
    FoundItem {
        id: entry.item.id,
        title: entry.item.title.clone(),
        card: entry.item.card.clone(),
        item: entry.item.item.clone(),
        collection: entry.item.collection.clone(),
        kind: entry.item.kind,
        sensitivity: entry.item.sensitivity,
        date: Some(date.to_string()),
        tags: entry.tags.clone(),
        checked_out: entry.checked_out,
    }
}

/// The timeline of `catalog`, as `options` asks for, of only the items in `only` when given
/// (such as those a search's words found, from [`matching`]).
#[must_use]
pub fn timeline(catalog: &Catalog, options: &Options, only: Option<&HashSet<Id>>) -> Timeline {
    let mut periods: BTreeMap<String, Vec<Placed>> = BTreeMap::new();
    let mut undated = 0;
    for entry in &catalog.entries {
        if only.is_some_and(|only| !only.contains(&entry.item.id)) {
            continue;
        }
        let dates = placings(entry, options.by);
        if dates.is_empty() {
            // Undated items are counted when they'd pass every filter but the dates.
            let mut filters = options.filters.clone();
            (filters.after, filters.before) = (None, None);
            let mut found = as_found(entry, Date::MIN);
            found.date = None;
            undated += usize::from(crate::search::passes(&filters, &found));
            continue;
        }
        let mut seen = HashSet::new();
        for (date, what) in dates {
            let period = options.resolution.period(date);
            if !crate::search::passes(&options.filters, &as_found(entry, date))
                || !seen.insert(period.clone())
            {
                continue;
            }
            periods.entry(period).or_default().push(Placed {
                item: entry.item.clone(),
                date: date.to_string(),
                what,
            });
        }
    }
    Timeline {
        resolution: options.resolution,
        by: options.by,
        periods: periods
            .into_iter()
            .map(|(period, mut items)| {
                items.sort_by(|a, b| a.date.cmp(&b.date).then(a.item.card.cmp(&b.item.card)));
                Period { period, items }
            })
            .collect(),
        undated,
    }
}

/// The items `query`'s words find in `archive` by search (its filters are left to the
/// timeline), or none to narrow by when it has no words.
///
/// # Errors
///
/// An error reading the archive for search.
pub fn matching(archive: &Archive, query: &Query) -> Result<Option<HashSet<Id>>, ArchiveError> {
    if query.text.trim().is_empty() {
        return Ok(None);
    }
    let search = Search::build(archive)?;
    let words = Query {
        text: query.text.clone(),
        filters: Filters::default(),
    };
    let results = search.search(&words, search.len().max(1));
    Ok(Some(
        results
            .hits
            .into_iter()
            .filter_map(|hit| match hit.found {
                Found::Item(item) => Some(item.id),
                _ => None,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{date, entry};

    fn catalog() -> Catalog {
        let mut lease = entry(1, "Lease", "Home");
        lease.created = Some(date("2019-04-02"));
        lease.filed = Some(date("2025-01-05"));
        lease.tags = vec!["housing".into()];
        let mut notes = entry(2, "Notes", "Work");
        notes.filed = Some(date("2025-01-20"));
        notes.worked = vec![date("2025-02-01"), date("2025-02-03"), date("2025-03-01")];
        let undated = entry(3, "Undated", "Work");
        Catalog::new(vec![lease, notes, undated], Vec::new())
    }

    fn periods(timeline: &Timeline) -> Vec<(&str, Vec<&str>)> {
        timeline
            .periods
            .iter()
            .map(|p| {
                (
                    p.period.as_str(),
                    p.items.iter().map(|i| i.item.title.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn items_are_placed_by_their_own_date_unless_asked() {
        let t = timeline(&catalog(), &Options::default(), None);
        assert_eq!(
            periods(&t),
            [("2019-04", vec!["Lease"]), ("2025-01", vec!["Notes"])]
        );
        assert_eq!(t.undated, 1);
        assert_eq!(t.periods[0].items[0].what, DateKind::Created);

        let worked = Options {
            by: Some(DateKind::Worked),
            ..Options::default()
        };
        let t = timeline(&catalog(), &worked, None);
        assert_eq!(
            periods(&t),
            [("2025-02", vec!["Notes"]), ("2025-03", vec!["Notes"])],
            "once in each month it was worked on"
        );
        assert_eq!(t.periods[0].items[0].date, "2025-02-01");
        assert_eq!(t.undated, 2);
    }

    #[test]
    fn it_groups_by_year_or_day_and_takes_search_filters() {
        let by_year = Options {
            by: Some(DateKind::Filed),
            resolution: Resolution::Year,
            ..Options::default()
        };
        let t = timeline(&catalog(), &by_year, None);
        assert_eq!(periods(&t), [("2025", vec!["Lease", "Notes"])]);

        let filtered = Options {
            resolution: Resolution::Day,
            filters: Query::parse("tag:housing after:2019").unwrap().filters,
            ..Options::default()
        };
        let t = timeline(&catalog(), &filtered, None);
        assert_eq!(periods(&t), [("2019-04-02", vec!["Lease"])]);
        assert_eq!(t.undated, 0, "the undated item hasn't the tag");

        let before = Options {
            filters: Query::parse("before:2020").unwrap().filters,
            ..Options::default()
        };
        assert_eq!(
            periods(&timeline(&catalog(), &before, None)),
            [("2019-04", vec!["Lease"])]
        );

        let only: HashSet<Id> = [Id(uuid::Uuid::from_u128(2))].into();
        let t = timeline(&catalog(), &Options::default(), Some(&only));
        assert_eq!(periods(&t), [("2025-01", vec!["Notes"])]);
    }
}
