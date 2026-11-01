//! Readings of what a proposal's item is, when the drafting model wasn't sure (#449): shown
//! in the inspector, and picked through the correction sheet.

use carrel_core::ingest::readings;
use carrel_core::review::ReviewEntry;

/// One reading of what a proposal's item is.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProposalReading {
    /// Its number, counting from 1, as picking it names it.
    pub number: u32,
    /// The title it gives the card.
    pub title: String,
    /// The summary it gives the card.
    pub summary: String,
    /// The tags it gives the card.
    pub tags: Vec<String>,
    /// Why the model read the item this way.
    pub reason: String,
    /// What about the item doesn't fit it, such as its file name.
    pub against: Vec<String>,
    /// Whether the card shows it now.
    pub on_card: bool,
}

/// `entry`'s readings, empty when the model gave only one.
pub(crate) fn of(entry: &ReviewEntry) -> Vec<ProposalReading> {
    let shown = readings::on_card(entry);
    entry
        .readings
        .iter()
        .enumerate()
        .map(|(i, r)| ProposalReading {
            number: u32::try_from(i + 1).unwrap_or(u32::MAX),
            title: r.title.clone(),
            summary: r.summary.clone(),
            tags: r.tags.iter().map(|t| t.0.clone()).collect(),
            reason: r.reason.clone(),
            against: r.against.clone(),
            on_card: shown == Some(i),
        })
        .collect()
}

/// The words that pick reading `number` of a proposal, for
/// [`crate::ArchiveHandle::prepare_proposal_correction`]: it rewrites the card's title,
/// summary and tags as one correction, without a model.
#[uniffi::export]
#[must_use]
pub fn reading_pick_words(number: u32) -> String {
    readings::pick_words(number as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Sample;
    use carrel_core::ingest::readings::Reading;
    use carrel_core::review::ReviewQueue;

    #[test]
    fn a_proposals_readings_are_shown_and_picking_one_rewrites_its_card() {
        let sample = Sample::new();
        let id = sample.first_proposal();
        let archive = carrel_core::archive::Archive::open(&sample.root()).unwrap();
        let queue = ReviewQueue::of(&archive);
        let mut entry = queue.find(&id).unwrap();
        entry.readings = vec![
            Reading {
                title: entry.card.title.clone(),
                summary: entry.card.summary.clone().unwrap(),
                tags: entry.card.tags.clone(),
                reason: "the file name".into(),
                against: Vec::new(),
            },
            Reading {
                title: "Lampshade print file".into(),
                summary: "A print file for a lampshade.".into(),
                tags: Vec::new(),
                reason: "its dome shape".into(),
                against: vec!["the file name (“lamp-diffuser”)".into()],
            },
        ];
        queue.save(&entry).unwrap();

        let proposal = sample.handle.proposal(id.clone()).unwrap();
        assert!(proposal.readings_doubted);
        assert_eq!(proposal.readings.len(), 2);
        assert!(proposal.readings[0].on_card && !proposal.readings[1].on_card);
        assert_eq!(proposal.readings[1].number, 2);
        assert_eq!(proposal.readings[1].reason, "its dome shape");

        let correction = sample
            .handle
            .prepare_proposal_correction(id.clone(), reading_pick_words(2), None)
            .unwrap();
        let changes = correction.changes();
        assert_eq!(changes.after.title, "Lampshade print file");
        assert!(changes.model.is_none(), "picking needs no model");
        sample.handle.save_correction(correction).unwrap();
        let proposal = sample.handle.proposal(id).unwrap();
        assert_eq!(proposal.fields.title, "Lampshade print file");
        assert!(proposal.readings[1].on_card);
        assert!(!proposal.readings_doubted, "the owner settled it");
    }
}
