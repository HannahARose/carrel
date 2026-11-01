//! The librarian (`docs/design/librarian.md`): a conversation over the archive on a local
//! text model, through a fixed set of read-only tools.
//!
//! The owner's question goes in. Each step, the model answers in a fixed JSON form
//! ([`prompts::schema`]): either a tool to run ([`tools`]: `search`, `show`, `related`,
//! `read`, `history`, `collection`) or its final answer, with the items it found and the
//! passages it used. The core runs each tool and asks again with the result, up to a limit of
//! steps and tokens per answer ([`Limits`]). The answer names the model that gave it.
//!
//! Restricted items are found by their card only: the tools never read or return their text,
//! and a passage quoted from one is dropped. Passages are kept only when they're from what the
//! tools returned.
//!
//! Along with its answer, the librarian may suggest changes to the catalog ([`changes`]). By
//! default each is left for the owner to confirm; with `librarian.changes` on, it makes those
//! it's sure of itself, each an undoable history change marked as its own.
//!
//! Conversations are kept in memory ([`Conversation`]), and written down only when the owner
//! saves one to the Journal ([`journal`]).

pub mod changes;
pub mod journal;
pub mod model;
pub mod prompts;
pub mod tools;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Id, Sensitivity};
use crate::providers::device::AvailableModel;
use crate::providers::{Capability, CompletionRequest, ProviderError, Registry, estimate_tokens};

use changes::{Rights, Suggested, Suggestion};
use tools::{Ran, Shelf};

/// How far the librarian may go for one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How many times it may ask the model, its answer included.
    pub steps: usize,
    /// How many tokens its prompts and answers may take in all.
    pub tokens: u64,
    /// Room for each of the model's answers, in tokens.
    pub answer_tokens: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps: 8,
            tokens: 40_000,
            answer_tokens: 900,
        }
    }
}

/// An item, as an answer links to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemLink {
    /// Its identifier.
    pub id: Id,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// The collection it's in, by path.
    pub collection: String,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
}

/// A passage the librarian used, from an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quote {
    /// The item.
    pub item: ItemLink,
    /// The passage, word for word.
    pub text: String,
}

/// One step towards an answer: a tool the librarian used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The tool.
    pub tool: String,
    /// What it was given.
    pub input: String,
    /// What it did, in a few words.
    pub did: String,
}

/// Why the librarian stopped before it answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stopped {
    /// It used every step.
    Steps,
    /// It used its tokens.
    Tokens,
}

/// The librarian's answer to one question.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    /// The question.
    pub question: String,
    /// The answer, then anything it suggests looking at.
    pub text: String,
    /// The items it's about.
    pub items: Vec<ItemLink>,
    /// The passages it used.
    pub passages: Vec<Quote>,
    /// Changes it suggested, and what became of each.
    pub suggestions: Vec<Suggestion>,
    /// The tools it used, in order.
    pub steps: Vec<Step>,
    /// The model that gave it, for people.
    pub model: String,
    /// The tokens its prompts and answers took.
    pub tokens: u64,
    /// Why it stopped before answering, when it did.
    pub stopped: Option<Stopped>,
}

/// A conversation: the answers so far, oldest first. Kept in memory only.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Conversation {
    /// The answers.
    pub answers: Vec<Answer>,
}

/// Why the librarian couldn't answer.
#[derive(Debug)]
pub enum LibrarianError {
    /// There's no text model, or the chosen one isn't available.
    NoModel(String),
    /// The model failed.
    Model(ProviderError),
    /// The archive couldn't be read.
    Archive(ArchiveError),
}

