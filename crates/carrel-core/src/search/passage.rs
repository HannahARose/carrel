//! The passage shown with a result: the stretch of text with the most of the query's words,
//! those words marked.

use std::collections::HashSet;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::words::tokens;

/// About how many words a passage holds.
const WINDOW: usize = 30;

/// A stretch of a field's text, with where the query's words are in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passage {
    /// The text, with runs of white space made single spaces, and `…` where it was cut.
    pub text: String,
    /// Where the query's words are in `text`, in bytes, in order and not overlapping.
    pub highlights: Vec<Range<usize>>,
}

impl Passage {
    /// The text with each highlight between `open` and `close`, such as `**` and `**`.
    #[must_use]
    pub fn marked(&self, open: &str, close: &str) -> String {
        let mut out = String::new();
        let mut at = 0;
        for range in &self.highlights {
            out.push_str(&self.text[at..range.start]);
            out.push_str(open);
            out.push_str(&self.text[range.clone()]);
            out.push_str(close);
            at = range.end;
        }
        out.push_str(&self.text[at..]);
        out
    }
}

/// The passage of `text` with the most of `terms`, or its start when none are in it. Returns
/// `None` for text that's empty or only white space.
#[must_use]
pub fn best<S: std::hash::BuildHasher>(text: &str, terms: &HashSet<String, S>) -> Option<Passage> {
    let found = tokens(text);
    if text.trim().is_empty() {
        return None;
    }
    // The words as written, between white space, and whether any of their tokens matched.
    let mut words: Vec<(Range<usize>, bool)> = Vec::new();
    let mut offset = 0;
    for raw in text.split_inclusive(char::is_whitespace) {
        let span = offset..offset + raw.trim_end().len();
        offset += raw.len();
        if !span.is_empty() {
            let hit = found.iter().any(|t| {
                t.span.start >= span.start && t.span.end <= span.end && terms.contains(&t.term)
            });
            words.push((span, hit));
        }
    }
    let (mut first, mut best_seen) = (0, (0, 0));
    for start in 0..words.len() {
        let window = &words[start..words.len().min(start + WINDOW)];
        let hits = window.iter().filter(|(_, hit)| *hit);
        let distinct: HashSet<String> = hits
            .clone()
            .map(|(span, _)| text[span.clone()].to_lowercase())
            .collect();
        let score = (distinct.len(), hits.count());
        if score > best_seen {
            best_seen = score;
            first = start;
        }
    }
    // Start a little before the first match, so it's read in context.
    let first_hit = words[first..]
        .iter()
        .position(|(_, hit)| *hit)
        .map_or(0, |p| p + first);
    let first = first_hit.saturating_sub(4);
    let last = (first + WINDOW).min(words.len()) - 1;
    let range = words[first].0.start..words[last].0.end;
    let highlights: Vec<Range<usize>> = found
        .iter()
        .filter(|t| {
            terms.contains(&t.term) && t.span.start >= range.start && t.span.end <= range.end
        })
        .map(|t| t.span.clone())
        .collect();
    Some(squash(
        text,
        range,
        &merge(highlights),
        first > 0,
        last + 1 < words.len(),
    ))
}

/// Overlapping or touching ranges joined, in order.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// `text[range]` with white space made single spaces, the highlights moved to match, and
/// `…` before or after where it was cut.
fn squash(
    text: &str,
    range: Range<usize>,
    highlights: &[Range<usize>],
    cut_before: bool,
    cut_after: bool,
) -> Passage {
    let mut out = String::new();
    if cut_before {
        out.push_str("… ");
    }
    let mut moved = Vec::new();
    let mut open: Option<usize> = None;
    let mut space = false;
    for (at, c) in text[range.clone()].char_indices() {
        let at = at + range.start;
        if highlights.iter().any(|h| h.end == at)
            && let Some(start) = open.take()
        {
            moved.push(start..out.len());
        }
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if highlights.iter().any(|h| h.start == at) {
            open = Some(out.len());
        }
        out.push(c);
    }
    if let Some(start) = open {
        moved.push(start..out.len());
    }
    if cut_after {
        out.push_str(" …");
    }
    Passage {
        text: out,
        highlights: moved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn the_passage_with_the_most_words_is_chosen_and_marked() {
        let filler = "nothing to see here at all. ".repeat(20);
        let text = format!(
            "{filler}Please pay\ninvoice  2057-4038 by Friday; the invoice covers a chair. {filler}"
        );
        let passage = best(&text, &terms(&["invoice", "2057-4038", "2057", "4038"])).unwrap();
        assert!(passage.text.starts_with("… "), "{}", passage.text);
        assert!(passage.text.ends_with(" …"));
        assert!(
            passage
                .marked("[", "]")
                .contains("all. Please pay [invoice] [2057-4038] by Friday; the [invoice] covers"),
            "{}",
            passage.marked("[", "]")
        );
    }

    #[test]
    fn text_without_the_words_shows_its_start() {
        let passage = best("A short note.", &terms(&["chair"])).unwrap();
        assert_eq!(passage.text, "A short note.");
        assert_eq!(passage.highlights.len(), 0);
        assert_eq!(best("  \n", &terms(&["chair"])), None);
    }
}
