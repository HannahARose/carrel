//! Starting the interactive mode on the screen `carrel tui SCREEN` asks for.

use super::app::{App, Tab};
use super::discover::View;
use crate::start_screen::StartScreen;

/// The tab a screen is on, and for Discover, which view.
fn tab_and_view(screen: StartScreen) -> (Tab, Option<View>) {
    match screen {
        StartScreen::Status => (Tab::Status, None),
        StartScreen::Review => (Tab::Review, None),
        StartScreen::Active => (Tab::Active, None),
        StartScreen::Questions => (Tab::Questions, None),
        StartScreen::Browse => (Tab::Browse, None),
        StartScreen::History => (Tab::History, None),
        StartScreen::Ingest => (Tab::Ingest, None),
        StartScreen::Settings => (Tab::Settings, None),
        StartScreen::Backups => (Tab::Backups, None),
        StartScreen::Librarian => (Tab::Librarian, None),
        StartScreen::Discover => (Tab::Discover, None),
        StartScreen::DiscoverToday => (Tab::Discover, Some(View::Today)),
        StartScreen::DiscoverWander => (Tab::Discover, Some(View::Wander)),
        StartScreen::DiscoverTimeline => (Tab::Discover, Some(View::Timeline)),
        StartScreen::DiscoverMap => (Tab::Discover, Some(View::Map)),
        StartScreen::DiscoverActivity => (Tab::Discover, Some(View::Activity)),
        StartScreen::DiscoverGraph => (Tab::Discover, Some(View::Graph)),
    }
}

impl App {
    /// Shows `screen`, as if the owner had gone to it.
    pub(super) fn start_on_screen(&mut self, screen: StartScreen) {
        let (tab, view) = tab_and_view(screen);
        self.tab = tab;
        if let Some(view) = view {
            self.stacks.view = view;
            self.stacks.lines.clear();
            if view == View::Wander {
                self.wander_step();
            }
        }
        self.settings_shown();
        self.screen_shown();
    }
}

#[cfg(test)]
mod tests {
    use super::super::app::Tab;
    use super::super::discover::View;
    use crate::start_screen::StartScreen;

    #[test]
    fn starts_on_the_asked_tab() {
        let (_dir, mut app) = super::super::tests::sample(false);
        assert_eq!(app.tab, Tab::Status);
        app.start_on_screen(StartScreen::Questions);
        assert_eq!(app.tab, Tab::Questions);
        app.start_on_screen(StartScreen::Backups);
        assert_eq!(app.tab, Tab::Backups);
    }

    #[test]
    fn starts_on_a_discover_view() {
        let (_dir, mut app) = super::super::tests::sample(false);
        app.start_on_screen(StartScreen::DiscoverMap);
        assert_eq!(app.tab, Tab::Discover);
        assert_eq!(app.stacks.view, View::Map);
        assert!(app.stacks.lines.len() > 1, "{:?}", app.stacks.lines);
        app.start_on_screen(StartScreen::DiscoverWander);
        assert_eq!(app.stacks.view, View::Wander);
    }
}
