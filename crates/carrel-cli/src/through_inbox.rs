//! Acting through this device's inbox (`docs/design/sync-and-mobile.md`, "Inboxes"): when
//! another device is the writer, or the archive is synced on a system where Carrel can't take
//! its writer lock, a command sends what it would do as a request, for the writer to apply as
//! this person's and device's, instead of doing it. [`check`] says whether that's so, and
//! [`ThroughInbox`] sends and words it.

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::coordinator::Holder;
use carrel_core::inbox::{self, Request, Sending};
use serde_json::{Value, json};

use crate::config::Identity;
use crate::{Failure, Report};

/// Why this device acts through its inbox, and as whom.
#[derive(Debug, Clone)]
pub(crate) struct ThroughInbox {
    /// Why.
    pub sending: Sending,
    /// The person and device the writer records it as.
    pub holder: Holder,
}

/// `Some` when changes made here go into this device's inbox rather than being made now.
///
/// # Errors
///
/// [`Failure`] if this device's identity can't be read or made.
pub(crate) fn check(archive: &Archive) -> Result<Option<ThroughInbox>, Failure> {
    let identity = Identity::load_or_create()?;
    Ok(
        inbox::sending(archive, identity.device).map(|sending| ThroughInbox {
            sending,
            holder: identity.holder(),
        }),
    )
}

impl ThroughInbox {
    /// Puts `request` in this device's inbox, and says so.
    ///
    /// # Errors
    ///
    /// [`Failure`] if the entry can't be written.
    pub fn send(&self, archive: &Archive, request: Request) -> Result<Report, Failure> {
        let what = request.describe();
        let sent = inbox::send(archive, self.holder, request)?;
        Ok(self.report(&sent, &what))
    }

    /// Says that `what` went into this device's inbox, as the entry `sent`.
    pub fn report(&self, sent: &Path, what: &str) -> Report {
        Report::new(
            format!("{}\nSent: {what}.\n", self.sending.describe()),
            self.json(Some(sent)),
        )
    }

    /// What `--json` says about it: the entry, the writer, and that it isn't applied yet.
    pub fn json(&self, sent: Option<&Path>) -> Value {
        json!({
            "inbox": sent,
            "writer": self.sending.writer(),
            "applied": false,
        })
    }
}

/// Whether `report` says its change went into the inbox rather than being made.
pub(crate) fn was_sent(report: &Report) -> bool {
    report.json.get("applied") == Some(&json!(false))
}
