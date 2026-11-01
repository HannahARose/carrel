//! What one line of the decision log holds ([`super`]): identifiers, kinds, extensions,
//! models, counts and timings, never titles, text, names, paths or queries.

use serde::{Deserialize, Serialize};

use crate::card::{Id, Kind};
use crate::questions::QuestionKind;
use crate::review::Confidence;

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// When, in RFC 3339 (UTC).
    pub at: String,
    /// The archive it happened in.
    pub archive: Id,
    /// What happened.
    #[serde(flatten)]
    pub event: Event,
}

impl Record {
    /// When it happened, if `at` can be read.
    #[must_use]
    pub fn time(&self) -> Option<jiff::Timestamp> {
        self.at.parse().ok()
    }
}

/// What the owner decided about a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    /// Approved, to be filed where it was going.
    Approved,
    /// Sent to Holding.
    Held,
    /// Left where it was.
    Rejected,
}

/// How a search was run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchMode {
    /// By words only.
    Words,
    /// By words and meaning together.
    Both,
    /// By filters alone, with no words.
    Filters,
}

impl SearchMode {
    /// Its name, as the log and the report write it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Words => "words",
            Self::Both => "both",
            Self::Filters => "filters",
        }
    }
}

/// How a question ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionOutcome {
    /// Answered.
    Answered,
    /// Answered with "no change", and kept so it isn't asked again.
    Declined,
    /// Skipped for now.
    Skipped,
    /// Dismissed.
    Dismissed,
}

/// Where a proposal was going, as routing chose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routed {
    /// The collection.
    pub collection: Id,
    /// How sure routing was.
    pub confidence: Confidence,
    /// Whether an owner rule chose it, rather than a model.
    #[serde(default)]
    pub rule: bool,
}

/// Something that happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// The worker drafted a proposal.
    Drafted {
        /// The proposal.
        item: Id,
        /// The item's kind.
        kind: Kind,
        /// Its extension, lowercased, as `unreadable.json` keeps it; empty for none.
        extension: String,
        /// The model that drafted its card, as `provider/offer`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Where routing sent it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routed: Option<Routed>,
        /// Seconds it waited in the intake queue before drafting began.
        queued_secs: i64,
        /// Milliseconds drafting it took.
        draft_ms: u64,
    },
    /// The owner moved a proposal to another collection before deciding.
    Moved {
        /// The proposal.
        item: Id,
        /// Where routing had sent it.
        from: Routed,
        /// The collection the owner chose.
        to: Id,
    },
    /// The owner decided a proposal, or an owner rule filed it.
    Decided {
        /// The proposal.
        item: Id,
        /// The item's kind.
        kind: Kind,
        /// Its extension.
        extension: String,
        /// What was decided.
        decision: Decision,
        /// Where it was going when decided.
        to: Routed,
        /// The model that drafted its card.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Seconds from the proposal being drafted to this decision.
        waited_secs: i64,
    },
    /// The owner chose whether a compressed file is expanded or kept.
    Compressed {
        /// The proposal.
        item: Id,
        /// `expand` or `keep`.
        choice: String,
    },
    /// The owner chose how to keep a likely duplicate.
    Duplicate {
        /// The proposal.
        item: Id,
        /// `version-of` or `related`.
        kept: String,
    },
    /// A card was corrected.
    Corrected {
        /// The proposal or filed item.
        item: Id,
        /// The item's kind.
        kind: Kind,
        /// Its extension.
        extension: String,
        /// The fields changed, such as `title`.
        fields: Vec<String>,
        /// Whether it changed several fields or what the item is (broad), not one (narrow).
        broad: bool,
        /// Whether it was a filed item, not a proposal.
        filed: bool,
        /// Whether it moved the item to another collection.
        moved: bool,
        /// The model that wrote the fields corrected.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Who corrected it: `owner`, or a program acting for them such as `librarian`.
        by: String,
    },
    /// A question was answered, skipped or dismissed.
    Question {
        /// The question.
        question: Id,
        /// What it asked about.
        kind: QuestionKind,
        /// How it ended.
        outcome: QuestionOutcome,
        /// How many proposals and items answering it moved or settled.
        #[serde(default)]
        items: usize,
    },
    /// A search ran.
    Searched {
        /// The search, so an opened result can name it.
        search: Id,
        /// How it searched.
        mode: SearchMode,
        /// How many results it found.
        results: usize,
    },
    /// A result of a search was opened.
    Opened {
        /// The search.
        search: Id,
        /// Where the item ranked, from 1.
        rank: usize,
    },
    /// The structure's signals, at most once a day, as Status saw them.
    Snapshot {
        /// Items in the archive.
        items: usize,
        /// Items in Holding.
        holding: usize,
        /// Items filed with low confidence, in collections that show it.
        low: usize,
        /// Items filed with medium confidence, in collections that show it.
        medium: usize,
        /// Collections crowded with loose items.
        crowded: usize,
        /// Collections holding one item.
        single: usize,
        /// Tags used across several top-level collections.
        spread_tags: usize,
        /// Pairs of collections whose items are linked or worked on together.
        #[serde(default)]
        entangled: usize,
        /// Sibling collections at very different levels of detail.
        #[serde(default)]
        uneven: usize,
    },
}
