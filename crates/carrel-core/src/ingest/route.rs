//! Stage 8: choosing an item's collection, with the owner's rules first and then a model
//! (`docs/design/ingest.md`).

use std::fmt::Write as _;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use crate::archive::{Archive, Collection};
use crate::card::{Id, Kind, Tag};
use crate::detect::NamePattern;
use crate::frontmatter;
use crate::providers::{Capability, CompletionRequest, Offer, Provider, ProviderError, Registry};
use crate::review::{Attempt, Confidence};

use super::Candidate;
use super::card::{Draft, Vocabulary};

/// Where routing rules live, relative to the archive root.
pub const RULES_FILE: &str = "_System/rules/routing.yaml";

/// The name models see for Holding.
const HOLDING_CHOICE: &str = "Holding";

/// The name models see for "the collection itself, not a subcollection".
const HERE_CHOICE: &str = "(here)";

/// How much of a design-document section goes into a routing prompt, in bytes.
const SECTION_BUDGET: usize = 600;

/// What a routing rule matches. Every key given must hold (§7.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Match {
    /// The file's extension, without the dot, ignoring case.
    #[serde(default)]
    pub extension: Option<String>,
    /// A regular expression on the item's name.
    #[serde(default)]
    pub name: Option<String>,
    /// A file in a folder's top level, as a pattern with `*`.
    #[serde(default)]
    pub marker: Option<String>,
    /// The folder the item came from; matches that folder and anything inside it.
    #[serde(default)]
    pub source: Option<String>,
}

/// An owner-written routing rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rule {
    /// The rule's name, for people.
    pub name: String,
    /// What it matches.
    #[serde(rename = "match")]
    pub matches: Match,
    /// The collection it files into.
    pub collection: Id,
    /// Whether matching items skip review. Sensitive items wait regardless.
    #[serde(default)]
    pub auto_file: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RulesFile {
    #[serde(default)]
    rules: Vec<Rule>,
}

/// The archive's routing rules, in order.
#[derive(Debug, Clone, Default)]
pub struct RoutingRules {
    /// The rules, first first.
    pub rules: Vec<Rule>,
    /// Rules that couldn't be used, and why.
    pub problems: Vec<String>,
}

impl RoutingRules {
    /// Reads `_System/rules/routing.yaml`. A missing file means no rules; a malformed one, or
    /// a rule with an invalid pattern or an unknown collection, is reported and left out.
    #[must_use]
    pub fn of(archive: &Archive, collections: &[Collection]) -> Self {
        let Ok(text) = std::fs::read_to_string(archive.root().join(RULES_FILE)) else {
            return Self::default();
        };
        let file: RulesFile = match serde_saphyr::from_str(&text) {
            Ok(file) => file,
            Err(e) => {
                return Self {
                    rules: Vec::new(),
                    problems: vec![format!("{RULES_FILE}: {e}")],
                };
            }
        };
        let mut rules = Vec::new();
        let mut problems = Vec::new();
        for rule in file.rules {
            if let Some(pattern) = &rule.matches.name
                && let Err(e) = regex::Regex::new(pattern)
            {
                problems.push(format!("rule \u{201c}{}\u{201d}: {e}", rule.name));
                continue;
            }
            if !collections.iter().any(|c| c.doc.id == rule.collection) {
                problems.push(format!(
                    "rule \u{201c}{}\u{201d}: no collection {}",
                    rule.name, rule.collection
                ));
                continue;
            }
            rules.push(rule);
        }
        Self { rules, problems }
    }

    /// The first rule that matches `candidate`, which came from the folder `source`.
    #[must_use]
    pub fn first_match(&self, candidate: &Candidate, source: &Path) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|rule| matches(&rule.matches, candidate, source))
    }
}

fn matches(m: &Match, candidate: &Candidate, source: &Path) -> bool {
    let name = candidate
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(extension) = &m.extension {
        let actual = candidate
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        if candidate.kind != Kind::File
            || actual.as_deref() != Some(extension.trim_start_matches('.').to_lowercase().as_str())
        {
            return false;
        }
    }
    if let Some(pattern) = &m.name
        && !regex::Regex::new(pattern).is_ok_and(|re| re.is_match(&name))
    {
        return false;
    }
    if let Some(marker) = &m.marker {
        if candidate.kind == Kind::File {
            return false;
        }
        let pattern = NamePattern(marker.clone());
        let found = std::fs::read_dir(&candidate.path).is_ok_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|e| pattern.matches(&e.file_name().to_string_lossy()))
        });
        if !found {
            return false;
        }
    }
    if let Some(folder) = &m.source {
        let folder = expand_home(folder);
        if !source.starts_with(&folder) {
            return false;
        }
    }
    true
}

