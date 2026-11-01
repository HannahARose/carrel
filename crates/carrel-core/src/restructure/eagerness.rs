//! How eager Carrel is to rearrange (`docs/design/restructure.md`, "How eager to rearrange"):
//! the `restructure.eagerness` setting, archive-wide in `settings.json` and per collection in
//! its `_collection.md`, and the numbers its three presets stand for.
//!
//! A preset sets three numbers, each a setting of its own for anyone who wants to tune it:
//!
//! - **the margin:** how much a change's benefit must beat its cost by, before it's scaled by
//!   how invasive the change is ([`Thresholds::required_gain`]), so subdividing a collection
//!   needs less than moving items between branches;
//! - **the stability penalty:** the cost added to a change in a section restructured
//!   recently, and for how many days after;
//! - **the move budget:** how many moves between branches and top-level collections one
//!   proposal may make; subdividing, renaming and rewriting are never counted.
//!
//! Eagerness changes how readily Carrel proposes, never whether it asks, and never overrides
//! the findability test.

use serde::{Deserialize, Serialize};

use super::Invasiveness;
use crate::archive::{Archive, Collection};
use crate::card::Extra;

/// How eager Carrel is to rearrange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Eagerness {
    /// The owner knows where things are: only a large, clear gain moves anything between
    /// branches, though subdividing and renaming still need only a modest one.
    Settled,
    /// A clear gain, scaled by how invasive the change is.
    #[default]
    Balanced,
    /// Any real gain, including splitting and merging freely.
    Eager,
}

impl Eagerness {
    /// Every level, least eager first.
    pub const ALL: [Self; 3] = [Self::Settled, Self::Balanced, Self::Eager];

    /// Its name, as settings and design documents write it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::Balanced => "balanced",
            Self::Eager => "eager",
        }
    }

    /// The level named `name`, ignoring case.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|e| e.as_str().eq_ignore_ascii_case(name.trim()))
    }

    /// The numbers the preset stands for.
    #[must_use]
    pub fn preset(self) -> Thresholds {
        match self {
            Self::Settled => Thresholds {
                margin: 0.3,
                stability_penalty: 0.5,
                stability_days: 365,
                move_budget: Some(10),
            },
            Self::Balanced => Thresholds {
                margin: 0.15,
                stability_penalty: 0.2,
                stability_days: 90,
                move_budget: None,
            },
            Self::Eager => Thresholds {
                margin: 0.0,
                stability_penalty: 0.05,
                stability_days: 30,
                move_budget: None,
            },
        }
    }
}

impl std::fmt::Display for Eagerness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The numbers behind an eagerness level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// How much a change's benefit must beat its cost by, for moves between branches; other
    /// kinds of change scale it ([`Thresholds::required_gain`]). 0 means any real gain.
    pub margin: f64,
    /// The cost added to a change in a section restructured within `stability_days`.
    pub stability_penalty: f64,
    /// How many days after a restructure its section counts as recently changed.
    pub stability_days: u32,
    /// How many items one proposal may move between branches or top-level collections, or
    /// `None` for as many as the gain justifies.
    pub move_budget: Option<usize>,
}

impl Thresholds {
    /// How much a change of this kind must gain: the margin, scaled by how invasive it is.
    /// Rewriting needs a quarter of the margin, subdividing half, renaming 0.6 of it, moving
    /// items between branches all of it, and moving top-level collections half as much again.
    #[must_use]
    pub fn required_gain(&self, invasiveness: Invasiveness) -> f64 {
        self.margin * scale(invasiveness)
    }

    /// Whether a change of this kind counts against the move budget.
    #[must_use]
    pub fn counts_against_budget(invasiveness: Invasiveness) -> bool {
        invasiveness >= Invasiveness::BetweenBranches
    }
}

/// How much of the margin a change of this kind needs.
#[must_use]
pub fn scale(invasiveness: Invasiveness) -> f64 {
    match invasiveness {
        Invasiveness::Rewriting => 0.25,
        Invasiveness::Subdividing => 0.5,
        Invasiveness::Renaming => 0.6,
        Invasiveness::BetweenBranches => 1.0,
        Invasiveness::TopLevel => 1.5,
    }
}

/// The `restructure` settings in `settings.json` (FORMAT.md §7.2): the level, and any of its
/// numbers set directly, which take precedence over the level's preset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RestructureSettings {
    /// How eager Carrel is to rearrange, archive-wide.
    #[serde(default)]
    pub eagerness: Eagerness,
    /// The margin, if set directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<f64>,
    /// The stability penalty, if set directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability_penalty: Option<f64>,
    /// How long the stability penalty lasts, in days, if set directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability_days: Option<u32>,
    /// The move budget, if set directly: a number, or `none` for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_budget: Option<MoveBudget>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A move budget set directly: a number of moves, or no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MoveBudget {
    /// At most this many.
    Moves(usize),
    /// `"none"`: as many as the gain justifies.
    Unlimited(Unlimited),
}

/// The word `none`, for a move budget with no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unlimited {
    /// No limit.
    None,
}

