//! The screen `carrel tui` and `carrel gui` open on, with the same names in both and in the
//! app's `carrel://open?screen=` links.

/// A screen of the interactive mode and the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StartScreen {
    /// What `carrel status` shows.
    Status,
    /// Proposals waiting for review.
    Review,
    /// What's checked out and on the desk.
    Active,
    /// The question queue.
    Questions,
    /// Collections and their items.
    Browse,
    /// Recorded changes.
    History,
    /// The intake queue, and adding to it.
    Ingest,
    /// This device's and the archive's settings.
    Settings,
    /// Backups and their snapshots.
    Backups,
    /// A conversation with the librarian.
    Librarian,
    /// Discovery, on its first view.
    Discover,
    /// Discover's "on this day".
    #[value(name = "discover:today")]
    DiscoverToday,
    /// Discover's walk through the stacks.
    #[value(name = "discover:wander")]
    DiscoverWander,
    /// Discover's timeline.
    #[value(name = "discover:timeline")]
    DiscoverTimeline,
    /// Discover's map of the collections.
    #[value(name = "discover:map")]
    DiscoverMap,
    /// Discover's activity heatmap.
    #[value(name = "discover:activity")]
    DiscoverActivity,
    /// Discover's strongest connections.
    #[value(name = "discover:graph")]
    DiscoverGraph,
}

impl StartScreen {
    /// Its name, as typed and as the app's links carry it.
    pub fn name(self) -> String {
        clap::ValueEnum::to_possible_value(&self)
            .map(|value| value.get_name().to_owned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::StartScreen;
    use clap::ValueEnum as _;

    #[test]
    fn names_are_the_links_names() {
        assert_eq!(StartScreen::Questions.name(), "questions");
        assert_eq!(StartScreen::DiscoverWander.name(), "discover:wander");
        assert_eq!(
            StartScreen::from_str("discover:timeline", false),
            Ok(StartScreen::DiscoverTimeline)
        );
        assert!(StartScreen::from_str("restructure", false).is_err());
    }
}
