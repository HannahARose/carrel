//! The reminders on the interactive mode's Status (`docs/design/reminders.md`): their lines
//! come with the rest of `carrel status`, near the top, and Enter goes to the first one's item
//! on Active, where its entry is approved or the checkout returned.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::active::Row;
use super::app::{App, Tab};
use super::review::Handled;

impl App {
    /// Reads the reminders due again.
    pub(super) fn load_reminders(&mut self) {
        self.reminders = crate::reminders::due_now(&self.archive);
    }

    /// Whether a reminder is about an item, so Enter on Status has somewhere to go.
    pub(super) fn reminder_has_item(&self) -> bool {
        self.reminders.iter().any(|r| r.item.is_some())
    }

    /// Status's keys: Enter goes to the first reminder's item on Active.
    pub(super) fn status_key(&mut self, key: KeyEvent) -> Handled {
        if key.code != KeyCode::Enter || !self.reminder_has_item() {
            return Handled::No;
        }
        let rows = self.active_rows();
        let found = self.reminders.iter().filter_map(|r| r.item).find_map(|id| {
            rows.iter()
                .position(|row| matches!(row, Row::Item(i) if self.active[*i].id == id))
        });
        match found {
            Some(row) => {
                self.tab = Tab::Active;
                self.active_chosen = row;
            }
            None => self.message = Some("That item isn't checked out or on the desk now.".into()),
        }
        Handled::Yes(None)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    use super::*;
    use crate::tui::tests::{sample, screen};

    #[test]
    fn status_lists_the_reminders_and_enter_goes_to_the_item() {
        let (_dir, mut app) = sample(false);
        let shown = screen(&app, 120, 40);
        assert!(shown.contains("Reminders:"), "{shown}");
        assert!(
            shown.contains("You worked on “oscillator-sim” for 2h 10m."),
            "{shown}"
        );
        assert!(shown.contains("Enter reminder's item"), "{shown}");
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.tab, Tab::Active);
        let Row::Item(i) = app.active_rows()[app.active_chosen] else {
            panic!("an item is chosen");
        };
        assert_eq!(app.active[i].title, "oscillator-sim");
    }
}
