//! A card's history entries (FORMAT.md §5.5): the dated work sessions in its `history`
//! managed section, newest first. Programs write new entries as pending callouts; the owner
//! approves, edits, combines or drops them (`docs/design/checkout.md`).
//!
//! ```markdown
//! ### 2026-10-29 · 2h 10m
//! Added a phase-response analysis and regenerated figure 4.
//! My notes: compare with the 2024 notebook.
//!
//! > [!pending] 2026-10-30 · 45m · auto-generated
//! > Changed 3 files.
//! ```
//!
//! Parsing keeps every line it doesn't recognize, so writing the section back changes only the
//! entries an operation touched.

use std::fmt::Write as _;

/// The marker that starts a pending entry.
const PENDING: &str = "> [!pending]";

/// What a program adds after an entry's heading to say it wrote the entry.
pub const AUTO: &str = "auto-generated";

/// The prefix of the line in an entry that's the owner's own.
const NOTES: &str = "My notes:";

/// One history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// When, as `YYYY-MM-DD`.
    pub date: String,
    /// How long, such as `2h 10m`, if known.
    pub duration: Option<String>,
    /// Whether it's waiting for the owner's approval.
    pub pending: bool,
    /// Anything else on the heading line, such as `auto-generated`.
    pub tags: Vec<String>,
    /// The entry's text, one string per line, without the callout's `> `.
    pub lines: Vec<String>,
    /// The lines it was read from, exactly, so an entry nothing changed is written back as
    /// the owner left it. Empty for a new entry.
    source: Vec<String>,
}

/// Entries are the same when they say the same, however they were written.
impl PartialEq for HistoryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.date == other.date
            && self.duration == other.duration
            && self.pending == other.pending
            && self.tags == other.tags
            && self.lines == other.lines
    }
}

impl Eq for HistoryEntry {}

impl HistoryEntry {
    /// A new pending entry written by a program.
    #[must_use]
    pub fn pending(date: &str, duration: Option<u64>, lines: Vec<String>) -> Self {
        Self {
            date: date.to_owned(),
            duration: duration.map(format_minutes),
            pending: true,
            tags: vec![AUTO.to_owned()],
            lines,
            source: Vec::new(),
        }
    }

    /// The minutes in `duration`, if it reads as one.
    #[must_use]
    pub fn minutes(&self) -> Option<u64> {
        self.duration.as_deref().and_then(parse_minutes)
    }

    fn heading(&self) -> String {
        let mut parts = vec![self.date.clone()];
        parts.extend(self.duration.clone());
        if self.pending {
            parts.extend(self.tags.iter().cloned());
        } else {
            // An approved entry is the owner's: it no longer says a program wrote it.
            parts.extend(self.tags.iter().filter(|t| *t != AUTO).cloned());
        }
        parts.join(" · ")
    }

    fn render(&self, out: &mut String) {
        if !self.source.is_empty()
            && HistoryLog::parse(&self.source.join("\n")).entries.first() == Some(self)
        {
            for line in &self.source {
                writeln!(out, "{line}").unwrap();
            }
            return;
        }
        if self.pending {
            writeln!(out, "{PENDING} {}", self.heading()).unwrap();
            for line in &self.lines {
                if line.is_empty() {
                    out.push_str(">\n");
                } else {
                    writeln!(out, "> {line}").unwrap();
                }
            }
        } else {
            writeln!(out, "### {}", self.heading()).unwrap();
            for line in &self.lines {
                writeln!(out, "{line}").unwrap();
            }
        }
    }
}

/// A history section: whatever comes before the first entry (usually `## History`), then the
/// entries, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryLog {
    /// The lines before the first entry.
    pub preamble: Vec<String>,
    /// The entries, in the order they're written.
    pub entries: Vec<HistoryEntry>,
}

/// Why an operation on entries couldn't be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryError {
    /// There's no entry at this position.
    NoEntry(usize),
    /// The entry is already approved, so it can't be approved again or dropped.
    NotPending(usize),
    /// Combining needs at least two entries.
    TooFew,
    /// Edited text with nothing in it: dropping an entry is its own operation.
    Empty,
    /// Edited text that would read as more than one entry, or break the history section.
    Unreadable(String),
}

impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntry(i) => write!(f, "there's no history entry {}", i + 1),
            Self::NotPending(i) => write!(f, "history entry {} is already approved", i + 1),
            Self::TooFew => f.write_str("combining needs at least two pending entries"),
            Self::Empty => f.write_str("the entry has no text left; drop it instead"),
            Self::Unreadable(line) => write!(
                f,
                "the line “{line}” would break the history section; change or remove it"
            ),
        }
    }
}