fn expand_home(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

/// Where routing decided an item goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The collection, or `None` for Holding when the archive has none.
    pub collection: Option<Id>,
    /// Its path, such as `Projects/Hardware`.
    pub path: String,
    /// How sure routing is.
    pub confidence: Confidence,
    /// Why, for review.
    pub reasons: Vec<String>,
    /// The rule that decided, if one did.
    pub rule: Option<String>,
    /// Whether the rule lets the item skip review.
    pub auto_file: bool,
    /// Every model call made.
    pub attempts: Vec<Attempt>,
}

/// How well the model says an item fits the collection it chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Well,
    Partly,
    Poorly,
}

/// A collection as a model sees it when choosing.
struct Choice<'a> {
    collection: &'a Collection,
    /// "Purpose and scope" and "Not here" from its design document.
    guidance: String,
}

/// The design document's guidance for choosing: its description, "Purpose and scope" and
/// "Not here" sections.
fn guidance(archive: &Archive, collection: &Collection) -> String {
    let mut text = String::new();
    if let Some(description) = &collection.doc.description {
        text.push_str(description);
        text.push('\n');
    }
    let path = archive.root().join(&collection.path).join("_collection.md");
    let Ok(document) = std::fs::read_to_string(path) else {
        return text;
    };
    let body = frontmatter::split(&document).map_or(document.as_str(), |s| s.body);
    for wanted in ["Purpose and scope", "Not here"] {
        if let Some(section) = heading_section(body, wanted) {
            let section = section.trim();
            let cut = &section[..section.floor_char_boundary(SECTION_BUDGET)];
            writeln!(text, "{wanted}: {}", cut.replace('\n', " ")).unwrap();
        }
    }
    text
}

/// The text under `## {heading}`, up to the next heading of that level or a managed section.
fn heading_section<'a>(body: &'a str, heading: &str) -> Option<&'a str> {
    let marker = format!("## {heading}");
    let start = body
        .lines()
        .position(|l| l.trim().eq_ignore_ascii_case(&marker))?;
    let mut offset = 0;
    for (i, line) in body.split_inclusive('\n').enumerate() {
        if i == start {
            offset += line.len();
            break;
        }
        offset += line.len();
    }
    let rest = &body[offset..];
    let end = rest
        .find("\n## ")
        .into_iter()
        .chain(rest.find("<!-- carrel:"))
        .min()
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The collections directly under `parent` (or the top level for `None`), Holding left out.
fn children<'a>(collections: &'a [Collection], parent: Option<&str>) -> Vec<&'a Collection> {
    collections
        .iter()
        .filter(|c| {
            let up = c.path.rsplit_once('/').map(|(p, _)| p);
            up == parent && !(parent.is_none() && c.path == HOLDING_CHOICE)
        })
        .collect()
}

/// Everything routing needs about the archive.
pub struct Router<'a> {
    archive: &'a Archive,
    collections: &'a [Collection],
    rules: &'a RoutingRules,
}

impl<'a> Router<'a> {
    /// A router for `archive`.
    #[must_use]
    pub fn new(
        archive: &'a Archive,
        collections: &'a [Collection],
        rules: &'a RoutingRules,
    ) -> Self {
        Self {
            archive,
            collections,
            rules,
        }
    }

    fn holding(
        &self,
        confidence: Confidence,
        reasons: Vec<String>,
        attempts: Vec<Attempt>,
    ) -> Route {
        let holding = self.collections.iter().find(|c| c.path == HOLDING_CHOICE);
        Route {
            collection: holding.map(|c| c.doc.id),
            path: HOLDING_CHOICE.into(),
            confidence,
            reasons,
            rule: None,
            auto_file: false,
            attempts,
        }
    }

