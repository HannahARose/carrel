//! Tag restructuring (`docs/design/restructure.md`, building step 4): the least invasive
//! restructure, since no files move. It proposes
//!
//! - **merging** tags that mean the same thing: case and spelling variants (`Receipt`,
//!   `receipt`), plurals (`photo`, `photos`), near spellings whose items are close in meaning
//!   (`colour`, `color`), and tags the embedding index puts very close that rarely share an
//!   item;
//! - **splitting** a tag spread over two groups of items far apart in meaning and mostly in
//!   different collections, each group getting a narrower tag;
//! - **retiring** a tag used once or twice and in no vocabulary.
//!
//! Each change says why and which items it touches. Tags the owner wrote themselves (a card
//! whose `generated.fields` doesn't list `tags`, FORMAT.md §5.6) are never changed: the item
//! is listed as held, with why. Checked-out items are held too, until they're returned.
//!
//! Grouping is numerical, from the card vectors already in the embedding index, given to
//! [`propose`] as a function so tests can give fake ones. Without an index only the name
//! variants and rare tags are proposed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::archive::Archive;
use crate::card::{Card, Extra, Id, Kind, Tag};
use crate::card_file::CardFile;
use crate::documents::{ArchiveDoc, CollectionDoc, TagDef};

use super::{
    Edit, Held, Invasiveness, Outcome, Proposal, ProposalKind, ProposedChange, RestructureError,
    SplitPart, Touched, io,
};

/// The thresholds tag restructuring uses. The defaults suit most archives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagOptions {
    /// A tag on at most this many items, and in no vocabulary, is proposed for retiring.
    pub rare_uses: usize,
    /// How close (cosine) two tags' items must be, on average, for tags with unrelated names
    /// to be proposed as synonyms.
    pub synonym_similarity: f32,
    /// How close two tags' items must be for tags one letter apart to be proposed as two
    /// spellings of one word.
    pub spelling_similarity: f32,
    /// A tag needs at least this many items in the index to be considered for splitting.
    pub split_min_items: usize,
    /// A tag is split when its two groups of items are no closer than this.
    pub split_similarity: f32,
}

impl Default for TagOptions {
    fn default() -> Self {
        Self {
            rare_uses: 2,
            synonym_similarity: 0.95,
            spelling_similarity: 0.85,
            split_min_items: 4,
            split_similarity: 0.5,
        }
    }
}

/// An item with tags, as tag restructuring sees it.
#[derive(Debug, Clone)]
struct Tagged {
    id: Id,
    card: String,
    title: String,
    collection: String,
    tags: Vec<Tag>,
    /// Why its tags can't be changed, if they can't.
    held: Option<&'static str>,
}

impl Tagged {
    fn touched(&self) -> Touched {
        Touched {
            id: self.id,
            card: self.card.clone(),
            title: self.title.clone(),
        }
    }

    fn held(&self, why: &str) -> Held {
        Held {
            id: self.id,
            card: self.card.clone(),
            title: self.title.clone(),
            why: why.to_owned(),
        }
    }
}

/// Why `card`'s tags can't be changed by a restructure, if they can't.
#[must_use]
pub fn held_because(card: &Card) -> Option<&'static str> {
    if card.checked_out.is_some() {
        return Some("it's checked out; its tags can change once it's returned");
    }
    let generated = card
        .generated
        .as_ref()
        .is_some_and(|g| g.fields.iter().any(|f| f == "tags"));
    (!generated).then_some("you wrote its tags yourself, so they stay as you wrote them")
}

/// What the archive's tags look like: every tagged item, and where each tag is defined.
struct Survey {
    items: Vec<Tagged>,
    /// Each tag's items, as indexes into `items`.
    uses: BTreeMap<Tag, Vec<usize>>,
    /// The design documents whose vocabularies define each tag.
    defined: BTreeMap<Tag, Vec<String>>,
}

