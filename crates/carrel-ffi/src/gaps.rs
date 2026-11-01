//! The gap log (#455), as the app's Status shows it: what Carrel couldn't do on this device
//! ([`carrel_core::gaps`]), an issue draft for each to open in the browser, where the owner
//! reads, edits and submits it (the app never sends it), the issue's address once reported,
//! and missing features the owner reports in their own words. Like `carrel report gaps`, it
//! holds no titles, text, paths or file names.

use carrel_core::card::Id;
use carrel_core::gaps::Gap;
use carrel_core::gaps::draft::IssueDraft;

use crate::{ArchiveHandle, CarrelError};

/// One gap: every sighting of the same kind, type and stage.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GapRecord {
    /// Its number on this device.
    pub number: u32,
    /// Its kind's name, such as `unread-format`.
    pub kind: String,
    /// It in one line, such as `no reader for the file type: .3mf (model/3mf), at text
    /// extraction`.
    pub summary: String,
    /// The items' media type; empty when there's no item.
    pub media_type: String,
    /// The items' extension, without the dot; empty if none.
    pub extension: String,
    /// The provider or stage that failed.
    pub stage: String,
    /// How many items it has affected.
    pub items: u64,
    /// The day it was first seen, `YYYY-MM-DD`.
    pub first_seen: String,
    /// The day it was last seen.
    pub last_seen: String,
    /// The version of Carrel that last saw it.
    pub last_version: String,
    /// The owner's own words, for a missing feature they reported.
    pub words: Option<String>,
    /// The address of the issue it was reported in.
    pub reported: Option<String>,
    /// Whether this Carrel is newer than the one that last saw it, so it may be addressed.
    pub not_seen_since_update: bool,
}

impl From<&Gap> for GapRecord {
    fn from(gap: &Gap) -> Self {
        Self {
            number: gap.number,
            kind: gap.kind.name().to_owned(),
            summary: gap.summary(),
            media_type: gap.media_type.clone(),
            extension: gap.extension.clone(),
            stage: gap.stage.clone(),
            items: gap.items,
            first_seen: gap.first_seen.clone(),
            last_seen: gap.last_seen.clone(),
            last_version: gap.last_version.clone(),
            words: gap.words.clone(),
            reported: gap.reported.clone(),
            not_seen_since_update: gap.not_seen_since_update(carrel_core::CARREL_VERSION),
        }
    }
}

/// An issue draft for a gap, for the owner to read before it opens in the browser.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GapIssueDraft {
    /// The issue's title.
    pub title: String,
    /// Its body, in Markdown.
    pub body: String,
    /// The prefilled new-issue address on GitHub, which the app opens.
    pub url: String,
}

#[uniffi::export]
impl ArchiveHandle {
    /// This device's gaps, most items first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if there's no configuration folder.
    pub fn gaps(&self) -> Result<Vec<GapRecord>, CarrelError> {
        let log = carrel_core::gaps::list_in(&self.config_dir()?);
        Ok(log.gaps.iter().map(GapRecord::from).collect())
    }

    /// The issue draft for gap `number`, with the owner's `added` words if any. Nothing is
    /// sent: the app opens its address for the owner to submit.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such gap.
    pub fn gap_issue_draft(
        &self,
        number: u32,
        added: Option<String>,
    ) -> Result<GapIssueDraft, CarrelError> {
        let log = carrel_core::gaps::list_in(&self.config_dir()?);
        let gap = log
            .get(number)
            .ok_or_else(|| CarrelError::invalid(format!("there's no gap {number}")))?;
        let draft = IssueDraft::of(
            gap,
            carrel_core::CARREL_VERSION,
            std::env::consts::OS,
            added.as_deref(),
        );
        Ok(GapIssueDraft {
            url: draft.url(),
            title: draft.title,
            body: draft.body,
        })
    }

    /// Records that gap `number` was reported at `address`, or clears it when empty.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such gap or the address isn't one.
    pub fn mark_gap_reported(
        &self,
        number: u32,
        address: String,
    ) -> Result<GapRecord, CarrelError> {
        carrel_core::gaps::mark_reported_in(&self.config_dir()?, number, &address)
            .map(|g| GapRecord::from(&g))
            .map_err(CarrelError::invalid)
    }

    /// Records a missing feature the owner reports in their own `words`, about the item `id`
    /// when given: its type is recorded, never its name.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if `words` are empty or there's no such item.
    pub fn report_missing_feature(
        &self,
        id: Option<String>,
        words: String,
    ) -> Result<GapRecord, CarrelError> {
        let archive = self.fresh()?;
        let items = if id.is_some() {
            archive.items()?
        } else {
            Vec::new()
        };
        let item = match &id {
            Some(id) => {
                let wanted = id
                    .parse()
                    .map(Id)
                    .map_err(|_| CarrelError::invalid(format!("“{id}” isn't an identifier")))?;
                Some(items.iter().find(|i| i.id == wanted).ok_or_else(|| {
                    CarrelError::invalid(format!("no item has the identifier {id}"))
                })?)
            }
            None => None,
        };
        carrel_core::gaps::report_missing_feature_in(
            &self.config_dir()?,
            Some(&archive),
            item,
            &words,
        )
        .map(|g| GapRecord::from(&g))
        .map_err(CarrelError::invalid)
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn gaps_are_listed_drafted_and_marked_reported_without_names() {
        let sample = Sample::new();
        let handle = &sample.handle;
        assert_eq!(handle.gaps().unwrap().len(), 0);
        let items = handle.items().unwrap();
        let pdf = items
            .iter()
            .find(|i| i.item.as_deref().is_some_and(|p| p.ends_with("note.pdf")))
            .expect("a PDF in the sample");
        let gap = handle
            .report_missing_feature(Some(pdf.id.clone()), "Read its equations".into())
            .unwrap();
        assert_eq!(
            (gap.kind.as_str(), gap.extension.as_str()),
            ("owner-reported", "pdf")
        );
        assert!(handle.report_missing_feature(None, " ".into()).is_err());

        let draft = handle
            .gap_issue_draft(gap.number, Some("Like LaTeX".into()))
            .unwrap();
        assert!(
            draft
                .url
                .starts_with("https://github.com/Horizon-Labs/carrel/issues/new?")
        );
        assert!(draft.body.contains("Read its equations"));
        assert!(draft.body.contains("Like LaTeX"));
        assert!(!draft.body.contains("oscillators"), "{}", draft.body);

        let reported = handle
            .mark_gap_reported(
                gap.number,
                "https://github.com/Horizon-Labs/carrel/issues/9".into(),
            )
            .unwrap();
        assert!(reported.reported.is_some());
        assert!(handle.mark_gap_reported(gap.number, "soon".into()).is_err());
        assert_eq!(handle.gaps().unwrap().len(), 1);
    }
}