impl std::error::Error for EntryError {}

impl HistoryLog {
    /// Reads a history section's content.
    #[must_use]
    pub fn parse(content: &str) -> Self {
        let mut log = Self::default();
        let mut current: Option<HistoryEntry> = None;
        // Whether the current entry is a callout, whose lines start with `>`.
        let mut callout = false;
        for line in content.lines() {
            let started = if let Some(heading) = line.strip_prefix(PENDING) {
                Some((entry_from(heading.trim(), true), true))
            } else if let Some(heading) = line.strip_prefix("### ") {
                Some((entry_from(heading.trim(), false), false))
            } else {
                approved_callout(line).map(|heading| (entry_from(heading, false), true))
            };
            if let Some((mut entry, is_callout)) = started {
                log.entries.extend(current.take());
                entry.source.push(line.to_owned());
                current = Some(entry);
                callout = is_callout;
            } else if let Some(entry) = current.as_mut() {
                if callout {
                    if let Some(rest) = line.strip_prefix('>') {
                        entry
                            .lines
                            .push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
                    } else if line.trim().is_empty() {
                        // A blank line ends a callout.
                        log.entries.extend(current.take());
                        continue;
                    } else {
                        // Text after a callout that isn't part of it: keep it with the entry.
                        entry.lines.push(line.to_owned());
                    }
                } else {
                    entry.lines.push(line.to_owned());
                }
                entry.source.push(line.to_owned());
            } else {
                log.preamble.push(line.to_owned());
            }
        }
        log.entries.extend(current);
        for entry in &mut log.entries {
            while entry.lines.last().is_some_and(|l| l.trim().is_empty()) {
                entry.lines.pop();
            }
            while entry.source.last().is_some_and(|l| l.trim().is_empty()) {
                entry.source.pop();
            }
        }
        while log.preamble.last().is_some_and(|l| l.trim().is_empty()) {
            log.preamble.pop();
        }
        log
    }