impl Survey {
    fn read(archive: &Archive) -> Result<Self, RestructureError> {
        let mut defined: BTreeMap<Tag, Vec<String>> = BTreeMap::new();
        for def in &archive.doc().tags {
            defined
                .entry(def.name.clone())
                .or_default()
                .push("ARCHIVE.md".into());
        }
        let mut items = Vec::new();
        for collection in archive.collections()? {
            for def in &collection.doc.tags {
                defined
                    .entry(def.name.clone())
                    .or_default()
                    .push(format!("{}/_collection.md", collection.path));
            }
            for (name, card) in archive.cards_in(&collection)? {
                if card.kind == Kind::Tombstone || card.tags.is_empty() {
                    continue;
                }
                items.push(Tagged {
                    id: card.id,
                    card: format!("{}/{name}", collection.path),
                    title: card.title.clone(),
                    collection: collection.path.clone(),
                    held: held_because(&card),
                    tags: card.tags,
                });
            }
        }
        let mut uses: BTreeMap<Tag, Vec<usize>> = BTreeMap::new();
        for (i, item) in items.iter().enumerate() {
            let mut seen = BTreeSet::new();
            for tag in &item.tags {
                if seen.insert(tag) {
                    uses.entry(tag.clone()).or_default().push(i);
                }
            }
        }
        Ok(Self {
            items,
            uses,
            defined,
        })
    }

    fn uses(&self, tag: &Tag) -> usize {
        self.uses.get(tag).map_or(0, Vec::len)
    }

    fn definitions(&self, tag: &Tag) -> usize {
        self.defined.get(tag).map_or(0, Vec::len)
    }

    /// The touched and held items among those carrying any of `tags`.
    fn split_items(&self, tags: &[Tag]) -> (Vec<Touched>, Vec<Held>) {
        let mut indexes: BTreeSet<usize> = BTreeSet::new();
        for tag in tags {
            indexes.extend(self.uses.get(tag).into_iter().flatten().copied());
        }
        let mut touched = Vec::new();
        let mut held = Vec::new();
        for i in indexes {
            let item = &self.items[i];
            match item.held {
                Some(why) => held.push(item.held(why)),
                None => touched.push(item.touched()),
            }
        }
        (touched, held)
    }

    /// The design documents defining any of `tags`, once each, sorted.
    fn documents(&self, tags: &[Tag]) -> Vec<String> {
        let set: BTreeSet<&String> = tags
            .iter()
            .filter_map(|t| self.defined.get(t))
            .flatten()
            .collect();
        set.into_iter().cloned().collect()
    }
}

/// The item vectors of the items carrying `tag`, normalized, with the items' indexes.
fn tag_vectors(
    survey: &Survey,
    tag: &Tag,
    vectors: &dyn Fn(Id) -> Option<Vec<f32>>,
) -> Vec<(usize, Vec<f32>)> {
    survey
        .uses
        .get(tag)
        .into_iter()
        .flatten()
        .filter_map(|&i| {
            vectors(survey.items[i].id)
                .and_then(normalized)
                .map(|v| (i, v))
        })
        .collect()
}

fn normalized(mut v: Vec<f32>) -> Option<Vec<f32>> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON || !norm.is_finite() {
        return None;
    }
    for x in &mut v {
        *x /= norm;
    }
    Some(v)
}

/// The direction of the sum of `vectors`: their mean's direction, which is all cosine needs.
fn centroid<'a>(vectors: impl Iterator<Item = &'a Vec<f32>>) -> Option<Vec<f32>> {
    let mut sum: Vec<f32> = Vec::new();
    for v in vectors {
        if sum.is_empty() {
            sum = vec![0.0; v.len()];
        }
        if v.len() != sum.len() {
            return None;
        }
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x;
        }
    }
    normalized(sum)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `tag` lowercased, with anything but letters and digits made `-` and runs of `-` joined:
/// the well-formed tag (FORMAT.md §6.5) it most likely means.
#[must_use]
pub fn canonical(tag: &str) -> String {
    let mut out = String::new();
    for c in tag.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Why `a` and `b` are the same tag written differently, if their names say so.
fn same_name(a: &str, b: &str) -> Option<String> {
    let (ca, cb) = (canonical(a), canonical(b));
    if ca.len() < 3 || cb.len() < 3 {
        return None;
    }
    if ca == cb {
        return Some(format!(
            "“{a}” and “{b}” differ only in capitals or punctuation"
        ));
    }
    let plural = |one: &str, many: &str| {
        many.strip_suffix('s').is_some_and(|s| s == one)
            || many.strip_suffix("es").is_some_and(|s| s == one)
            || (one.ends_with('y') && many.strip_suffix("ies") == one.strip_suffix('y'))
    };
    if plural(&ca, &cb) {
        return Some(format!("“{b}” is the plural of “{a}”"));
    }
    if plural(&cb, &ca) {
        return Some(format!("“{a}” is the plural of “{b}”"));
    }
    None
}

/// Whether `a` and `b` are one letter apart (an insertion, deletion or substitution), long
/// enough to be words, and differ in a letter rather than a number.
fn one_letter_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().min(b.len()) < 4 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let end = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (da, db) = (&a[start..a.len() - end], &b[start..b.len() - end]);
    da.len() <= 1
        && db.len() <= 1
        && da.len() + db.len() > 0
        && da.iter().chain(db).all(char::is_ascii_alphabetic)
}

