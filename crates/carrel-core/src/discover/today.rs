//! On this day: what was filed, created or worked on on a date's month and day in other years.

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::{Catalog, DateKind, Item};

/// One thing that happened on the day: an item, and what was done with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Happened {
    /// The item.
    pub item: Item,
    /// What happened: it was made, filed or worked on that day. An item with more than one
    /// is listed once, with each.
    pub what: Vec<DateKind>,
    /// Its summary, from its card.
    pub summary: Option<String>,
}

/// One earlier year's items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Year {
    /// The year.
    pub year: i16,
    /// How many years before the day asked about.
    pub years_ago: i16,
    /// What happened that day that year, by card path.
    pub items: Vec<Happened>,
}

/// What happened on a date in other years.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnThisDay {
    /// The date asked about, as `YYYY-MM-DD`.
    pub date: String,
    /// The other years that have something, most recent first. The date's own year is left
    /// out.
    pub years: Vec<Year>,
}

/// Whether `day` falls on `date`'s month and day. On 28 February in a year that isn't a leap
/// year, 29 February counts too, so those items still come round.
fn same_day(day: Date, date: Date) -> bool {
    (day.month(), day.day()) == (date.month(), date.day())
        || (date.month() == 2
            && date.day() == 28
            && !date.in_leap_year()
            && (day.month(), day.day()) == (2, 29))
}

/// What was made, filed or worked on on `date`'s month and day in years other than `date`'s.
#[must_use]
pub fn on_this_day(catalog: &Catalog, date: Date) -> OnThisDay {
    let mut years: Vec<Year> = Vec::new();
    for entry in &catalog.entries {
        let mut found: Vec<(i16, DateKind)> = Vec::new();
        for kind in DateKind::ALL {
            for day in entry.dates(kind) {
                if day.year() != date.year() && same_day(day, date) {
                    found.push((day.year(), kind));
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        let mut by_year: Vec<(i16, Vec<DateKind>)> = Vec::new();
        for (year, kind) in found {
            match by_year.iter_mut().find(|(y, _)| *y == year) {
                Some((_, kinds)) => kinds.push(kind),
                None => by_year.push((year, vec![kind])),
            }
        }
        for (year, what) in by_year {
            let happened = Happened {
                item: entry.item.clone(),
                what,
                summary: entry.summary.clone(),
            };
            match years.iter_mut().find(|y| y.year == year) {
                Some(y) => y.items.push(happened),
                None => years.push(Year {
                    year,
                    years_ago: date.year() - year,
                    items: vec![happened],
                }),
            }
        }
    }
    years.sort_by(|a, b| b.year.cmp(&a.year));
    OnThisDay {
        date: date.to_string(),
        years,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{date, entry};

    #[test]
    fn items_from_the_same_day_in_other_years_are_listed_by_year() {
        let mut lease = entry(1, "Lease", "Home");
        lease.created = Some(date("2019-11-01"));
        lease.filed = Some(date("2024-11-01"));
        let mut notes = entry(2, "Notes", "Work");
        notes.worked = vec![date("2024-11-01"), date("2025-06-01")];
        notes.filed = Some(date("2024-11-01"));
        let mut today = entry(3, "Today", "Work");
        today.filed = Some(date("2026-11-01"));
        let mut other = entry(4, "Other", "Work");
        other.created = Some(date("2020-11-02"));
        let catalog = Catalog::new(vec![lease, notes, today, other], Vec::new());

        let day = on_this_day(&catalog, date("2026-11-01"));
        assert_eq!(day.date, "2026-11-01");
        let years: Vec<(i16, i16)> = day.years.iter().map(|y| (y.year, y.years_ago)).collect();
        assert_eq!(years, [(2024, 2), (2019, 7)], "this year's own is left out");
        let in_2024: Vec<(&str, &[DateKind])> = day.years[0]
            .items
            .iter()
            .map(|h| (h.item.title.as_str(), h.what.as_slice()))
            .collect();
        assert_eq!(
            in_2024,
            [
                ("Lease", &[DateKind::Filed][..]),
                ("Notes", &[DateKind::Filed, DateKind::Worked][..]),
            ]
        );
    }

    #[test]
    fn the_29th_of_february_comes_round_on_the_28th() {
        let mut leap = entry(1, "Leap", "Home");
        leap.created = Some(date("2024-02-29"));
        let catalog = Catalog::new(vec![leap], Vec::new());
        assert_eq!(on_this_day(&catalog, date("2025-02-28")).years.len(), 1);
        assert!(on_this_day(&catalog, date("2028-02-28")).years.is_empty());
        assert_eq!(on_this_day(&catalog, date("2028-02-29")).years.len(), 1);
    }
}