impl std::fmt::Display for LibrarianError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoModel(why) => f.write_str(why),
            Self::Model(e) => write!(f, "the librarian's model failed: {e}"),
            Self::Archive(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LibrarianError {}

impl From<ArchiveError> for LibrarianError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

/// What the model answers each step, following [`prompts::schema`].
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reply {
    action: String,
    input: String,
    answer: String,
    items: Vec<String>,
    passages: Vec<ReplyPassage>,
    suggestions: Vec<Suggested>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReplyPassage {
    item: String,
    text: String,
}

/// The librarian, for one archive, on one model.
pub struct Librarian<'a> {
    archive: &'a Archive,
    registry: &'a Registry,
    model: AvailableModel,
    providers: &'a dyn Fn() -> Registry,
    limits: Limits,
    rights: Option<Rights>,
}

impl<'a> Librarian<'a> {
    /// The librarian for `archive`, running on `model` from `registry`. `providers` starts the
    /// providers search embeds queries with, when it needs them.
    ///
    /// # Errors
    ///
    /// [`LibrarianError::NoModel`] if `registry` doesn't offer `model`.
    pub fn new(
        archive: &'a Archive,
        registry: &'a Registry,
        model: AvailableModel,
        providers: &'a dyn Fn() -> Registry,
    ) -> Result<Self, LibrarianError> {
        let offered = registry
            .ranked(Capability::TextModel)
            .iter()
            .any(|(p, o)| p.name() == model.provider && o.id == model.offer);
        if !offered {
            return Err(LibrarianError::NoModel(format!(
                "{} isn't available on this device",
                model.label
            )));
        }
        Ok(Self {
            archive,
            registry,
            model,
            providers,
            limits: Limits::default(),
            rights: None,
        })
    }

    /// With other limits.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Lets it make the changes it's sure of on its own, for `holder`, as the archive's
    /// `librarian.changes` and `librarian.confidence` say. With `librarian.changes` off, it
    /// still only suggests.
    #[must_use]
    pub fn with_changes_for(mut self, holder: crate::coordinator::Holder) -> Self {
        let settings = &self.archive.settings().librarian;
        self.rights = settings.changes.then_some(Rights {
            holder,
            confidence: settings.confidence,
        });
        self
    }

    /// The model it runs on.
    #[must_use]
    pub fn model(&self) -> &AvailableModel {
        &self.model
    }

    /// Whether it may make changes on its own.
    #[must_use]
    pub fn makes_changes(&self) -> bool {
        self.rights.is_some()
    }

    /// Answers `question`, adding the answer to `conversation`.
    ///
    /// # Errors
    ///
    /// [`LibrarianError`] if the archive can't be read or the model fails.
    pub fn ask<'c>(
        &self,
        conversation: &'c mut Conversation,
        question: &str,
    ) -> Result<&'c Answer, LibrarianError> {
        let stacks = Shelf {
            archive: self.archive,
            items: self.archive.items()?,
            providers: self.providers,
        };
        let setting = self.setting(&stacks)?;
        let earlier: Vec<(String, String)> = conversation
            .answers
            .iter()
            .rev()
            .take(4)
            .rev()
            .map(|a| (a.question.clone(), a.text.clone()))
            .collect();
        let mut work = Work::default();
        let reply = self.steps(&stacks, &setting, &earlier, question, &mut work)?;
        let answer = self.finish(&stacks, question, reply, work);
        conversation.answers.push(answer);
        Ok(&conversation.answers[conversation.answers.len() - 1])
    }

    fn setting(&self, stacks: &Shelf<'_>) -> Result<prompts::Setting, LibrarianError> {
        let collections = self.archive.collections()?;
        Ok(prompts::Setting {
            today: jiff::Zoned::now().strftime("%Y-%m-%d (%A)").to_string(),
            archive: self.archive.doc().name.clone(),
            items: stacks.items.len(),
            collections: collections
                .iter()
                .map(|c| {
                    let purpose = c
                        .doc
                        .description
                        .clone()
                        .unwrap_or_else(|| c.doc.title.clone());
                    (c.path.clone(), purpose)
                })
                .collect(),
        })
    }

    /// Asks the model, step by step, running the tools it asks for, until it answers or a
    /// limit is reached. Returns its answer, if it gave one.
    fn steps(
        &self,
        stacks: &Shelf<'_>,
        setting: &prompts::Setting,
        earlier: &[(String, String)],
        question: &str,
        work: &mut Work,
    ) -> Result<Option<Reply>, LibrarianError> {
        let system = prompts::system(self.rights.is_some());
        for step in 0..self.limits.steps {
            let left = self.limits.steps - step;
            let prompt = prompts::step(setting, earlier, question, &work.done, left);
            let needed = estimate_tokens(&system) + estimate_tokens(&prompt);
            if work.tokens + needed > self.limits.tokens {
                work.stopped = Some(Stopped::Tokens);
                return Ok(None);
            }
            let must_answer = left == 1
                || work.tokens + 2 * needed + u64::from(self.limits.answer_tokens) * 2
                    > self.limits.tokens;
            let prompt = if must_answer && left > 1 {
                prompt + "\nYou're nearly out of room: answer now, with what you have.\n"
            } else {
                prompt
            };
            let request = CompletionRequest {
                system: Some(system.clone()),
                prompt,
                schema: Some(prompts::schema(must_answer)),
                max_tokens: Some(self.limits.answer_tokens),
            };
            let completion = self.complete(&request)?;
            work.tokens += completion.usage.as_ref().map_or_else(
                || needed + estimate_tokens(&completion.text),
                |u| u.input_tokens + u.output_tokens,
            );
            let reply = completion
                .json
                .and_then(|j| serde_json::from_value::<Reply>(j).ok());
            match reply {
                Some(reply) if reply.action == "answer" && !reply.answer.trim().is_empty() => {
                    return Ok(Some(reply));
                }
                Some(reply) if prompts::TOOLS.iter().any(|(n, _)| *n == reply.action) => {
                    let ran = stacks.run(&reply.action, &reply.input, question);
                    work.record(&reply.action, &reply.input, ran);
                }
                _ => work.done.push(
                    "Your answer wasn't usable: answer with the JSON form, naming a tool and its input, or the action answer with your answer.".into(),
                ),
            }
        }
        work.stopped = Some(Stopped::Steps);
        Ok(None)
    }

    fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<crate::providers::Completion, LibrarianError> {
        let ranked = self.registry.ranked(Capability::TextModel);
        let (provider, offer) = ranked
            .iter()
            .find(|(p, o)| p.name() == self.model.provider && o.id == self.model.offer)
            .ok_or_else(|| {
                LibrarianError::NoModel(format!("{} isn't available any more", self.model.label))
            })?;
        provider
            .complete(&offer.id, request)
            .map_err(LibrarianError::Model)
    }

    /// The answer from the model's `reply`, checked: items it names that exist, passages that
    /// are from what the tools returned and not from restricted items, and its suggestions
    /// considered. Without a reply, it says it ran out and lists what it came across.
    fn finish(
        &self,
        stacks: &Shelf<'_>,
        question: &str,
        reply: Option<Reply>,
        mut work: Work,
    ) -> Answer {
        let mut answer = Answer {
            question: question.trim().to_owned(),
            text: String::new(),
            items: Vec::new(),
            passages: Vec::new(),
            suggestions: Vec::new(),
            steps: std::mem::take(&mut work.steps),
            model: self.model.label.clone(),
            tokens: work.tokens,
            stopped: work.stopped,
        };
        let Some(reply) = reply else {
            match work.stopped {
                Some(Stopped::Tokens) => "I ran out of room before I could finish an answer.",
                _ => "I couldn't finish an answer within my steps.",
            }
            .clone_into(&mut answer.text);
            if !work.seen.is_empty() {
                answer
                    .text
                    .push_str(" These are the items I came across, which may help:");
            }
            answer.items = work.seen;
            return answer;
        };
        reply.answer.trim().clone_into(&mut answer.text);
        let mut listed = HashSet::new();
        for name in &reply.items {
            if let Ok(item) = stacks.resolve(name)
                && listed.insert(item.id)
            {
                answer.items.push(stacks.link(item));
            }
        }
        for passage in &reply.passages {
            let Ok(item) = stacks.resolve(&passage.item) else {
                continue;
            };
            let link = stacks.link(item);
            if link.sensitivity == Sensitivity::Restricted || !work.has_read(item.id, &passage.text)
            {
                continue;
            }
            answer.passages.push(Quote {
                item: link,
                text: passage.text.trim().to_owned(),
            });
        }
        let model = format!("{}/{}", self.model.provider, self.model.offer);
        answer.suggestions = reply
            .suggestions
            .iter()
            .filter_map(|s| {
                changes::consider(stacks, self.registry, &model, s, self.rights.as_ref())
            })
            .collect();
        answer
    }
}