/// Joins tags into groups, remembering why each pair was joined.
#[derive(Default)]
struct Groups {
    parent: BTreeMap<Tag, Tag>,
    reasons: Vec<(Tag, String)>,
}

impl Groups {
    fn root(&self, tag: &Tag) -> Tag {
        let mut at = tag.clone();
        while let Some(up) = self.parent.get(&at) {
            if *up == at {
                break;
            }
            at = up.clone();
        }
        at
    }

    fn join(&mut self, a: &Tag, b: &Tag, reason: String) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra != rb {
            self.parent.insert(rb, ra);
        }
        self.reasons.push((a.clone(), reason));
    }

    fn joined(&self, a: &Tag, b: &Tag) -> bool {
        self.root(a) == self.root(b)
    }

    /// Every group of two or more tags, with the reasons that joined it.
    fn groups(&self, tags: &[Tag]) -> Vec<(Vec<Tag>, Vec<String>)> {
        let mut by_root: BTreeMap<Tag, Vec<Tag>> = BTreeMap::new();
        for tag in tags {
            by_root.entry(self.root(tag)).or_default().push(tag.clone());
        }
        by_root
            .into_values()
            .filter(|g| g.len() > 1)
            .map(|group| {
                let reasons = self
                    .reasons
                    .iter()
                    .filter(|(t, _)| group.contains(t))
                    .map(|(_, r)| r.clone())
                    .collect();
                (group, reasons)
            })
            .collect()
    }
}

/// Builds a tag restructure proposal for `archive`, with `vectors` giving each item's card
/// vector from the embedding index (or `None` for an item that isn't in it).
///
/// # Errors
///
/// An error reading the archive.
pub fn propose(
    archive: &Archive,
    vectors: &dyn Fn(Id) -> Option<Vec<f32>>,
    options: &TagOptions,
) -> Result<Proposal, RestructureError> {
    let survey = Survey::read(archive)?;
    let mut proposal = Proposal::new(ProposalKind::Tags, "");
    let mut settled: BTreeSet<Tag> = BTreeSet::new();
    for change in merges(&survey, vectors, options) {
        if let Edit::MergeTags { from, into } = &change.change {
            settled.extend(from.iter().cloned());
            settled.insert(into.clone());
        }
        push(&mut proposal, change);
    }
    for change in splits(&survey, vectors, options, &settled) {
        if let Edit::SplitTag { tag, .. } = &change.change {
            settled.insert(tag.clone());
        }
        push(&mut proposal, change);
    }
    for change in retirements(&survey, options, &settled) {
        push(&mut proposal, change);
    }
    proposal.rationale = rationale(&survey, &proposal);
    Ok(proposal)
}

fn push(proposal: &mut Proposal, mut change: ProposedChange) {
    change.id = (proposal.changes.len() + 1).to_string();
    proposal.changes.push(change);
}

