//! What's embedded for an item (`docs/design/search.md`): its card as one text, and its own
//! text cut into passages.

use std::ops::Range;

use crate::card::Card;

/// About how many tokens a passage holds.
pub const PASSAGE_TOKENS: usize = 500;

/// The most passages embedded for one item: a long document is found by what's near its
/// start, as the text cache keeps only its start.
pub const MAX_PASSAGES: usize = 20;

/// A passage's length in bytes, by the estimate of three bytes a token that
/// [`estimate_tokens`](crate::providers::estimate_tokens) makes.
const PASSAGE_BYTES: usize = 3 * PASSAGE_TOKENS;

/// An item's card as the one text that's embedded for it: its title, summary, tags,
/// collection, dates and kind, a line each. `collection` is its folder, relative to the
/// archive root.
#[must_use]
pub fn card_text(card: &Card, collection: &str) -> String {
    let mut lines = vec![card.title.trim().to_owned()];
    if let Some(summary) = card.summary.as_deref().map(str::trim)
        && !summary.is_empty()
    {
        lines.push(summary.to_owned());
    }
    if !card.tags.is_empty() {
        let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
        lines.push(format!("Tags: {}", tags.join(", ")));
    }
    if !collection.is_empty() {
        lines.push(format!("Collection: {}", collection.replace('/', " / ")));
    }
    if let Some(dates) = &card.dates {
        // The dates that say what the item is; when it was last worked on or modified changes
        // too often to be worth embedding again.
        let named = [
            ("document", &dates.document),
            ("created", &dates.created),
            ("added", &dates.added),
        ];
        let days: Vec<String> = named
            .iter()
            .filter_map(|(name, d)| d.as_ref().map(|d| format!("{name} {}", d.0)))
            .collect();
        if !days.is_empty() {
            lines.push(format!("Dates: {}", days.join(", ")));
        }
    }
    let kind = serde_json::to_value(card.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    lines.push(format!("Kind: {kind}"));
    lines.join("\n")
}

/// The largest character boundary in `text` at or before `at`.
fn boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `at` moved past any white space in `text`.
fn skip_space(text: &str, at: usize) -> usize {
    text[at..]
        .find(|c: char| !c.is_whitespace())
        .map_or(text.len(), |p| at + p)
}

/// Where `text` is cut into passages of about [`PASSAGE_TOKENS`] tokens, at most
/// [`MAX_PASSAGES`] of them, in order: each ends at a paragraph's or sentence's end when one
/// is in its second half, else between words. White space around them is left out.
#[must_use]
pub fn passages(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut at = skip_space(text, 0);
    while at < text.len() && found.len() < MAX_PASSAGES {
        let limit = boundary(text, at + PASSAGE_BYTES);
        let end = if limit >= text.len() {
            text.len()
        } else {
            let window = &text[at..limit];
            let half = window.len() / 2;
            window
                .rfind("\n\n")
                .filter(|&p| p >= half)
                .or_else(|| {
                    ["。", ". ", "? ", "! ", ".\n"]
                        .iter()
                        .filter_map(|end| window.rfind(end).map(|p| p + end.len()))
                        .max()
                        .filter(|&p| p >= half)
                })
                .or_else(|| window.rfind(char::is_whitespace).filter(|&p| p > 0))
                .map_or(limit, |p| at + p)
        };
        let passage = text[at..end].trim_end();
        if !passage.is_empty() {
            found.push(at..at + passage.len());
        }
        let next = skip_space(text, end);
        // When nothing was taken, move past one character so the cut always moves on.
        at = if next > at {
            next
        } else {
            at + text[at..].chars().next().map_or(1, char::len_utf8)
        };
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_passage_and_empty_text_none() {
        assert_eq!(passages("  A note.\n"), vec![2..9]);
        assert_eq!(passages(" \n\t"), Vec::<Range<usize>>::new());
        assert_eq!(passages(""), Vec::<Range<usize>>::new());
    }

    #[test]
    fn long_text_is_cut_between_sentences_up_to_the_limit() {
        let sentence = "The lease for the flat on Cowley Road runs a year. ";
        let text = sentence.repeat(200);
        let cut = passages(&text);
        assert!(cut.len() > 1 && cut.len() <= MAX_PASSAGES);
        for range in &cut {
            let passage = &text[range.clone()];
            assert!(passage.len() <= PASSAGE_BYTES, "{}", passage.len());
            assert!(passage.len() > PASSAGE_BYTES / 2);
            assert!(
                passage.starts_with("The") && passage.ends_with('.'),
                "{passage}"
            );
        }
        assert!(cut.windows(2).all(|w| w[0].end < w[1].start));

        let long = "word ".repeat(100_000);
        assert_eq!(passages(&long).len(), MAX_PASSAGES);
    }

    #[test]
    fn text_without_spaces_is_cut_on_character_boundaries() {
        let text = "é".repeat(2000);
        let cut = passages(&text);
        assert!(cut.len() >= 2);
        for range in cut {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn paragraphs_are_preferred_to_sentences() {
        let first = "Short. ".repeat(150);
        let text = format!("{}\n\n{}", first.trim_end(), "Second part. ".repeat(100));
        let cut = passages(&text);
        assert_eq!(&text[cut[0].clone()], first.trim_end());
    }
}