    /// The section's content, entries separated by blank lines.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.preamble {
            writeln!(out, "{line}").unwrap();
        }
        for entry in &self.entries {
            if !out.is_empty() {
                out.push('\n');
            }
            entry.render(&mut out);
        }
        out
    }

    /// The positions of the pending entries.
    #[must_use]
    pub fn pending(&self) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&i| self.entries[i].pending)
            .collect()
    }

    /// Adds `entry` where it belongs by date: entries are newest first.
    pub fn add(&mut self, entry: HistoryEntry) {
        let at = self
            .entries
            .iter()
            .position(|e| e.date < entry.date)
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
    }

    fn pending_at(&self, i: usize) -> Result<&HistoryEntry, EntryError> {
        let entry = self.entries.get(i).ok_or(EntryError::NoEntry(i))?;
        if entry.pending {
            Ok(entry)
        } else {
            Err(EntryError::NotPending(i))
        }
    }

    /// Approves the pending entry at `i`, keeping its text.
    ///
    /// # Errors
    ///
    /// [`EntryError`] if there's no pending entry there.
    pub fn approve(&mut self, i: usize) -> Result<(), EntryError> {
        self.pending_at(i)?;
        self.entries[i].pending = false;
        Ok(())
    }

    /// Drops the pending entry at `i`.
    ///
    /// # Errors
    ///
    /// [`EntryError`] if there's no pending entry there. Approved entries are the owner's to
    /// delete by hand.
    pub fn drop_entry(&mut self, i: usize) -> Result<HistoryEntry, EntryError> {
        self.pending_at(i)?;
        Ok(self.entries.remove(i))
    }

    /// Replaces the text of the pending entry at `i` with `text`, one line per line, keeping
    /// it pending. Trailing blank lines are left out. Returns whether anything changed.
    ///
    /// # Errors
    ///
    /// [`EntryError`] if there's no pending entry there, `text` is empty, or a line of it
    /// would read as a new entry or a section marker, whether pending or once approved.
    pub fn edit(&mut self, i: usize, text: &str) -> Result<bool, EntryError> {
        let entry = self.pending_at(i)?;
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if lines.iter().all(|l| l.trim().is_empty()) {
            return Err(EntryError::Empty);
        }
        if lines == entry.lines {
            return Ok(false);
        }
        let edited = HistoryEntry {
            lines,
            source: Vec::new(),
            ..entry.clone()
        };
        check_readable(&edited)?;
        self.entries[i] = edited;
        Ok(true)
    }

    /// Combines the pending entries at `positions` into one, still pending, where the newest
    /// was: its date, the durations added up, and every line of text, the owner's notes last.
    ///
    /// # Errors
    ///
    /// [`EntryError`] if fewer than two are given or any isn't pending.
    pub fn combine(&mut self, positions: &[usize]) -> Result<(), EntryError> {
        let mut positions = positions.to_vec();
        positions.sort_unstable();
        positions.dedup();
        if positions.len() < 2 {
            return Err(EntryError::TooFew);
        }
        for &i in &positions {
            self.pending_at(i)?;
        }
        let parts: Vec<&HistoryEntry> = positions.iter().map(|&i| &self.entries[i]).collect();
        let date = parts
            .iter()
            .map(|e| e.date.clone())
            .max()
            .unwrap_or_default();
        let minutes: Option<u64> = parts.iter().map(|e| e.minutes()).sum();
        let mut lines = Vec::new();
        let mut notes = Vec::new();
        for entry in &parts {
            for line in &entry.lines {
                if let Some(note) = line.strip_prefix(NOTES) {
                    notes.push(note.trim().to_owned());
                } else if !line.trim().is_empty() && !lines.contains(line) {
                    lines.push(line.clone());
                }
            }
        }
        if !notes.is_empty() {
            lines.push(format!("{NOTES} {}", notes.join(" ")));
        }
        let mut tags: Vec<String> = Vec::new();
        for entry in &parts {
            for tag in &entry.tags {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
        let combined = HistoryEntry {
            date,
            duration: minutes.map(format_minutes),
            pending: true,
            tags,
            lines,
            source: Vec::new(),
        };
        let first = positions[0];
        for &i in positions.iter().rev() {
            self.entries.remove(i);
        }
        self.entries.insert(first, combined);
        Ok(())
    }
}

/// Checks that `entry` reads back as itself, both pending and once approved, and keeps the
/// section's markers where they are.
fn check_readable(entry: &HistoryEntry) -> Result<(), EntryError> {
    for pending in [true, false] {
        let entry = HistoryEntry {
            pending,
            ..entry.clone()
        };
        let mut text = String::new();
        entry.render(&mut text);
        let back = HistoryLog::parse(&text);
        if back.entries.len() == 1
            && back.entries[0].lines == entry.lines
            && crate::sections::safe_inside(&text)
        {
            continue;
        }
        let bad = entry
            .lines
            .iter()
            .find(|line| {
                line.starts_with("### ")
                    || line.starts_with(PENDING)
                    || !crate::sections::safe_inside(line)
            })
            .or_else(|| entry.lines.first())
            .cloned()
            .unwrap_or_default();
        return Err(EntryError::Unreadable(bad));
    }
    Ok(())
}

/// The heading of a callout of another type than `pending` that starts with a date, such as
/// `> [!done] 2026-10-29 · 2h 10m`: an entry the owner approved by changing its type.
fn approved_callout(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("> [!")?;
    let (kind, heading) = rest.split_once(']')?;
    if kind.eq_ignore_ascii_case("pending") {
        return None;
    }
    let heading = heading.trim_start_matches(['+', '-']).trim();
    let date = heading.get(..10)?.as_bytes();
    let is_date = date.iter().enumerate().all(|(i, b)| {
        if i == 4 || i == 7 {
            *b == b'-'
        } else {
            b.is_ascii_digit()
        }
    });
    is_date.then_some(heading)
}

/// An entry with nothing but its heading, `date · duration · tags`.
fn entry_from(heading: &str, pending: bool) -> HistoryEntry {
    let mut parts = heading.split(" · ").map(str::trim);
    let date = parts.next().unwrap_or_default().to_owned();
    let mut duration = None;
    let mut tags = Vec::new();
    for part in parts {
        if duration.is_none() && tags.is_empty() && parse_minutes(part).is_some() {
            duration = Some(part.to_owned());
        } else {
            tags.push(part.to_owned());
        }
    }
    HistoryEntry {
        date,
        duration,
        pending,
        tags,
        lines: Vec::new(),
        source: Vec::new(),
    }
}

/// `2h 10m`, `45m` or `3h` as minutes.
#[must_use]
pub fn parse_minutes(text: &str) -> Option<u64> {
    let mut total = 0;
    let mut any = false;
    for part in text.split_whitespace() {
        if let Some(h) = part.strip_suffix('h') {
            total += h.parse::<u64>().ok()? * 60;
        } else {
            total += part.strip_suffix('m')?.parse::<u64>().ok()?;
        }
        any = true;
    }
    any.then_some(total)
}

/// Minutes as `2h 10m`, `45m` or `3h`.
#[must_use]
pub fn format_minutes(minutes: u64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTION: &str = "## History\n\n\
        > [!pending] 2026-10-30 · 45m · auto-generated\n\
        > Changed 3 files.\n\
        > Commits: \"fix units\"\n\
        \n\
        ### 2026-10-29 · 2h 10m\n\
        Added a phase-response analysis.\n\
        My notes: compare with the 2024 notebook.\n\
        \n\
        ### 2026-10-01\n\
        Started.\n";

    #[test]
    fn a_section_reads_and_writes_back_unchanged() {
        let log = HistoryLog::parse(SECTION);
        assert_eq!(log.preamble, ["## History"]);
        assert_eq!(log.entries.len(), 3);
        let first = &log.entries[0];
        assert!(first.pending);
        assert_eq!(first.date, "2026-10-30");
        assert_eq!(first.duration.as_deref(), Some("45m"));
        assert_eq!(first.tags, [AUTO]);
        assert_eq!(first.lines, ["Changed 3 files.", "Commits: \"fix units\""]);
        assert_eq!(log.entries[1].minutes(), Some(130));
        assert_eq!(log.entries[2].duration, None);
        assert_eq!(log.render(), SECTION);
        assert_eq!(log.pending(), [0]);
    }

    #[test]
    fn approving_keeps_the_text_and_drops_the_auto_mark() {
        let mut log = HistoryLog::parse(SECTION);
        log.approve(0).unwrap();
        assert!(
            log.render()
                .contains("### 2026-10-30 · 45m\nChanged 3 files.\n")
        );
        assert_eq!(log.approve(0), Err(EntryError::NotPending(0)));
        assert_eq!(log.approve(9), Err(EntryError::NoEntry(9)));
    }

    #[test]
    fn entries_are_added_newest_first_and_combined() {
        let mut log = HistoryLog::parse("## History\n");
        log.add(HistoryEntry::pending(
            "2026-10-28",
            Some(30),
            vec!["Morning.".into()],
        ));
        log.add(HistoryEntry::pending(
            "2026-10-29",
            Some(100),
            vec!["Afternoon.".into(), "My notes: tidy later.".into()],
        ));
        log.add(HistoryEntry::pending(
            "2026-10-27",
            None,
            vec!["Earlier.".into()],
        ));
        let dates: Vec<&str> = log.entries.iter().map(|e| e.date.as_str()).collect();
        assert_eq!(dates, ["2026-10-29", "2026-10-28", "2026-10-27"]);

        log.combine(&[0, 1]).unwrap();
        assert_eq!(log.entries.len(), 2);
        let combined = &log.entries[0];
        assert_eq!(combined.date, "2026-10-29");
        assert_eq!(combined.duration.as_deref(), Some("2h 10m"));
        assert_eq!(
            combined.lines,
            ["Afternoon.", "Morning.", "My notes: tidy later."],
            "the owner's notes last"
        );
        assert!(combined.pending);
        assert_eq!(log.combine(&[0]), Err(EntryError::TooFew));

        let dropped = log.drop_entry(1).unwrap();
        assert_eq!(dropped.date, "2026-10-27");
        let text = log.render();
        assert!(
            text.starts_with("## History\n\n> [!pending] 2026-10-29 · 2h 10m · auto-generated\n"),
            "{text}"
        );
        assert_eq!(HistoryLog::parse(&text), log, "it reads back the same");
    }

    #[test]
    fn editing_an_entry_keeps_it_pending() {
        let mut log = HistoryLog::parse(SECTION);
        assert_eq!(
            log.edit(0, "Changed 3 files.\nCommits: \"fix units\"\n\n"),
            Ok(false),
            "the same text, give or take trailing blank lines"
        );
        assert_eq!(
            log.edit(
                0,
                "Fixed the units in the damping model.\n\nMy notes: check fig 2.\n"
            ),
            Ok(true)
        );
        let entry = &log.entries[0];
        assert!(entry.pending);
        assert_eq!(entry.duration.as_deref(), Some("45m"));
        assert_eq!(
            entry.lines,
            [
                "Fixed the units in the damping model.",
                "",
                "My notes: check fig 2."
            ]
        );
        assert_eq!(
            HistoryLog::parse(&log.render()),
            log,
            "it reads back the same"
        );
        assert_eq!(log.edit(1, "Mine."), Err(EntryError::NotPending(1)));
        assert_eq!(log.edit(0, " \n\n"), Err(EntryError::Empty));
    }

    #[test]
    fn edited_text_cant_break_the_section() {
        let mut log = HistoryLog::parse(SECTION);
        for text in [
            "Did a thing.\n### 2026-10-31\nAnother.",
            "Did a thing.\n> [!pending] 2026-10-31",
            "Did a thing.\n<!-- /carrel:history -->",
            "Did a thing.\n```\nunclosed",
        ] {
            assert!(
                matches!(log.edit(0, text), Err(EntryError::Unreadable(_))),
                "{text}"
            );
        }
        assert_eq!(log, HistoryLog::parse(SECTION), "nothing changed");
        assert_eq!(
            log.edit(0, "Wrote:\n```\nlet x = 1;\n```\n> a quote"),
            Ok(true),
            "a closed code block and a quote are fine"
        );
    }

    #[test]
    fn entries_approved_by_hand_are_written_back_as_the_owner_left_them() {
        let section = "## History\n\n\
            ### 2026-10-30 · 45m · auto-generated\n\
            Changed 3 files.\n\
            \n\
            > [!done]+ 2026-10-29 · 2h 10m\n\
            > Added a phase-response analysis.\n\
            >\n\
            > My notes: good day.\n";
        let mut log = HistoryLog::parse(section);
        assert_eq!(log.entries.len(), 2);
        assert!(log.pending().is_empty(), "both are approved");
        assert_eq!(log.entries[0].tags, [AUTO]);
        let callout = &log.entries[1];
        assert_eq!(callout.date, "2026-10-29");
        assert_eq!(callout.minutes(), Some(130));
        assert_eq!(
            callout.lines,
            [
                "Added a phase-response analysis.",
                "",
                "My notes: good day."
            ]
        );
        log.add(HistoryEntry::pending(
            "2026-10-31",
            Some(20),
            vec!["New.".into()],
        ));
        let text = log.render();
        assert!(
            text.ends_with(&section["## History\n\n".len()..]),
            "the owner's entries are as they were: {text}"
        );
        assert_eq!(HistoryLog::parse(&text), log);
    }

    #[test]
    fn only_dated_callouts_are_entries() {
        assert_eq!(
            approved_callout("> [!note] 2026-10-29 · 1h"),
            Some("2026-10-29 · 1h")
        );
        assert_eq!(approved_callout("> [!tip]- 2026-10-29"), Some("2026-10-29"));
        assert_eq!(approved_callout("> [!note] Remember the cable"), None);
        assert_eq!(approved_callout("> [!pending] 2026-10-29"), None);
        assert_eq!(approved_callout("> [!Pending] 2026-10-29"), None);
        assert_eq!(approved_callout("> [!note] 2026-1-29"), None);
    }

    #[test]
    fn durations_read_and_write() {
        assert_eq!(parse_minutes("2h 10m"), Some(130));
        assert_eq!(parse_minutes("45m"), Some(45));
        assert_eq!(parse_minutes("3h"), Some(180));
        assert_eq!(parse_minutes("soon"), None);
        assert_eq!(parse_minutes(""), None);
        assert_eq!(format_minutes(130), "2h 10m");
        assert_eq!(format_minutes(45), "45m");
        assert_eq!(format_minutes(180), "3h");
    }

    #[test]
    fn text_after_a_callout_stays_with_it() {
        let section = "> [!pending] 2026-10-30 · auto-generated\n> Did a thing.\nloose line\n";
        let log = HistoryLog::parse(section);
        assert_eq!(log.entries[0].lines, ["Did a thing.", "loose line"]);
    }

    #[test]
    fn a_card_gets_entries_without_touching_the_owners_text() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sample-archive/Projects/Hardware/parts-list.csv.md");
        let text = std::fs::read_to_string(path).unwrap();
        let mut file = crate::card_file::CardFile::parse(&text).unwrap();
        let before = file.body().to_owned();
        let mut log = file.history().unwrap();
        let had = log.entries.len();
        log.add(HistoryEntry::pending(
            "2099-01-01",
            Some(20),
            vec!["Checked a part.".into()],
        ));
        file.set_history(&log).unwrap();
        let after = file.history().unwrap();
        assert_eq!(after.entries.len(), had + 1);
        assert!(after.entries[0].pending);
        let outside = |body: &str| {
            crate::sections::find_all(body)
                .unwrap()
                .iter()
                .fold(body.to_owned(), |b, s| {
                    b.replace(&body[s.whole.clone()], "")
                })
        };
        assert_eq!(
            outside(file.body()),
            outside(&before),
            "only the section changed"
        );
    }
}