fn rationale(survey: &Survey, proposal: &Proposal) -> String {
    let count = |f: fn(&Edit) -> bool| proposal.changes.iter().filter(|c| f(&c.change)).count();
    let merges = count(|e| matches!(e, Edit::MergeTags { .. }));
    let splits = count(|e| matches!(e, Edit::SplitTag { .. }));
    let retired = count(|e| matches!(e, Edit::RetireTag { .. }));
    let tagged = survey.items.len();
    if proposal.changes.is_empty() {
        return format!(
            "The archive's {} tags, on {tagged} items, have no synonyms, overloaded tags or \
             rare tags worth changing.",
            survey.uses.len()
        );
    }
    let held: BTreeSet<Id> = proposal
        .changes
        .iter()
        .flat_map(|c| c.held.iter().map(|h| h.id))
        .collect();
    let plural = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        plural(merges, "merge", "merges"),
        plural(splits, "split", "splits"),
        plural(retired, "retirement", "retirements"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let parts = match parts.as_slice() {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => parts.join(""),
    };
    let mut text = format!(
        "{} tags on {tagged} items: {parts}, so each idea has one tag and every tag helps find \
         something. Only tags on cards and in vocabularies change; nothing moves.",
        survey.uses.len(),
    );
    if !held.is_empty() {
        let _ = write!(
            text,
            " {} item{} you tagged yourself, or that {} checked out, keep{} {} tags.",
            held.len(),
            if held.len() == 1 { "" } else { "s" },
            if held.len() == 1 { "is" } else { "are" },
            if held.len() == 1 { "s" } else { "" },
            if held.len() == 1 { "its" } else { "their" },
        );
    }
    text
}

/// Merge proposals: groups of tags joined by their names or by their items' meaning, and
/// badly formed tags on their own, each becoming one well-formed tag.
fn merges(
    survey: &Survey,
    vectors: &dyn Fn(Id) -> Option<Vec<f32>>,
    options: &TagOptions,
) -> Vec<ProposedChange> {
    // Tags only a vocabulary defines can be merged too, so a vocabulary's `receipt` and the
    // cards' `receipts` become one.
    let tags: Vec<Tag> = survey
        .uses
        .keys()
        .chain(survey.defined.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    let centroids: BTreeMap<&Tag, (Vec<f32>, usize)> = tags
        .iter()
        .filter_map(|t| {
            let vs = tag_vectors(survey, t, vectors);
            centroid(vs.iter().map(|(_, v)| v)).map(|c| (t, (c, vs.len())))
        })
        .collect();
    let mut groups = Groups::default();
    for (i, a) in tags.iter().enumerate() {
        for b in &tags[i + 1..] {
            if let Some(reason) = same_name(&a.0, &b.0) {
                groups.join(a, b, reason);
                continue;
            }
            let (Some((ca, na)), Some((cb, nb))) = (centroids.get(a), centroids.get(b)) else {
                continue;
            };
            let similarity = cosine(ca, cb);
            if one_letter_apart(&canonical(&a.0), &canonical(&b.0))
                && similarity >= options.spelling_similarity
            {
                groups.join(
                    a,
                    b,
                    format!(
                        "“{}” and “{}” are two spellings of one word, and their items are close in \
                     meaning ({similarity:.2})",
                        a.0, b.0
                    ),
                );
            } else if *na >= 2
                && *nb >= 2
                && similarity >= options.synonym_similarity
                && shared(survey, a, b) * 2 <= (*na).min(*nb)
                && !groups.joined(a, b)
            {
                groups.join(
                    a,
                    b,
                    format!(
                        "“{}” and “{}” are on items very close in meaning ({similarity:.2}) that \
                     seldom carry both",
                        a.0, b.0
                    ),
                );
            }
        }
    }
    let mut changes: Vec<ProposedChange> = groups
        .groups(&tags)
        .into_iter()
        .filter_map(|(group, reasons)| merge(survey, &group, &reasons))
        .collect();
    // A badly formed tag with no variant to join becomes its well-formed self.
    for tag in &tags {
        let alone = !groups.parent.contains_key(tag) && !groups.parent.values().any(|p| p == tag);
        if alone && !tag.is_well_formed() && !canonical(&tag.0).is_empty() {
            let reason = format!(
                "“{}” isn't written as a tag is (lowercase, with - between words)",
                tag.0
            );
            changes.extend(merge(survey, std::slice::from_ref(tag), &[reason]));
        }
    }
    changes
}

/// How many items carry both `a` and `b`.
fn shared(survey: &Survey, a: &Tag, b: &Tag) -> usize {
    let of = |t: &Tag| -> BTreeSet<usize> {
        survey.uses.get(t).into_iter().flatten().copied().collect()
    };
    of(a).intersection(&of(b)).count()
}

/// One merge of `group` into its best tag: the one most vocabularies define, then the most
/// used, then the shortest, of those well formed; or the well-formed form of the most used.
fn merge(survey: &Survey, group: &[Tag], reasons: &[String]) -> Option<ProposedChange> {
    let best = group
        .iter()
        .filter(|t| t.is_well_formed())
        .max_by(|a, b| {
            (
                survey.definitions(a),
                survey.uses(a),
                std::cmp::Reverse(a.0.len()),
            )
                .cmp(&(
                    survey.definitions(b),
                    survey.uses(b),
                    std::cmp::Reverse(b.0.len()),
                ))
                .then_with(|| b.0.cmp(&a.0))
        })
        .cloned();
    let into = if let Some(tag) = best {
        tag
    } else {
        let most = group.iter().max_by_key(|t| survey.uses(t))?;
        Tag(canonical(&most.0))
    };
    let from: Vec<Tag> = group.iter().filter(|t| **t != into).cloned().collect();
    if from.is_empty() {
        return None;
    }
    let (items, held) = survey.split_items(&from);
    let documents = survey.documents(&from);
    if items.is_empty() && documents.is_empty() {
        return None;
    }
    let mut reason = reasons.join("; ");
    let _ = write!(reason, ", so they become “{}”.", into.0);
    if !held.is_empty() {
        reason.push_str(" Items whose tags you wrote keep theirs.");
    }
    Some(ProposedChange {
        id: String::new(),
        change: Edit::MergeTags { from, into },
        reason,
        invasiveness: Invasiveness::Rewriting,
        items,
        held,
        documents,
        depends_on: Vec::new(),
        extra: Extra::new(),
    })
}

/// Split proposals: tags whose items fall into two groups far apart in meaning.
fn splits(
    survey: &Survey,
    vectors: &dyn Fn(Id) -> Option<Vec<f32>>,
    options: &TagOptions,
    settled: &BTreeSet<Tag>,
) -> Vec<ProposedChange> {
    survey
        .uses
        .keys()
        .filter(|t| !settled.contains(*t) && t.is_well_formed())
        .filter_map(|tag| {
            let vs = tag_vectors(survey, tag, vectors);
            if vs.len() < options.split_min_items {
                return None;
            }
            let (groups, similarity) = two_groups(&vs)?;
            if similarity > options.split_similarity
                || groups.iter().any(|g| g.len() < 2)
                || main_collection(survey, &groups[0]) == main_collection(survey, &groups[1])
            {
                return None;
            }
            split(survey, tag, &groups, similarity)
        })
        .collect()
}

/// The collection most of the items `group` (indexes into the survey's items) are in.
fn main_collection<'a>(survey: &'a Survey, group: &[usize]) -> Option<&'a str> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for &i in group {
        *counts.entry(&survey.items[i].collection).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(c, _)| c)
}

