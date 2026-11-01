//! Restructuring's groundwork (`docs/design/restructure.md`, "How a proposal is built"): what
//! restructure proposals (#324) build on, as plain data in and plain data out. Nothing here
//! moves a file.
//!
//! - [`grouping::group_section`]: a section's items clustered by their card embeddings and the
//!   tag and link graph ([`crate::graph`]), into candidate groups with literary warrant, each
//!   with its cohesion and how invasive placing it would be.
//! - [`findability::compare`]: whether a proposed tree is easier to navigate than the current
//!   one, by having a text model find held-out cards by walking each tree from their summaries
//!   alone.
//! - [`Invasiveness`]: the design's ranking of changes, from rewriting to moving top-level
//!   collections.

pub mod findability;
pub mod grouping;

use serde::{Deserialize, Serialize};

/// How much a change disturbs what the owner knows of the archive, from least to most
/// (`docs/design/restructure.md`, "How invasive a change is"). The order is the ranking:
/// a later kind is more invasive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Invasiveness {
    /// Rewriting design documents and tags: nothing moves.
    Rewriting,
    /// Grouping a collection's items into new subcollections inside it: every item stays
    /// under the same collection.
    Subdividing,
    /// Renaming a collection: everything stays together, under a new name.
    Renaming,
    /// Moving items between branches, and merging or splitting collections across them.
    BetweenBranches,
    /// Moving or merging top-level collections: the archive's main shape changes.
    TopLevel,
}

impl Invasiveness {
    /// Every kind, least invasive first.
    pub const ALL: [Self; 5] = [
        Self::Rewriting,
        Self::Subdividing,
        Self::Renaming,
        Self::BetweenBranches,
        Self::TopLevel,
    ];

    /// Its rank in the design's list, from 1 (rewriting) to 5 (moving top-level collections).
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Rewriting => 1,
            Self::Subdividing => 2,
            Self::Renaming => 3,
            Self::BetweenBranches => 4,
            Self::TopLevel => 5,
        }
    }

    /// The kind in words, for a change list.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rewriting => "rewriting design documents and tags",
            Self::Subdividing => "subdividing a collection",
            Self::Renaming => "renaming a collection",
            Self::BetweenBranches => "moving items between branches",
            Self::TopLevel => "moving or merging top-level collections",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_ranked_as_the_design_lists_them() {
        let ranks: Vec<u8> = Invasiveness::ALL.iter().map(|k| k.rank()).collect();
        assert_eq!(ranks, [1, 2, 3, 4, 5]);
        assert!(Invasiveness::Subdividing < Invasiveness::BetweenBranches);
        assert_eq!(
            serde_json::to_value(Invasiveness::BetweenBranches).unwrap(),
            "between-branches"
        );
    }
}
