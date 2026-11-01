//! What the chosen question on the Questions screen is about (#446): each item's title and
//! where it is, and the first one's preview where the terminal draws images, as Browse shows
//! it. A restricted item shows no preview, as in Browse.

use carrel_core::questions::subjects::{QuestionSubject, subjects};
use ratatui::text::Line;

use super::app::App;

/// The most items listed under a question; the rest are counted.
const LISTED: usize = 6;

impl App {
    /// Reads what the chosen question is about, and starts making its first item's preview.
    pub(super) fn question_shown(&mut self) {
        self.question_subjects = self
            .asked
            .get(self.asked_chosen)
            .and_then(|q| subjects(&self.archive, q).ok())
            .unwrap_or_default();
        let key = self.question_preview_key();
        let shown = self.question_subjects.iter().find(|s| s.path.is_some());
        let item = shown.and_then(|s| s.path.as_ref()).map(|p| {
            p.strip_prefix(self.archive.root())
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        });
        let restricted = shown.is_some_and(|s| s.restricted);
        self.previews
            .want_file(&self.archive, key.as_deref(), item, restricted);
    }

    /// What the chosen question's preview is drawn by: the question's identifier, while it's
    /// about something with a file.
    pub(super) fn question_preview_key(&self) -> Option<String> {
        let question = self.asked.get(self.asked_chosen)?;
        self.question_subjects
            .iter()
            .any(|s| s.path.is_some())
            .then(|| format!("question:{}", question.id))
    }
}

/// The lines naming what a question is about: each item's title, then where it is, or that
/// it has no file here.
pub(super) fn subject_lines(found: &[QuestionSubject]) -> Vec<Line<'static>> {
    if found.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from("About:")];
    for subject in found.iter().take(LISTED) {
        let mut title = format!("  {}", subject.title);
        if subject.proposal {
            title.push_str(" (waiting)");
        }
        if subject.restricted {
            title.push_str(" (restricted)");
        }
        lines.push(Line::from(title));
        lines.push(Line::from(format!(
            "    {}",
            subject.path.as_deref().map_or_else(
                || "no file on this device".to_owned(),
                |p| p.display().to_string()
            )
        )));
        if found.len() == 1
            && !subject.restricted
            && let Some(summary) = &subject.summary
        {
            lines.push(Line::from(format!("    {summary}")));
        }
    }
    if found.len() > LISTED {
        lines.push(Line::from(format!("  and {} more", found.len() - LISTED)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use carrel_core::card::{Id, Sensitivity};
    use carrel_core::questions::QuestionEntry;
    use carrel_core::review::ReviewQueue;

    use super::super::app::{App, Tab};
    use super::super::preview::{Previews, Protocol};
    use super::super::tests::{sample, screen};

    /// A question about the filed items `about` and the proposals `items`.
    fn question(about: &[Id], items: &[Id]) -> QuestionEntry {
        serde_json::from_value(serde_json::json!({
            "carrel": "question",
            "format": "1.0",
            "id": Id::new_v7(),
            "created": "2026-11-01T10:00:00-07:00",
            "kind": "item",
            "subject": "lamp",
            "question": "What does this show?",
            "about": about,
            "items": items,
        }))
        .unwrap()
    }

    /// Shows `question` on the Questions screen, and waits for its preview.
    fn ask(app: &mut App, question: QuestionEntry) {
        app.tab = Tab::Questions;
        app.asked = vec![question];
        app.asked_settles = vec![1];
        app.asked_chosen = 0;
        app.question_shown();
        let key = app.question_preview_key();
        let started = std::time::Instant::now();
        while app.previews.shown(key.as_deref()).is_none() && started.elapsed().as_secs() < 10 {
            app.previews.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_question_shows_its_items_preview_where_the_terminal_draws_images() {
        let (dir, mut app) = sample(false);
        app.previews = Previews::with(Some(Protocol::Kitty));
        let lamp = app
            .archive
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.card == "Projects/Hardware/lamp-build.md")
            .unwrap();
        ask(&mut app, question(&[lamp.id], &[]));
        let text = screen(&app, 260, 30);
        assert!(text.contains("Preview"), "{text}");
        assert!(text.contains("arm.svg"), "the folder's listing: {text}");
        let path = dir.path().join("archive/Projects/Hardware/lamp-build");
        assert!(text.contains(&path.display().to_string()), "{text}");
        drop(dir);
    }

    #[test]
    fn elsewhere_a_question_names_its_items_path_and_a_restricted_one_shows_no_preview() {
        let (dir, mut app) = sample(false);
        let queue = ReviewQueue::of(&app.archive);
        let (entries, _) = queue.list().unwrap();
        let file = dir.path().join("passport");
        std::fs::create_dir(&file).unwrap();
        std::fs::write(file.join("page.txt"), "scan").unwrap();
        let mut entry = entries[0].clone();
        entry.id = Id::new_v7();
        entry.card.title = "Passport".into();
        entry.card.sensitivity = Sensitivity::Restricted;
        entry.source.path.clone_from(&file);
        queue.save(&entry).unwrap();

        // A terminal that draws images: still nothing for a restricted item.
        app.previews = Previews::with(Some(Protocol::Kitty));
        ask(&mut app, question(&[], &[entry.id]));
        let text = screen(&app, 260, 30);
        assert!(text.contains("Passport (waiting) (restricted)"), "{text}");
        assert!(text.contains(&file.display().to_string()), "{text}");
        assert!(!text.contains("page.txt"), "no preview: {text}");

        // One that doesn't: the path, with no preview.
        entry.card.sensitivity = Sensitivity::Personal;
        queue.save(&entry).unwrap();
        app.previews = Previews::with(None);
        ask(&mut app, question(&[], &[entry.id]));
        let text = screen(&app, 260, 30);
        assert!(text.contains(&file.display().to_string()), "{text}");
        assert!(!text.contains("Preview"), "{text}");
    }
}