/// Splits `vectors` into two groups by two-means, seeded with the two vectors farthest
/// apart, and gives how close the groups' centres are.
fn two_groups(vectors: &[(usize, Vec<f32>)]) -> Option<([Vec<usize>; 2], f32)> {
    let farthest = |from: &[f32]| {
        vectors
            .iter()
            .min_by(|a, b| cosine(from, &a.1).total_cmp(&cosine(from, &b.1)))
            .map(|(_, v)| v.clone())
    };
    let a = farthest(&vectors.first()?.1)?;
    let b = farthest(&a)?;
    let mut centres = [a, b];
    let mut groups: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for _ in 0..20 {
        let mut next: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        for (at, (_, v)) in vectors.iter().enumerate() {
            let side = usize::from(cosine(v, &centres[1]) > cosine(v, &centres[0]));
            next[side].push(at);
        }
        if next == groups {
            break;
        }
        groups = next;
        for (side, group) in groups.iter().enumerate() {
            if let Some(c) = centroid(group.iter().map(|&at| &vectors[at].1)) {
                centres[side] = c;
            }
        }
    }
    let similarity = cosine(&centres[0], &centres[1]);
    let groups = groups.map(|g| g.into_iter().map(|at| vectors[at].0).collect());
    Some((groups, similarity))
}