impl MoveBudget {
    /// The limit, or `None` for no limit.
    #[must_use]
    pub fn limit(self) -> Option<usize> {
        match self {
            Self::Moves(n) => Some(n),
            Self::Unlimited(_) => None,
        }
    }

    /// The budget written as a value: a number, or `none`.
    #[must_use]
    pub fn describe(limit: Option<usize>) -> String {
        limit.map_or_else(|| "none".to_owned(), |n| n.to_string())
    }

    /// Reads `none` or a whole number.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("none") {
            return Some(Self::Unlimited(Unlimited::None));
        }
        value.parse().ok().map(Self::Moves)
    }
}

impl RestructureSettings {
    /// Whether nothing is set, so `settings.json` leaves the section out.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// The numbers for `eagerness`, with any set directly in place of its preset's.
    #[must_use]
    pub fn thresholds(&self, eagerness: Eagerness) -> Thresholds {
        let preset = eagerness.preset();
        Thresholds {
            margin: self.margin.unwrap_or(preset.margin),
            stability_penalty: self.stability_penalty.unwrap_or(preset.stability_penalty),
            stability_days: self.stability_days.unwrap_or(preset.stability_days),
            move_budget: self
                .move_budget
                .map_or(preset.move_budget, MoveBudget::limit),
        }
    }
}

/// A collection's own eagerness, from `restructure: {eagerness: …}` in its design document
/// (FORMAT.md §6.3), if it sets one it knows.
#[must_use]
pub fn of_collection(collection: &Collection) -> Option<Eagerness> {
    collection
        .doc
        .extra
        .get("restructure")?
        .get("eagerness")?
        .as_str()
        .and_then(Eagerness::parse)
}

/// Where an eagerness level comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "from", content = "collection")]
pub enum Source {
    /// The archive's `restructure.eagerness`.
    Archive,
    /// A collection's design document: this one, or the one it's inside that sets it.
    Collection(String),
}

/// The eagerness that applies at `path` (a collection's path, or empty for the archive), and
/// where it comes from: the nearest collection at or above it whose design document sets one,
/// else the archive's.
#[must_use]
pub fn at(archive: &Archive, collections: &[Collection], path: &str) -> (Eagerness, Source) {
    let mut here = path.trim_matches('/');
    while !here.is_empty() {
        if let Some(level) = collections
            .iter()
            .find(|c| c.path == here)
            .and_then(of_collection)
        {
            return (level, Source::Collection(here.to_owned()));
        }
        here = here.rsplit_once('/').map_or("", |(parent, _)| parent);
    }
    (archive.settings().restructure.eagerness, Source::Archive)
}

/// The numbers that apply at `path`, as [`at`] finds its level.
#[must_use]
pub fn thresholds_at(archive: &Archive, collections: &[Collection], path: &str) -> Thresholds {
    archive
        .settings()
        .restructure
        .thresholds(at(archive, collections, path).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_ask_less_of_less_invasive_changes() {
        for level in Eagerness::ALL {
            let t = level.preset();
            let gains: Vec<f64> = Invasiveness::ALL
                .iter()
                .map(|k| t.required_gain(*k))
                .collect();
            assert!(gains.windows(2).all(|w| w[0] <= w[1]), "{level}: {gains:?}");
        }
        let settled = Eagerness::Settled.preset();
        let balanced = Eagerness::Balanced.preset();
        let eager = Eagerness::Eager.preset();
        // Each level asks more than the next, for every kind but rewriting at Eager.
        assert!(
            settled.required_gain(Invasiveness::BetweenBranches)
                > balanced.required_gain(Invasiveness::BetweenBranches)
        );
        assert!(eager.required_gain(Invasiveness::TopLevel).abs() < f64::EPSILON);
        // Even Settled subdivides for a modest gain, less than Balanced asks to move.
        assert!(
            settled.required_gain(Invasiveness::Subdividing)
                <= balanced.required_gain(Invasiveness::BetweenBranches)
        );
        assert!(settled.stability_penalty > balanced.stability_penalty);
        assert!(balanced.stability_penalty > eager.stability_penalty);
        assert!(settled.stability_days > balanced.stability_days);
        assert_eq!(settled.move_budget, Some(10));
        assert_eq!(eager.move_budget, None);
    }

    #[test]
    fn numbers_set_directly_take_precedence() {
        let settings: RestructureSettings = serde_json::from_value(serde_json::json!({
            "eagerness": "settled",
            "margin": 0.05,
            "move_budget": "none",
        }))
        .unwrap();
        let t = settings.thresholds(settings.eagerness);
        assert!((t.margin - 0.05).abs() < f64::EPSILON);
        assert_eq!(t.move_budget, None);
        assert_eq!(t.stability_days, 365);
        let back = serde_json::to_value(&settings).unwrap();
        assert_eq!(back["move_budget"], "none");
        assert!(RestructureSettings::default().is_default());
        assert_eq!(MoveBudget::parse("12"), Some(MoveBudget::Moves(12)));
        assert_eq!(MoveBudget::parse("lots"), None);
        assert_eq!(Eagerness::parse(" Eager "), Some(Eagerness::Eager));
    }
}