    /// Chooses where `candidate` goes: the first matching rule, or else the model, one level
    /// at a time, asked twice with the choices in opposite orders, each time picking the best
    /// collection and saying how well it fits. Both passes agreeing on a good fit at every level
    /// means medium confidence; a partial fit keeps the collection with low confidence; a poor
    /// fit at the top, or passes that disagree, mean Holding (or the parent, below the top) with
    /// low confidence. Without a model, items go to Holding.
    #[must_use]
    pub fn route(
        &self,
        registry: &Registry,
        candidate: &Candidate,
        source: &Path,
        draft: &Draft,
    ) -> Route {
        if let Some(route) = self.by_rule(candidate, source) {
            return route;
        }
        let Some((provider, offer)) = registry.ranked(Capability::TextModel).into_iter().next()
        else {
            return self.holding(
                Confidence::Low,
                vec!["no text model to choose a collection".into()],
                Vec::new(),
            );
        };

        let mut attempts = Vec::new();
        let mut reasons = Vec::new();
        let mut chosen: Option<&Collection> = None;
        // Whether every level was a good fit, which is what medium confidence needs.
        let mut sure = true;
        loop {
            let parent = chosen.map(|c| c.path.as_str());
            let options: Vec<Choice<'_>> = children(self.collections, parent)
                .into_iter()
                .map(|c| Choice {
                    collection: c,
                    guidance: guidance(self.archive, c),
                })
                .collect();
            if options.is_empty() {
                break;
            }
            let first = Self::ask(
                provider,
                offer,
                &options,
                chosen,
                draft,
                false,
                &mut attempts,
            );
            let second = Self::ask(
                provider,
                offer,
                &options,
                chosen,
                draft,
                true,
                &mut attempts,
            );
            let (Some((pick, fit, reason)), Some((again, fit_again, _))) = (first, second) else {
                // A model failure: stop at the level reached.
                if chosen.is_none() {
                    return self.holding(
                        Confidence::Low,
                        vec!["the model couldn't choose a collection".into()],
                        attempts,
                    );
                }
                break;
            };
            if pick != again {
                if chosen.is_none() {
                    reasons.push(format!("the model wavered between {pick} and {again}"));
                    return self.holding(Confidence::Low, reasons, attempts);
                }
                reasons.push(format!(
                    "left in {} because the model wavered between {pick} and {again}",
                    parent.unwrap_or_default()
                ));
                return Self::done(chosen, Confidence::Low, reasons, attempts);
            }
            if fit == Fit::Poorly || fit_again == Fit::Poorly {
                if chosen.is_none() {
                    reasons.push(format!("fits {pick} poorly: {reason}"));
                    return self.holding(Confidence::Low, reasons, attempts);
                }
                // A poor fit below means the parent itself.
                reasons.push(format!(
                    "no subcollection of {} fits well",
                    parent.unwrap_or_default()
                ));
                break;
            }
            if fit == Fit::Partly || fit_again == Fit::Partly {
                sure = false;
            }
            if pick == HERE_CHOICE {
                reasons.push(reason);
                break;
            }
            let Some(next) = options.iter().find(|o| o.collection.path == pick) else {
                break;
            };
            reasons.push(reason);
            chosen = Some(next.collection);
        }
        let confidence = if chosen.is_some() && sure {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        if chosen.is_none() {
            return self.holding(confidence, reasons, attempts);
        }
        Self::done(chosen, confidence, reasons, attempts)
    }

    /// The route the first matching owner rule gives, if one matches.
    fn by_rule(&self, candidate: &Candidate, source: &Path) -> Option<Route> {
        if let Some(rule) = self.rules.first_match(candidate, source) {
            let collection = self
                .collections
                .iter()
                .find(|c| c.doc.id == rule.collection);
            return Some(Route {
                collection: Some(rule.collection),
                path: collection.map(|c| c.path.clone()).unwrap_or_default(),
                confidence: Confidence::High,
                reasons: vec![format!("routing rule \u{201c}{}\u{201d}", rule.name)],
                rule: Some(rule.name.clone()),
                auto_file: rule.auto_file,
                attempts: Vec::new(),
            });
        }
        None
    }

    fn done(
        chosen: Option<&Collection>,
        confidence: Confidence,
        reasons: Vec<String>,
        attempts: Vec<Attempt>,
    ) -> Route {
        let collection = chosen.expect("a collection was chosen");
        Route {
            collection: Some(collection.doc.id),
            path: collection.path.clone(),
            confidence,
            reasons,
            rule: None,
            auto_file: false,
            attempts,
        }
    }

    /// One question: which of `options` (or, below the top, the parent itself) fits best, and
    /// how well. `reversed` lists them in the opposite order, for the second pass.
    fn ask(
        provider: &dyn Provider,
        offer: &Offer,
        options: &[Choice<'_>],
        parent: Option<&Collection>,
        draft: &Draft,
        reversed: bool,
        attempts: &mut Vec<Attempt>,
    ) -> Option<(String, Fit, String)> {
        let (prompt, names) = routing_prompt(options, parent, draft, reversed);
        let request = CompletionRequest {
            system: Some("You file items in a personal archive. Choose the collection whose purpose best fits the item, respecting each collection's \u{201c}Not here\u{201d} notes, and say honestly how well it fits.".into()),
            prompt,
            schema: Some(json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["choice", "fit", "reason"],
                "properties": {
                    "choice": {"enum": names},
                    "fit": {"enum": ["well", "partly", "poorly"], "description": "How well the item fits your choice"},
                    "reason": {"type": "string", "description": "One short sentence"}
                }
            })),
            max_tokens: Some(120),
        };
        let stage = |result: &str, message: Option<String>| Attempt {
            stage: "route".into(),
            provider: provider.name().to_owned(),
            offer: offer.id.clone(),
            result: result.into(),
            message,
        };
        match provider.complete(&offer.id, &request) {
            Ok(completion) => {
                let choice = completion
                    .json
                    .as_ref()
                    .and_then(|j| j["choice"].as_str())
                    .map(str::to_owned);
                let reason = completion
                    .json
                    .as_ref()
                    .and_then(|j| j["reason"].as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                let fit = match completion.json.as_ref().and_then(|j| j["fit"].as_str()) {
                    Some("well") => Fit::Well,
                    Some("poorly") => Fit::Poorly,
                    _ => Fit::Partly,
                };
                if let Some(choice) = choice.filter(|c| names.contains(&c.as_str())) {
                    attempts.push(stage("ok", None));
                    Some((choice, fit, reason))
                } else {
                    attempts.push(stage(
                        "failed",
                        Some("the answer wasn't one of the choices".into()),
                    ));
                    None
                }
            }
            Err(error) => {
                let result = match error {
                    ProviderError::Unsupported(_) => "unsupported",
                    ProviderError::Unavailable(_) => "unavailable",
                    _ => "failed",
                };
                attempts.push(stage(result, Some(error.to_string())));
                None
            }
        }
    }
}

/// The routing question for one level, and the names the model may answer with.
fn routing_prompt<'a>(
    options: &'a [Choice<'a>],
    parent: Option<&Collection>,
    draft: &Draft,
    reversed: bool,
) -> (String, Vec<&'a str>) {
    // Holding isn't offered: the model picks the best collection and says how well it fits,
    // and a poor fit sends the item to Holding. Below the top, the parent itself always comes
    // last, so reversing the order doesn't move it.
    let mut listed: Vec<&Choice<'_>> = options.iter().collect();
    if reversed {
        listed.reverse();
    }
    let mut names: Vec<&str> = listed.iter().map(|o| o.collection.path.as_str()).collect();
    if parent.is_some() {
        names.push(HERE_CHOICE);
    }
    let mut prompt = String::new();
    writeln!(prompt, "Item: \u{201c}{}\u{201d}", draft.title).unwrap();
    if let Some(summary) = &draft.summary {
        writeln!(prompt, "{summary}").unwrap();
    }
    if !draft.tags.is_empty() {
        let tags: Vec<&str> = draft.tags.iter().map(|t| t.0.as_str()).collect();
        writeln!(prompt, "Tags: {}", tags.join(", ")).unwrap();
    }
    match parent {
        None => prompt.push_str("\nWhich collection should it go in?\n\n"),
        Some(p) => writeln!(prompt, "\nIt belongs in {}. Should it go in one of its subcollections, or in {} itself ({HERE_CHOICE})?\n", p.path, p.path).unwrap(),
    }
    for choice in &listed {
        writeln!(
            prompt,
            "{}: {}\n{}",
            choice.collection.path, choice.collection.doc.title, choice.guidance
        )
        .unwrap();
    }
    if let Some(p) = parent {
        writeln!(
            prompt,
            "{HERE_CHOICE}: {} itself, when no subcollection fits",
            p.path
        )
        .unwrap();
    }
    (prompt, names)
}

/// The tags in `tags` that apply at `path`: those of `ARCHIVE.md` and of every collection from
/// the top down to `path` (§6.5).
#[must_use]
pub fn tags_for(
    archive: &Archive,
    collections: &[Collection],
    path: &str,
    tags: &[Tag],
) -> Vec<Tag> {
    let mut applicable = Vocabulary::default();
    applicable.tags.extend(
        archive
            .doc()
            .tags
            .iter()
            .map(|t| (t.name.clone(), String::new())),
    );
    for c in collections {
        if path == c.path || path.starts_with(&format!("{}/", c.path)) {
            applicable
                .tags
                .extend(c.doc.tags.iter().map(|t| (t.name.clone(), String::new())));
        }
    }
    tags.iter()
        .filter(|t| applicable.contains(&t.0))
        .cloned()
        .collect()
}