/// One split of `tag` into a narrower tag for each of `groups` (indexes into the survey's
/// items), named after what sets each group apart.
fn split(
    survey: &Survey,
    tag: &Tag,
    groups: &[Vec<usize>; 2],
    similarity: f32,
) -> Option<ProposedChange> {
    let names = [0, 1].map(|side| part_name(survey, tag, &groups[side], &groups[1 - side]));
    let names = if names[0].0 == names[1].0 || names.iter().any(|n| n.0.is_empty()) {
        [1, 2].map(|n| {
            (
                n.to_string(),
                format!("group {n} of the items tagged “{}”", tag.0),
            )
        })
    } else {
        names
    };
    let mut items = Vec::new();
    let mut held = Vec::new();
    let mut parts = Vec::new();
    let mut grouped: BTreeSet<usize> = BTreeSet::new();
    for (side, (qualifier, about)) in names.iter().enumerate() {
        let part = Tag(format!("{}-{qualifier}", tag.0));
        let mut ids = Vec::new();
        for &i in &groups[side] {
            grouped.insert(i);
            let item = &survey.items[i];
            if let Some(why) = item.held {
                held.push(item.held(why));
            } else {
                items.push(item.touched());
                ids.push(item.id);
            }
        }
        parts.push(SplitPart {
            tag: part,
            description: format!("Split from “{}”: {about}", tag.0),
            items: ids,
        });
    }
    for &i in survey.uses.get(tag).into_iter().flatten() {
        if !grouped.contains(&i) {
            let item = &survey.items[i];
            held.push(item.held(
                item.held.unwrap_or(
                    "it isn't in the meaning index yet, so it can't be placed in a group",
                ),
            ));
        }
    }
    if items.is_empty() {
        return None;
    }
    let mut reason = format!(
        "“{}” is on two groups of items far apart in meaning ({similarity:.2}): {} and {}, so \
         each gets a narrower tag.",
        tag.0, names[0].1, names[1].1
    );
    if !held.is_empty() {
        reason.push_str(" Items it can't place, or whose tags you wrote, keep the old tag.");
    }
    Some(ProposedChange {
        id: String::new(),
        change: Edit::SplitTag {
            tag: tag.clone(),
            into: parts,
        },
        reason,
        invasiveness: Invasiveness::Rewriting,
        items,
        held,
        documents: survey.documents(std::slice::from_ref(tag)),
        depends_on: Vec::new(),
        extra: Extra::new(),
    })
}

/// What sets the items `mine` apart from `theirs`: a tag most of them carry and none of the
/// others do, else the collection most of them are in. Gives the qualifier for the new tag and
/// a phrase describing the group.
fn part_name(survey: &Survey, tag: &Tag, mine: &[usize], theirs: &[usize]) -> (String, String) {
    let tags_of = |group: &[usize]| -> BTreeMap<&Tag, usize> {
        let mut counts = BTreeMap::new();
        for &i in group {
            let unique: BTreeSet<&Tag> = survey.items[i].tags.iter().collect();
            for t in unique {
                *counts.entry(t).or_insert(0) += 1;
            }
        }
        counts
    };
    let (ours, others) = (tags_of(mine), tags_of(theirs));
    let telling = ours
        .iter()
        .filter(|(t, n)| **t != tag && !others.contains_key(*t) && **n * 2 >= mine.len())
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)));
    if let Some((t, _)) = telling {
        return (canonical(&t.0), format!("the items also tagged “{}”", t.0));
    }
    let Some(collection) = main_collection(survey, mine) else {
        return (String::new(), String::new());
    };
    let last = collection.rsplit('/').next().unwrap_or(collection);
    (canonical(last), format!("the items in {collection}"))
}

/// Retirement proposals: tags on at most `rare_uses` items and in no vocabulary.
fn retirements(
    survey: &Survey,
    options: &TagOptions,
    settled: &BTreeSet<Tag>,
) -> Vec<ProposedChange> {
    survey
        .uses
        .iter()
        .filter(|(tag, uses)| {
            !settled.contains(*tag)
                && uses.len() <= options.rare_uses
                && survey.definitions(tag) == 0
        })
        .filter_map(|(tag, uses)| {
            let (items, held) = survey.split_items(std::slice::from_ref(tag));
            if items.is_empty() {
                return None;
            }
            let mut reason = format!(
                "“{}” is on only {} and in no vocabulary, so it doesn't help find anything.",
                tag.0,
                if uses.len() == 1 {
                    "one item".to_owned()
                } else {
                    format!("{} items", uses.len())
                }
            );
            if !held.is_empty() {
                reason.push_str(" Items whose tags you wrote keep it.");
            }
            Some(ProposedChange {
                id: String::new(),
                change: Edit::RetireTag { tag: tag.clone() },
                reason,
                invasiveness: Invasiveness::Rewriting,
                items,
                held,
                documents: Vec::new(),
                depends_on: Vec::new(),
                extra: Extra::new(),
            })
        })
        .collect()
}

/// `tags` with `edit` applied for the item `id`.
fn rewrite(edit: &Edit, id: Id, tags: &[Tag]) -> Vec<Tag> {
    let mapped = tags.iter().filter_map(|t| match edit {
        Edit::MergeTags { from, into } if from.contains(t) => Some(into.clone()),
        Edit::SplitTag { tag, into } if t == tag => Some(
            into.iter()
                .find(|p| p.items.contains(&id))
                .map_or_else(|| t.clone(), |p| p.tag.clone()),
        ),
        Edit::RetireTag { tag } if t == tag => None,
        _ => Some(t.clone()),
    });
    let mut seen = BTreeSet::new();
    mapped.filter(|t| seen.insert(t.clone())).collect()
}