/// What's been done towards one answer.
#[derive(Debug, Default)]
struct Work {
    /// Each step, for the model.
    done: Vec<String>,
    /// Each tool used, for people.
    steps: Vec<Step>,
    /// The items come across, in order.
    seen: Vec<ItemLink>,
    /// The text the tools gave from items.
    read: Vec<(Id, String)>,
    tokens: u64,
    stopped: Option<Stopped>,
}

impl Work {
    fn record(&mut self, tool: &str, input: &str, ran: Ran) {
        self.done.push(format!(
            "{tool} {}\nResult:\n{}",
            input.trim(),
            ran.text.trim_end()
        ));
        self.steps.push(Step {
            tool: tool.to_owned(),
            input: input.trim().to_owned(),
            did: ran.summary,
        });
        for link in ran.found {
            if !self.seen.iter().any(|s| s.id == link.id) {
                self.seen.push(link);
            }
        }
        self.read.extend(ran.read);
    }

    /// Whether `text` is in what the tools gave from the item `id`, ignoring spacing and
    /// marks where a passage was cut.
    fn has_read(&self, id: Id, text: &str) -> bool {
        let wanted = squash(text);
        !wanted.is_empty()
            && self
                .read
                .iter()
                .any(|(item, read)| *item == id && squash(read).contains(&wanted))
    }
}

/// `text` lowercased with its white space and `…` left out, for comparing quotes.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '…')
        .flat_map(char::to_lowercase)
        .collect()
}
