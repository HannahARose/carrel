//! What's searched for: words, and filters that narrow the items found
//! (`docs/design/search.md`).
//!
//! Filters are written `name:value`, or `name:"a value with spaces"`: `collection:Finances`,
//! `tag:receipts`, `kind:folder`, `after:2025-01`, `before:2026`, `sensitivity:sensitive`,
//! and `checked-out` on its own. Anything else is words, including a word with a colon that
//! isn't a filter's name, such as `10:30`.

use crate::card::{Kind, Sensitivity};

/// A date or the start of one: a year, a month or a day, as `2025`, `2025-01` or
/// `2025-01-31`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Period(String);

impl Period {
    /// Reads `text` as a year, month or day.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('-').collect();
        let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
        let ok = match parts.as_slice() {
            [y] => digits(y, 4),
            [y, m] => digits(y, 4) && digits(m, 2),
            [y, m, d] => digits(y, 4) && digits(m, 2) && digits(d, 2),
            _ => false,
        };
        ok.then(|| Self(text.to_owned()))
    }

    /// Whether `date` (`YYYY-MM-DD`, perhaps with a time) is in this period or after it.
    #[must_use]
    pub fn starts_by(&self, date: &str) -> bool {
        date.get(..self.0.len())
            .is_some_and(|d| d >= self.0.as_str())
    }

    /// Whether `date` is before this period starts.
    #[must_use]
    pub fn is_after(&self, date: &str) -> bool {
        date.get(..self.0.len())
            .is_some_and(|d| d < self.0.as_str())
    }
}

impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What narrows the items found. Every filter given must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// Collections, by path or name: an item in one of them or below matches.
    pub collections: Vec<String>,
    /// Tags the item must have.
    pub tags: Vec<String>,
    /// Kinds, of which the item must be one.
    pub kinds: Vec<Kind>,
    /// The item's date is in this period or after it.
    pub after: Option<Period>,
    /// The item's date is before this period.
    pub before: Option<Period>,
    /// Sensitivity levels, of which the item's must be one.
    pub sensitivity: Vec<Sensitivity>,
    /// Only items that are checked out.
    pub checked_out: bool,
}

impl Filters {
    /// Whether no filter is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A search: words, and filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The words, as typed, without the filters.
    pub text: String,
    /// The filters.
    pub filters: Filters,
}

/// Why a query can't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError(pub String);

impl std::error::Error for QueryError {}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `text` split into words, keeping a quoted value together with the filter name before it.
fn pieces(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    found.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        found.push(current);
    }
    found
}

fn named<T: serde::de::DeserializeOwned>(what: &str, value: &str) -> Result<T, QueryError> {
    serde_json::from_value(serde_json::Value::String(value.to_lowercase()))
        .map_err(|_| QueryError(format!("{what}:{value} isn't a {what} Carrel knows")))
}

impl Query {
    /// Reads a query as typed.
    ///
    /// # Errors
    ///
    /// [`QueryError`] for a filter whose value can't be used, such as `kind:box` or
    /// `after:March`.
    pub fn parse(text: &str) -> Result<Self, QueryError> {
        let mut words = Vec::new();
        let mut filters = Filters::default();
        let period = |name: &str, value: &str| {
            Period::parse(value).ok_or_else(|| {
                QueryError(format!(
                    "{name}:{value} needs a year, month or day, such as {name}:2025-01"
                ))
            })
        };
        for piece in pieces(text) {
            if piece.eq_ignore_ascii_case("checked-out") {
                filters.checked_out = true;
                continue;
            }
            let Some((name, value)) = piece.split_once(':') else {
                words.push(piece);
                continue;
            };
            let name = name.to_lowercase();
            let known = [
                "collection",
                "tag",
                "kind",
                "after",
                "before",
                "sensitivity",
            ];
            if !known.contains(&name.as_str()) {
                words.push(piece);
                continue;
            }
            if value.is_empty() {
                return Err(QueryError(format!("{name}: needs a value")));
            }
            match name.as_str() {
                "collection" => filters.collections.push(value.to_owned()),
                "tag" => filters.tags.push(value.to_lowercase()),
                "kind" => filters.kinds.push(named("kind", value)?),
                "after" => filters.after = Some(period("after", value)?),
                "before" => filters.before = Some(period("before", value)?),
                _ => filters.sensitivity.push(named("sensitivity", value)?),
            }
        }
        Ok(Self {
            text: words.join(" "),
            filters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_taken_out_of_the_words() {
        let query = Query::parse(
            r#"invoice collection:"Finances/Bills" tag:Receipts kind:file after:2025-01 before:2026 sensitivity:sensitive checked-out 10:30 2057-4038"#,
        )
        .unwrap();
        assert_eq!(query.text, "invoice 10:30 2057-4038");
        let f = query.filters;
        assert_eq!(f.collections, ["Finances/Bills"]);
        assert_eq!(f.tags, ["receipts"]);
        assert_eq!(f.kinds, [Kind::File]);
        assert_eq!(f.after, Period::parse("2025-01"));
        assert_eq!(f.before, Period::parse("2026"));
        assert_eq!(f.sensitivity, [Sensitivity::Sensitive]);
        assert!(f.checked_out);
        assert!(Query::parse("plain words").unwrap().filters.is_empty());
    }

    #[test]
    fn bad_filter_values_are_explained() {
        for bad in [
            "kind:box",
            "after:March",
            "before:2025-1",
            "sensitivity:secret",
            "tag:",
        ] {
            assert!(Query::parse(bad).is_err(), "{bad}");
        }
        assert!(
            Query::parse("kind:box")
                .unwrap_err()
                .to_string()
                .contains("kind:box")
        );
    }

    #[test]
    fn periods_compare_by_their_length() {
        let after = Period::parse("2025-01").unwrap();
        assert!(after.starts_by("2025-01-01"));
        assert!(after.starts_by("2026-03-04T10:00:00Z"));
        assert!(!after.starts_by("2024-12-31"));
        let before = Period::parse("2026").unwrap();
        assert!(before.is_after("2025-12-31"));
        assert!(!before.is_after("2026-01-01"));
        assert!(!before.is_after("20"), "too short to compare");
    }
}