/// Applies one tag change to the archive as it is now: rewrites the cards of the items it
/// touches that can still be changed, then the vocabularies it names.
///
/// # Errors
///
/// An error reading or writing a card or design document.
pub(super) fn apply(
    archive: &Archive,
    change: &ProposedChange,
) -> Result<Outcome, RestructureError> {
    let mut outcome = Outcome {
        change: change.id.clone(),
        ..Outcome::default()
    };
    let cards: BTreeMap<Id, String> = archive
        .items()?
        .into_iter()
        .map(|item| (item.id, item.card))
        .collect();
    for touched in &change.items {
        let Some(card) = cards.get(&touched.id) else {
            outcome
                .skipped
                .push((touched.id, "it's no longer in the archive".into()));
            continue;
        };
        match rewrite_card(archive, card, touched.id, &change.change)? {
            Ok(()) => outcome.items.push(touched.id),
            Err(why) => outcome.skipped.push((touched.id, why)),
        }
    }
    let (documents, kept) = rewrite_vocabularies(archive, change)?;
    outcome.documents = documents;
    outcome.kept = kept;
    Ok(outcome)
}

/// Rewrites one card's tags; the inner error says why it was left alone.
fn rewrite_card(
    archive: &Archive,
    card: &str,
    id: Id,
    edit: &Edit,
) -> Result<Result<(), String>, RestructureError> {
    let path = archive.root().join(card);
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let mut file =
        CardFile::parse(&text).map_err(|e| RestructureError::Invalid(format!("{card}: {e}")))?;
    if let Some(why) = held_because(file.card()) {
        return Ok(Err(why.to_owned()));
    }
    let tags = rewrite(edit, id, &file.card().tags);
    if tags == file.card().tags {
        return Ok(Err(
            "its tags have changed since; it no longer has the tag".into()
        ));
    }
    file.card_mut().tags = tags;
    let text = file
        .to_text()
        .map_err(|e| RestructureError::Invalid(format!("{card}: {e}")))?;
    replace(&path, &text)?;
    Ok(Ok(()))
}

fn replace(path: &std::path::Path, text: &str) -> Result<(), RestructureError> {
    let temp = path.with_extension(format!("tmp-{}", Id::new_v7()));
    std::fs::write(&temp, text).map_err(io(&temp))?;
    std::fs::rename(&temp, path).map_err(io(path))
}

