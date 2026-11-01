//! The card fields the owner sets beyond the title, summary, tags, dates and sensitivity: its
//! links to other items, where a physical or external item is, a URL item's address, and when
//! its working copy is snapshotted (FORMAT.md §5.2). Each is checked as the card's own checks
//! and the archive's would check it ([`crate::card::Card::problems`]).

use crate::archive::{Archive, ItemRef};
use crate::card::{Card, Extra, Id, Kind, Link, LinkType, Snapshots};

/// A link to another item as the owner edits it (FORMAT.md §5.4). Carrel keeps the target
/// card's path itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkField {
    /// The link's type: a built-in one or one `ARCHIVE.md` declares.
    #[serde(rename = "type")]
    pub link_type: String,
    /// The item linked to: its identifier, or enough of its start or end to tell it apart.
    pub id: String,
    /// A short explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl From<&Link> for LinkField {
    fn from(link: &Link) -> Self {
        Self {
            link_type: link.link_type.0.clone(),
            id: link.id.to_string(),
            note: link.note.clone(),
        }
    }
}

/// The owner's other fields, checked, as they'd go on the card.
pub(super) struct CheckedOwnerFields {
    pub links: Vec<Link>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub snapshots: Option<Snapshots>,
}

impl CheckedOwnerFields {
    /// The names of the fields that differ from `card`'s.
    pub fn changed_from(&self, card: &Card) -> Vec<&'static str> {
        [
            ("links", self.links != card.links),
            ("location", self.location != card.location),
            ("url", self.url != card.url),
            ("snapshots", self.snapshots != card.snapshots),
        ]
        .into_iter()
        .filter_map(|(field, differs)| differs.then_some(field))
        .collect()
    }

    /// Puts the fields on `card`.
    pub fn apply_to(self, card: &mut Card) {
        card.links = self.links;
        card.location = self.location;
        card.url = self.url;
        card.snapshots = self.snapshots;
    }
}

/// One line of text, white space run together; `None` when nothing's left.
fn one_line(text: Option<&str>) -> Option<String> {
    text.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty())
}

/// Whether `url` is an absolute address: a scheme, such as `https:`, then something, with no
/// spaces.
fn is_absolute_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    let mut letters = scheme.chars();
    letters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && letters.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        && !rest.is_empty()
        && !url.chars().any(char::is_whitespace)
}

/// Checks the links, location, URL and snapshots of `card`, a card going in the folder
/// `folder`, edited to `links`, `location`, `url` and `snapshots`. A link's target is found
/// among the archive's items, by its identifier or the start or end of it; links already on
/// the card keep what Carrel keeps on them.
///
/// # Errors
///
/// The problems, in words, when any doesn't check out; or the archive's error when its items
/// can't be read, as the only problem.
pub(super) fn check_owner_fields(
    archive: &Archive,
    card: &Card,
    folder: &str,
    links: &[LinkField],
    (location, url, snapshots): (Option<&str>, Option<&str>, Option<Snapshots>),
) -> Result<CheckedOwnerFields, Vec<String>> {
    let mut problems = Vec::new();
    let location = one_line(location);
    if location.is_none() && matches!(card.kind, Kind::Physical | Kind::External) {
        problems.push("the location can't be empty: say where the item is".to_owned());
    }
    let url = url
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_owned);
    match &url {
        None if card.kind == Kind::Url => {
            problems.push("the url can't be empty for a URL item".to_owned());
        }
        Some(url) if !is_absolute_url(url) => problems.push(format!(
            "the url “{url}” isn't an address, such as https://example.org/page"
        )),
        _ => {}
    }
    if snapshots.is_some() && card.kind.is_card_only() {
        problems.push(
            "snapshots only apply to items that are checked out: files, folders and bundles"
                .to_owned(),
        );
    }
    let links = if links.is_empty() {
        Vec::new()
    } else {
        match archive.items() {
            Ok(items) => check_links(archive, card, folder, links, &items, &mut problems),
            Err(e) => return Err(vec![format!("the archive's items can't be read: {e}")]),
        }
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(CheckedOwnerFields {
        links,
        location,
        url,
        snapshots,
    })
}

/// The links, each to one item other than `card`'s, of a type the archive knows, each kept
/// once; problems go in `problems`.
fn check_links(
    archive: &Archive,
    card: &Card,
    folder: &str,
    links: &[LinkField],
    items: &[ItemRef],
    problems: &mut Vec<String>,
) -> Vec<Link> {
    let declared: Vec<&str> = archive
        .doc()
        .link_types
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    let mut checked: Vec<Link> = Vec::new();
    for field in links {
        let link_type = LinkType(field.link_type.trim().to_owned());
        if !link_type.is_built_in() && !declared.contains(&link_type.0.as_str()) {
            let mut known: Vec<&str> = LinkType::BUILT_IN.to_vec();
            known.extend(&declared);
            problems.push(format!(
                "the link type “{}” isn't one of {}, or one ARCHIVE.md declares",
                field.link_type,
                known.join(", ")
            ));
        }
        let Some(target) = link_target(items, &field.id, card.id, problems) else {
            continue;
        };
        if checked
            .iter()
            .any(|l| l.id == target.id && l.link_type == link_type)
        {
            continue;
        }
        let note = one_line(field.note.as_deref());
        // A link that was already there keeps its path and fields this version doesn't know.
        let link = match card
            .links
            .iter()
            .find(|l| l.id == target.id && l.link_type == link_type)
        {
            Some(was) => Link {
                note,
                ..was.clone()
            },
            None => Link {
                link_type,
                id: target.id,
                path: Some(crate::listing::relative(folder, &target.card)),
                note,
                extra: Extra::new(),
            },
        };
        checked.push(link);
    }
    checked
}

/// The one item `wanted` names, by its identifier or the start or end of it, other than the
/// card's own `this`; a problem in `problems` when there's none or several.
fn link_target<'a>(
    items: &'a [ItemRef],
    wanted: &str,
    this: Id,
    problems: &mut Vec<String>,
) -> Option<&'a ItemRef> {
    let wanted_lower = wanted.trim().to_lowercase();
    let matches: Vec<&ItemRef> = if wanted_lower.is_empty() {
        Vec::new()
    } else {
        items
            .iter()
            .filter(|i| {
                let id = i.id.to_string();
                id.starts_with(&wanted_lower) || id.ends_with(&wanted_lower)
            })
            .collect()
    };
    match matches.as_slice() {
        [one] if one.id == this => {
            problems.push("a card can't link to itself".to_owned());
            None
        }
        [one] => Some(one),
        [] => {
            problems.push(format!(
                "the link to “{wanted}” names no item in the archive"
            ));
            None
        }
        _ => {
            problems.push(format!(
                "the link to “{wanted}” matches several items; give more of its identifier"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_needs_a_scheme_and_no_spaces() {
        for good in [
            "https://example.org/page",
            "mailto:me@example.org",
            "x-y.z+1:a",
        ] {
            assert!(is_absolute_url(good), "{good}");
        }
        for bad in [
            "example.org",
            "://example.org",
            "1http://x",
            "https://a b",
            "https:",
        ] {
            assert!(!is_absolute_url(bad), "{bad}");
        }
    }
}
