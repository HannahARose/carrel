//! Activity: when items were filed and worked on, by day, for a heatmap.

use std::collections::BTreeMap;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::Catalog;

/// One day's activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Day {
    /// The day, as `YYYY-MM-DD`.
    #[serde(with = "date_text")]
    pub date: Date,
    /// How many items were filed.
    pub filed: usize,
    /// How many work sessions there were, on any item.
    pub sessions: usize,
}

impl Day {
    /// Everything that happened that day: filings and sessions together.
    #[must_use]
    pub fn total(&self) -> usize {
        self.filed + self.sessions
    }
}

/// Activity by day over a span of days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// The first day of the span.
    #[serde(with = "date_text")]
    pub from: Date,
    /// The last day of the span.
    #[serde(with = "date_text")]
    pub to: Date,
    /// The days with anything, oldest first; the others had nothing.
    pub days: Vec<Day>,
    /// The most on any one day, so a heatmap can scale its shades.
    pub busiest: usize,
}

impl Activity {
    /// The day `date`'s activity: nothing if it's not listed.
    #[must_use]
    pub fn on(&self, date: Date) -> Day {
        self.days
            .iter()
            .find(|d| d.date == date)
            .copied()
            .unwrap_or(Day {
                date,
                ..Day::default()
            })
    }

    /// How busy `day` was compared with the busiest day: 0 for nothing, then 1 to `levels`, so
    /// a heatmap can shade it.
    #[must_use]
    pub fn level(&self, day: &Day, levels: usize) -> usize {
        if day.total() == 0 || self.busiest == 0 {
            return 0;
        }
        (day.total() * levels)
            .div_ceil(self.busiest)
            .clamp(1, levels)
    }
}

/// Filing and work sessions in `catalog` by day, from `from` to `to`, both included.
#[must_use]
pub fn activity(catalog: &Catalog, from: Date, to: Date) -> Activity {
    let mut days: BTreeMap<Date, Day> = BTreeMap::new();
    let in_span = |d: &Date| *d >= from && *d <= to;
    let blank = |date: Date| Day {
        date,
        ..Day::default()
    };
    for entry in &catalog.entries {
        if let Some(filed) = entry.filed.filter(in_span) {
            days.entry(filed).or_insert_with(|| blank(filed)).filed += 1;
        }
        for &(date, n) in entry.sessions.iter().filter(|(d, _)| in_span(d)) {
            days.entry(date).or_insert_with(|| blank(date)).sessions += n;
        }
    }
    let days: Vec<Day> = days.into_values().collect();
    Activity {
        from,
        to,
        busiest: days.iter().map(Day::total).max().unwrap_or(0),
        days,
    }
}

/// The span a heatmap shows by default: the year up to `today`, starting on a Monday so the
/// weeks line up.
#[must_use]
pub fn last_year(today: Date) -> (Date, Date) {
    let start = today
        .checked_sub(jiff::Span::new().days(364))
        .unwrap_or(today);
    let back = i64::from(start.weekday().to_monday_zero_offset());
    (
        start
            .checked_sub(jiff::Span::new().days(back))
            .unwrap_or(start),
        today,
    )
}

/// Dates written as `YYYY-MM-DD`.
mod date_text {
    use jiff::civil::Date;
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's signature
    pub fn serialize<S: Serializer>(date: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(date)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{date, entry};

    #[test]
    fn filings_and_sessions_are_counted_by_day() {
        let mut a = entry(1, "a", "Work");
        a.filed = Some(date("2025-01-05"));
        a.sessions = vec![(date("2025-01-05"), 2), (date("2025-02-01"), 1)];
        let mut b = entry(2, "b", "Work");
        b.filed = Some(date("2025-01-05"));
        let mut old = entry(3, "old", "Work");
        old.filed = Some(date("2020-01-01"));
        let catalog = Catalog::new(vec![a, b, old], Vec::new());

        let activity = activity(&catalog, date("2025-01-01"), date("2025-12-31"));
        assert_eq!(activity.days.len(), 2, "outside the span is left out");
        let busy = activity.on(date("2025-01-05"));
        assert_eq!((busy.filed, busy.sessions, busy.total()), (2, 2, 4));
        assert_eq!(activity.busiest, 4);
        assert_eq!(activity.level(&busy, 4), 4);
        let quiet = activity.on(date("2025-02-01"));
        assert_eq!(activity.level(&quiet, 4), 1);
        assert_eq!(activity.level(&activity.on(date("2025-03-01")), 4), 0);
        let json = serde_json::to_value(&activity).unwrap();
        assert_eq!(json["days"][0]["date"], "2025-01-05");
    }

    #[test]
    fn the_last_year_starts_on_a_monday() {
        let (from, to) = last_year(date("2026-11-01"));
        assert_eq!(to, date("2026-11-01"));
        assert_eq!(from.weekday(), jiff::civil::Weekday::Monday);
        assert!(from <= date("2025-11-02"));
    }
}
