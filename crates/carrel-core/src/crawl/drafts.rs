//! Outdated drafts (`docs/design/restructure.md`, "Crawlers"): cards drafted by a weaker model
//! than the crawler's, or by an older drafting version ([`crate::ingest::card::DRAFTING`]), are
//! redrafted before the slow sweep, weakest and oldest first. What drafted a card is its
//! `generated.model` and `generated.drafting` (FORMAT.md §5.6). A card no model wrote any
//! field of is the owner's, and never counts.
//!
//! A redraft is the crawler's usual judgment, told the card is an old draft: it corrects the
//! fields a model wrote, through the corrections path, and leaves the owner's alone. Once a
//! card is judged by the crawler's model at the current drafting version, it waits, until a
//! stronger model or a newer drafting version puts it back.

use serde::{Deserialize, Serialize};

use crate::card::Card;
use crate::ingest::card::DRAFTING;

/// What drafted a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drafted {
    /// The model, as `provider/offer`, when the card says.
    pub model: Option<String>,
    /// The drafting version; 0 for an unknown older one.
    pub drafting: u32,
}

impl Drafted {
    /// What drafted `card`, or `None` when no model wrote any of its fields.
    #[must_use]
    pub fn of(card: &Card) -> Option<Self> {
        let generated = card.generated.as_ref()?;
        if generated.fields.is_empty() {
            return None;
        }
        Some(Self {
            model: generated.model.clone(),
            drafting: generated.drafting.unwrap_or(0),
        })
    }
}

/// How strong a model is, for comparing two: the memory the catalog says it needs, then its
/// size, as the benchmark ranks the catalog's models. `None` for a model the catalog doesn't
/// know, such as Apple's, which can't be compared.
#[must_use]
pub fn strength(model: &str) -> Option<(u8, u64)> {
    let offer = model.rsplit_once('/').map_or(model, |(_, offer)| offer);
    crate::models::text_models()
        .find(|m| m.name == offer)
        .map(|m| (m.memory_gb, m.size))
}

/// Why a card's draft is outdated, and where it comes in the stage: weakest model, then oldest
/// drafting version, first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outdated {
    /// The model that would redraft it, as `provider/offer`, when known.
    pub crawler: Option<String>,
    /// Whether the model that drafted it is weaker than the crawler's.
    pub weaker: bool,
    /// The drafting version it was drafted with, older than the current one or not.
    pub drafting: u32,
    /// Its drafting model's strength, `(0, 0)` when unknown.
    pub strength: (u8, u64),
}

impl Outdated {
    /// Its place among outdated drafts: lower first.
    #[must_use]
    pub fn rank(&self) -> ((u8, u64), u32) {
        (self.strength, self.drafting)
    }
}

/// Whether a card `drafted` so is outdated beside the crawler's model `crawler` (when known)
/// and the current drafting version.
#[must_use]
pub fn outdated(drafted: &Drafted, crawler: Option<&str>) -> Option<Outdated> {
    let drafter = drafted.model.as_deref().and_then(strength);
    let weaker = match (drafter, crawler.and_then(strength)) {
        (Some(drafter), Some(crawler)) => drafter < crawler,
        _ => false,
    };
    (weaker || drafted.drafting < DRAFTING).then(|| Outdated {
        crawler: crawler.map(str::to_owned),
        weaker,
        drafting: drafted.drafting,
        strength: drafter.unwrap_or((0, 0)),
    })
}

/// Whether a verdict by `model` at drafting version `drafting` is stale for an outdated card:
/// given by another model than the crawler's now, or before the current drafting version.
#[must_use]
pub fn stale(outdated: &Outdated, model: &str, drafting: Option<u32>) -> bool {
    outdated.crawler.as_deref().is_some_and(|c| c != model) || drafting.unwrap_or(0) < DRAFTING
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drafted(model: Option<&str>, drafting: u32) -> Drafted {
        Drafted {
            model: model.map(str::to_owned),
            drafting,
        }
    }

    #[test]
    fn a_weaker_model_or_an_older_version_makes_a_draft_outdated() {
        let crawler = Some("llama/qwen3.5-9b");
        let weak = outdated(&drafted(Some("llama/qwen3.5-4b"), DRAFTING), crawler).unwrap();
        assert!(weak.weaker);
        assert_eq!(
            outdated(&drafted(Some("llama/qwen3.5-9b"), DRAFTING), crawler),
            None,
            "the crawler's own model, at the current version"
        );
        let old = outdated(&drafted(Some("llama/qwen3.5-9b"), 0), crawler).unwrap();
        assert!(!old.weaker);
        assert!(
            outdated(&drafted(Some("apple/afm"), DRAFTING), crawler).is_none(),
            "a model the catalog doesn't know isn't compared"
        );
        assert!(outdated(&drafted(None, 0), None).is_some());
        // The weakest first, then the oldest.
        let older_weak = outdated(&drafted(Some("llama/qwen3.5-4b"), 0), crawler).unwrap();
        assert!(older_weak.rank() < weak.rank());
        assert!(weak.rank() < old.rank());
    }

    #[test]
    fn a_verdict_by_another_model_or_version_is_stale() {
        let crawler = Some("llama/qwen3.5-9b");
        let old = outdated(&drafted(Some("llama/qwen3.5-4b"), 0), crawler).unwrap();
        assert!(!stale(&old, "llama/qwen3.5-9b", Some(DRAFTING)));
        assert!(
            stale(&old, "llama/qwen3.5-4b", Some(DRAFTING)),
            "a stronger model now"
        );
        assert!(
            stale(&old, "llama/qwen3.5-9b", None),
            "before this drafting version"
        );
    }
}