/// Whether any card in the archive still carries `tag`.
fn still_used(archive: &Archive, tag: &Tag) -> Result<bool, RestructureError> {
    for collection in archive.collections()? {
        if archive
            .cards_in(&collection)?
            .iter()
            .any(|(_, card)| card.kind != Kind::Tombstone && card.tags.contains(tag))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Rewrites the vocabularies of the design documents `change` names. Returns the documents
/// changed, and the entries kept because items left alone still carry them.
fn rewrite_vocabularies(
    archive: &Archive,
    change: &ProposedChange,
) -> Result<(Vec<String>, Vec<String>), RestructureError> {
    let old: Vec<Tag> = match &change.change {
        Edit::MergeTags { from, .. } => from.clone(),
        Edit::SplitTag { tag, .. } | Edit::RetireTag { tag } => vec![tag.clone()],
    };
    let mut in_use = BTreeSet::new();
    for tag in &old {
        if still_used(archive, tag)? {
            in_use.insert(tag.clone());
        }
    }
    let mut documents = Vec::new();
    let mut kept = BTreeSet::new();
    for document in &change.documents {
        let changed = edit_vocabulary(archive, document, |defs| {
            vocabulary(&change.change, defs, &in_use, &mut kept)
        })?;
        if changed {
            if let Some(collection) = document.strip_suffix("/_collection.md") {
                let entry = format!("tags: {} (restructure)", change.change.describe());
                crate::listing::add_to_changelog(archive, collection, &entry)
                    .map_err(io(&archive.root().join(collection).join("CHANGELOG.md")))?;
            }
            documents.push(document.clone());
        }
    }
    Ok((documents, kept.into_iter().collect()))
}

/// Applies `edit` to one vocabulary, keeping entries for tags in `in_use`. Returns whether it
/// changed.
fn vocabulary(
    edit: &Edit,
    defs: &mut Vec<TagDef>,
    in_use: &BTreeSet<Tag>,
    kept: &mut BTreeSet<String>,
) -> bool {
    let before = defs.clone();
    let define = |defs: &mut Vec<TagDef>, tag: &Tag, description: String| {
        if !defs.iter().any(|d| d.name == *tag) {
            defs.push(TagDef {
                name: tag.clone(),
                description,
                extra: Extra::new(),
            });
        }
    };
    let mut retire = |defs: &mut Vec<TagDef>, tag: &Tag| {
        if in_use.contains(tag) {
            kept.insert(tag.0.clone());
        } else {
            defs.retain(|d| d.name != *tag);
        }
    };
    match edit {
        Edit::MergeTags { from, into } => {
            for tag in from {
                let Some(at) = defs.iter().position(|d| d.name == *tag) else {
                    continue;
                };
                let description = defs[at].description.clone();
                if !defs.iter().any(|d| d.name == *into) && !in_use.contains(tag) {
                    // Take its place, so the vocabulary keeps its order.
                    defs[at].name = into.clone();
                    continue;
                }
                define(defs, into, description);
                retire(defs, tag);
            }
        }
        Edit::SplitTag { tag, into } => {
            if defs.iter().any(|d| d.name == *tag) {
                for part in into {
                    define(defs, &part.tag, part.description.clone());
                }
                retire(defs, tag);
            }
        }
        Edit::RetireTag { tag } => retire(defs, tag),
    }
    *defs != before
}

/// Reads the design document at `document` (relative to the archive root), lets `edit` change
/// its vocabulary, and writes it back if it changed, keeping its body exactly.
fn edit_vocabulary(
    archive: &Archive,
    document: &str,
    edit: impl FnOnce(&mut Vec<TagDef>) -> bool,
) -> Result<bool, RestructureError> {
    let path = archive.root().join(document);
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let invalid = |e: String| RestructureError::Invalid(format!("{document}: {e}"));
    let split = crate::frontmatter::split(&text).map_err(|e| invalid(e.to_string()))?;
    let yaml = if document == "ARCHIVE.md" {
        let mut doc: ArchiveDoc =
            serde_saphyr::from_str(split.yaml).map_err(|e| invalid(e.to_string()))?;
        if !edit(&mut doc.tags) {
            return Ok(false);
        }
        serde_saphyr::to_string(&doc).map_err(|e| invalid(e.to_string()))?
    } else {
        let mut doc: CollectionDoc =
            serde_saphyr::from_str(split.yaml).map_err(|e| invalid(e.to_string()))?;
        if !edit(&mut doc.tags) {
            return Ok(false);
        }
        serde_saphyr::to_string(&doc).map_err(|e| invalid(e.to_string()))?
    };
    replace(&path, &crate::frontmatter::join(&yaml, split.body))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_differ_in_case_punctuation_or_number_are_the_same_tag() {
        assert!(same_name("Receipt", "receipt").is_some());
        assert!(same_name("photo", "photos").is_some());
        assert!(same_name("tax", "taxes").is_some());
        assert!(same_name("library", "libraries").is_some());
        assert!(same_name("c_elegans", "c-elegans").is_some());
        assert!(same_name("photo", "photon").is_none());
        assert!(same_name("ab", "abs").is_none());
    }

    #[test]
    fn one_letter_apart_needs_words_and_letters() {
        assert!(one_letter_apart("colour", "color"));
        assert!(one_letter_apart("grey", "gray"));
        assert!(!one_letter_apart("v2024", "v2025"));
        assert!(!one_letter_apart("cat", "car"));
        assert!(!one_letter_apart("colour", "colour"));
    }

    #[test]
    fn canonical_makes_a_well_formed_tag() {
        assert_eq!(canonical(" Phase Response "), "phase-response");
        assert_eq!(canonical("C_elegans!"), "c-elegans");
        assert_eq!(canonical("--"), "");
    }

    #[test]
    fn rewriting_keeps_order_and_drops_duplicates() {
        let id = Id::new_v7();
        let tags = |names: &[&str]| names.iter().map(|n| Tag((*n).into())).collect::<Vec<_>>();
        let merge = Edit::MergeTags {
            from: tags(&["receipts"]),
            into: Tag("receipt".into()),
        };
        assert_eq!(
            rewrite(&merge, id, &tags(&["a", "receipts", "receipt", "b"])),
            tags(&["a", "receipt", "b"])
        );
        let retire = Edit::RetireTag {
            tag: Tag("b".into()),
        };
        assert_eq!(rewrite(&retire, id, &tags(&["a", "b"])), tags(&["a"]));
    }
}
