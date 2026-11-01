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
    /// Room for each step's reply naming a tool, in tokens.
    pub answer_tokens: u32,
    /// Room for the final answer, with its items, passages and suggestions, in tokens; less
    /// when the model's context is small.
    pub final_tokens: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps: 8,
            tokens: 40_000,
            answer_tokens: 900,
            final_tokens: 2_400,
        }
    }
}

/// The context assumed for a model that doesn't say, in tokens.
const DEFAULT_CONTEXT: u64 = 4_096;

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
    progress: Option<&'a dyn Fn(Progress<'_>)>,
}

/// How far the librarian has got with an answer, for an interface to show while it works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'p> {
    /// The step it's starting, from 1.
    pub step: usize,
    /// The most steps it may take.
    pub steps: usize,
    /// What its last step did, in a few words; `None` before the first.
    pub did: Option<&'p str>,
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
            progress: None,
        })
    }

    /// With other limits.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Tells `progress` as each step starts, so a slow model can be watched.
    #[must_use]
    pub fn with_progress(mut self, progress: &'a dyn Fn(Progress<'_>)) -> Self {
        self.progress = Some(progress);
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
    ///
    /// A tool call it already made isn't run again or counted as a step: it's told the call
    /// was already done, with the earlier result (#356). Past as many repeats as it has
    /// steps, a repeat costs a step, so it can't go round for ever.
    fn steps(
        &self,
        stacks: &Shelf<'_>,
        setting: &prompts::Setting,
        earlier: &[(String, String)],
        question: &str,
        work: &mut Work,
    ) -> Result<Option<Reply>, LibrarianError> {
        let system = prompts::system(self.rights.is_some());
        let asking = Asking {
            system: &system,
            setting,
            earlier,
            question,
            context: self.context(),
        };
        let mut step = 0;
        let mut repeats = 0;
        while step < self.limits.steps {
            let left = self.limits.steps - step;
            if let Some(progress) = self.progress {
                progress(Progress {
                    step: step + 1,
                    steps: self.limits.steps,
                    did: work.steps.last().map(|s| s.did.as_str()),
                });
            }
            let Some(reply) = self.ask_model(&asking, work, left)? else {
                work.stopped = Some(Stopped::Tokens);
                return Ok(None);
            };
            match reply {
                Some(reply) if reply.action == "answer" && !reply.answer.trim().is_empty() => {
                    return Ok(Some(reply));
                }
                Some(reply) if prompts::TOOLS.iter().any(|(n, _)| *n == reply.action) => {
                    if work.repeat(&reply.action, &reply.input) {
                        repeats += 1;
                        if repeats <= self.limits.steps {
                            continue;
                        }
                    } else {
                        let ran = stacks.run(&reply.action, &reply.input, question);
                        work.record(&reply.action, &reply.input, ran);
                    }
                }
                _ => work.done.push(
                    "Your answer wasn't usable: answer with the JSON form, naming a tool and its input, or the action answer with your answer.".into(),
                ),
            }
            step += 1;
        }
        work.stopped = Some(Stopped::Steps);
        Ok(None)
    }

    /// Asks the model for one step, with what's been done fitted to its context. `None` when
    /// the step wouldn't fit in what's left of the tokens; otherwise its reply, if usable.
    ///
    /// A final answer gets more room than a step naming a tool. When the model runs out of
    /// room anyway, as when its JSON is cut off, the step is asked again once with room for
    /// a whole answer; when its context overflows, it's asked again once with what's been
    /// done shortened further.
    fn ask_model(
        &self,
        asking: &Asking<'_>,
        work: &mut Work,
        left: usize,
    ) -> Result<Option<Option<Reply>>, LibrarianError> {
        let answer_room = self.answer_room(asking.context);
        let fixed = estimate_tokens(asking.system)
            + estimate_tokens(&prompts::step(
                asking.setting,
                asking.earlier,
                asking.question,
                &[],
                left,
            ));
        let room = |answer: u32, share: u64| {
            asking
                .context
                .saturating_sub(fixed + u64::from(answer) + 64)
                / share
        };
        let done = fit_done(&work.done, room(answer_room, 1));
        let prompt = prompts::step(asking.setting, asking.earlier, asking.question, &done, left);
        let needed = estimate_tokens(asking.system) + estimate_tokens(&prompt);
        if work.tokens + needed > self.limits.tokens {
            return Ok(None);
        }
        let must_answer = left == 1
            || work.tokens
                + 2 * needed
                + u64::from(self.limits.answer_tokens)
                + u64::from(answer_room)
                > self.limits.tokens;
        let nearly = if must_answer && left > 1 {
            "\nYou're nearly out of room: answer now, with what you have.\n"
        } else {
            ""
        };
        let mut request = CompletionRequest {
            system: Some(asking.system.to_owned()),
            prompt: prompt + nearly,
            schema: Some(prompts::schema(must_answer)),
            max_tokens: Some(if must_answer {
                answer_room
            } else {
                self.limits.answer_tokens
            }),
        };
        let completion = match self.complete(&request) {
            Err(LibrarianError::Model(ProviderError::Failed(_)))
                if request.max_tokens < Some(answer_room) =>
            {
                work.tokens += needed;
                request.max_tokens = Some(answer_room);
                self.complete(&request)?
            }
            Err(LibrarianError::Model(ProviderError::Unsupported(_))) => {
                work.tokens += needed;
                let done = fit_done(&work.done, room(answer_room, 2));
                request.prompt =
                    prompts::step(asking.setting, asking.earlier, asking.question, &done, left)
                        + nearly;
                self.complete(&request)?
            }
            other => other?,
        };
        work.tokens += completion.usage.as_ref().map_or_else(
            || needed + estimate_tokens(&completion.text),
            |u| u.input_tokens + u.output_tokens,
        );
        Ok(Some(
            completion
                .json
                .and_then(|j| serde_json::from_value::<Reply>(j).ok()),
        ))
    }

    /// The model's context, in tokens, as its offer says.
    fn context(&self) -> u64 {
        self.registry
            .ranked(Capability::TextModel)
            .iter()
            .find(|(p, o)| p.name() == self.model.provider && o.id == self.model.offer)
            .and_then(|(_, o)| o.extra.get("context_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_CONTEXT)
    }

    /// Room for the final answer in a model with `context` tokens: [`Limits::final_tokens`],
    /// up to a third of its context, and never less than a step's.
    fn answer_room(&self, context: u64) -> u32 {
        let third = u32::try_from(context / 3).unwrap_or(u32::MAX);
        self.limits
            .final_tokens
            .min(third)
            .max(self.limits.answer_tokens)
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

/// What each step's prompt is made from, besides what's been done.
struct Asking<'s> {
    system: &'s str,
    setting: &'s prompts::Setting,
    earlier: &'s [(String, String)],
    question: &'s str,
    /// The model's context, in tokens.
    context: u64,
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
    /// Each tool call made, by [`call_key`], with its result.
    results: std::collections::HashMap<String, String>,
    tokens: u64,
    stopped: Option<Stopped>,
}

impl Work {
    /// When `tool` has already been run with `input`, tells the model so, with the earlier
    /// result, and says so.
    fn repeat(&mut self, tool: &str, input: &str) -> bool {
        let Some(result) = self.results.get(&call_key(tool, input)) else {
            return false;
        };
        self.done.push(format!(
            "{tool} {}\nAlready done, so it wasn't run again. Its result was:\n{result}\nUse it, or try another tool or other words.",
            input.trim()
        ));
        true
    }

    fn record(&mut self, tool: &str, input: &str, ran: Ran) {
        self.results
            .insert(call_key(tool, input), ran.text.trim_end().to_owned());
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

/// A tool call, as far as telling a repeat: the tool, and its input lowercased with its white
/// space run together.
fn call_key(tool: &str, input: &str) -> String {
    let words: Vec<String> = input.split_whitespace().map(str::to_lowercase).collect();
    format!("{tool} {}", words.join(" "))
}

/// `done` shortened to take at most `room` tokens, so the prompt fits the model's context
/// (#356): the oldest steps' results are left out first, then the newest is cut short.
fn fit_done(done: &[String], room: u64) -> Vec<String> {
    let size = |steps: &[String]| -> u64 { steps.iter().map(|s| estimate_tokens(s) + 4).sum() };
    let mut fitted = done.to_vec();
    let newest = fitted.len().saturating_sub(1);
    for i in 0..newest {
        if size(&fitted) <= room {
            break;
        }
        let call = fitted[i].lines().next().unwrap_or_default().to_owned();
        fitted[i] = format!("{call}\n(Its result is left out, to fit.)");
    }
    let before: u64 = size(&fitted[..newest.min(fitted.len())]);
    if let Some(last) = fitted.last_mut()
        && before + estimate_tokens(last) + 4 > room
    {
        // `estimate_tokens` counts three bytes a token.
        let mut keep = usize::try_from(room.saturating_sub(before + 12) * 3).unwrap_or(0);
        while !last.is_char_boundary(keep.min(last.len())) {
            keep -= 1;
        }
        last.truncate(keep.min(last.len()));
        last.push_str("\n… (cut short, to fit)");
    }
    fitted
}

/// `text` lowercased with its white space and `…` left out, for comparing quotes.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '…')
        .flat_map(char::to_lowercase)
        .collect()
}
