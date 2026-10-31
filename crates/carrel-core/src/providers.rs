//! Providers: where Carrel gets capabilities that depend on the platform (`docs/providers.md`).
//!
//! A [`Provider`] offers text models and text extraction. Native tools are used through
//! providers where they exist (Apple's frameworks through `carrel-provider-apple`), and
//! portable providers fill in elsewhere. A [`Registry`] holds the providers available on this
//! device and, for each request, tries the best [`Offer`] first and falls back to the next.
//!
//! Providers are usually separate programs speaking the provider protocol, which
//! [`process::ProcessProvider`] runs; that isn't available on iOS and similar systems, where the
//! app gives the core in-process providers instead. [`PlainText`] is built in. [`serve::serve`]
//! is the other end: it offers any [`Provider`] written in Rust as a provider program.

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
pub mod process;
pub mod serve;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// Priorities by convention: native tools, portable providers and built-in fallbacks.
pub mod priority {
    /// A platform's own tools, such as Apple's Foundation Models.
    pub const NATIVE: i32 = 100;
    /// Portable providers, such as llama.cpp.
    pub const PORTABLE: i32 = 50;
    /// Carrel's built-in fallbacks.
    pub const BUILT_IN: i32 = 10;
}

/// Something a provider can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Answering prompts with text, or with JSON following a schema.
    TextModel,
    /// Reading the text of PDFs, images and other documents.
    ExtractText,
}

impl Capability {
    /// The name used in the protocol, such as `text-model`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextModel => "text-model",
            Self::ExtractText => "extract-text",
        }
    }
}

/// One thing a provider offers, as reported by `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    /// What it does.
    pub capability: Capability,
    /// Identifies the offer within its provider, for example a model name.
    pub id: String,
    /// A name for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Which offer is preferred when several can do the same thing; higher wins.
    pub priority: i32,
    /// Whether it can be used now.
    pub available: bool,
    /// Why it can't be used, when it isn't available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For text extraction: the media types it reads. Empty means any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,
    /// Other fields, such as a model's context size.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Offer {
    /// Whether this offer reads files of `media_type`.
    #[must_use]
    pub fn reads(&self, media_type: &str) -> bool {
        self.media_types.is_empty()
            || self
                .media_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(media_type))
    }
}

/// A prompt for a text model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Instructions that frame the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// The prompt.
    pub prompt: String,
    /// A JSON Schema the answer must follow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    /// The longest answer wanted, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// A text model's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    /// The answer.
    pub text: String,
    /// The answer as JSON, when a schema was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    /// The exact model and version, for provenance.
    pub model: String,
    /// Token counts, when the provider reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Token counts for one completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens in the prompt.
    pub input_tokens: u64,
    /// Tokens in the answer.
    pub output_tokens: u64,
}

/// A file to read the text of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionRequest {
    /// The file's absolute path. Providers never change it.
    pub path: PathBuf,
    /// Its media type, such as `application/pdf`.
    pub media_type: String,
}

/// The text read from a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    /// The text.
    pub text: String,
    /// `embedded` for text stored in the file, `ocr` for recognized text, `described` when a
    /// model also described an image, `geometry` for a 3D model's size and shape, `rendered`
    /// when a model also described drawings of it.
    pub method: String,
    /// The number of pages, when the provider reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    /// When the file was made, if it says, such as a photo's date taken: a date, or a time
    /// with its offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Whether it records where it was made, such as a photo's location.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub located: bool,
}

/// Why a provider couldn't do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The offer can't be used right now (protocol error code 1).
    Unavailable(String),
    /// The input isn't one the offer handles (code 2).
    Unsupported(String),
    /// The offer tried and couldn't (code 3).
    Failed(String),
    /// The model declined the input, as a safety refusal (code 4). Carrel tries the next
    /// provider, and records the refusal so the owner can see it.
    Declined(String),
    /// The provider didn't follow the protocol, crashed or didn't answer in time.
    Protocol(String),
    /// No provider on this device offers what's needed.
    NoProvider(Capability),
}

impl ProviderError {
    /// The error for a JSON-RPC error `code` and `message`. A provider that answers with an
    /// error is still following the protocol, so none of these is a protocol failure: an
    /// unknown method (`-32601`), as from a provider older than the method, is unsupported.
    #[must_use]
    pub fn from_code(code: i64, message: String) -> Self {
        match code {
            1 => Self::Unavailable(message),
            2 | -32601 => Self::Unsupported(message),
            4 => Self::Declined(message),
            _ => Self::Failed(message),
        }
    }
}

impl ProviderError {
    /// The attempt result it's recorded as (FORMAT.md §7.9): `unsupported`, `unavailable`,
    /// `declined` or `failed`.
    #[must_use]
    pub fn result(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "unsupported",
            Self::Unavailable(_) | Self::NoProvider(_) => "unavailable",
            Self::Declined(_) => "declined",
            Self::Failed(_) | Self::Protocol(_) => "failed",
        }
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Failed(m) => write!(f, "failed: {m}"),
            Self::Declined(m) => write!(f, "declined: {m}"),
            Self::Protocol(m) => write!(f, "provider error: {m}"),
            Self::NoProvider(c) => write!(f, "no provider on this device offers {}", c.as_str()),
        }
    }
}

impl std::error::Error for ProviderError {}

/// What a provider program says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Where the program is.
    pub path: PathBuf,
    /// The name it gives itself, such as "Apple".
    pub label: Option<String>,
    /// Its version.
    pub version: Option<String>,
}

/// A source of capabilities: a provider program, or one built into Carrel or the app.
pub trait Provider: Send + Sync {
    /// The provider's name, as in `carrel-provider-<name>`.
    fn name(&self) -> &str;

    /// For a provider program, where it is and what it says about itself.
    fn program(&self) -> Option<Program> {
        None
    }

    /// What it offers.
    fn offers(&self) -> &[Offer];

    /// Answers `request` with the text model `offer`.
    ///
    /// # Errors
    ///
    /// A [`ProviderError`]; [`ProviderError::Unsupported`] if the provider has no text models.
    fn complete(
        &self,
        offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let _ = (offer, request);
        Err(ProviderError::Unsupported(format!(
            "{} has no text models",
            self.name()
        )))
    }

    /// The number of tokens `text` takes in the text model `offer`, for fitting text into its
    /// context.
    ///
    /// # Errors
    ///
    /// A [`ProviderError`]; [`ProviderError::Unsupported`] if the provider can't count, in which
    /// case [`estimate_tokens`] is the fallback.
    fn count_tokens(&self, offer: &str, text: &str) -> Result<u64, ProviderError> {
        let _ = (offer, text);
        Err(ProviderError::Unsupported(format!(
            "{} doesn't count tokens",
            self.name()
        )))
    }

    /// Reads the text of a file with the extractor `offer`.
    ///
    /// # Errors
    ///
    /// A [`ProviderError`]; [`ProviderError::Unsupported`] if the provider extracts no text.
    fn extract(
        &self,
        offer: &str,
        request: &ExtractionRequest,
    ) -> Result<Extraction, ProviderError> {
        let _ = (offer, request);
        Err(ProviderError::Unsupported(format!(
            "{} doesn't extract text",
            self.name()
        )))
    }
}

/// A conservative estimate of how many tokens `text` takes in any model, for when the model
/// can't say: one token for every three bytes of UTF-8. Real tokenizers average about four
/// bytes of English per token, and about one CJK character, which is three bytes, so this
/// overestimates rather than under.
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(3)
}

/// Which provider and offer produced a result, for provenance (FORMAT.md §5.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// The provider's name.
    pub provider: String,
    /// The offer's identifier.
    pub offer: String,
    /// The offer's priority, so a result from a lower-priority offer can be marked for a
    /// stronger device to redo.
    pub priority: i32,
}

/// One attempt that didn't succeed before a result was found, or before giving up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// The offer tried.
    pub source: Source,
    /// Why it didn't work.
    pub error: ProviderError,
}

/// The providers on this device, and which to try first.
#[derive(Default)]
pub struct Registry {
    providers: Vec<Box<dyn Provider>>,
    preferred: BTreeMap<Capability, (String, String)>,
    pinned: BTreeMap<Capability, (String, String)>,
    second: BTreeMap<Capability, (String, String)>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field(
                "providers",
                &self.providers.iter().map(|p| p.name()).collect::<Vec<_>>(),
            )
            .field("preferred", &self.preferred)
            .field("pinned", &self.pinned)
            .field("second", &self.second)
            .finish()
    }
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry with Carrel's built-in providers.
    #[must_use]
    pub fn with_built_ins() -> Self {
        let mut registry = Self::new();
        registry.add(Box::new(PlainText::new()));
        registry
    }

    /// Adds a provider. Providers added later win ties in priority with earlier ones.
    pub fn add(&mut self, provider: Box<dyn Provider>) {
        self.providers.push(provider);
    }

    /// Tries `provider`'s offer `offer` first for `capability`, whatever its priority, as when
    /// the owner chooses it in settings.
    pub fn prefer(&mut self, capability: Capability, provider: &str, offer: &str) {
        self.preferred
            .insert(capability, (provider.to_owned(), offer.to_owned()));
    }

    /// Uses only `provider`'s offer `offer` for `capability`, as when the benchmark measures
    /// one model, with no other to fall back on but the second opinion, if one is set.
    pub fn pin(&mut self, capability: Capability, provider: &str, offer: &str) {
        self.pinned
            .insert(capability, (provider.to_owned(), offer.to_owned()));
    }

    /// Asks `provider`'s offer `offer` for a second opinion on `capability` when the first
    /// model isn't sure, as routing does for items it would send to Holding. `None` asks no
    /// one.
    pub fn second_opinion(&mut self, capability: Capability, choice: Option<(&str, &str)>) {
        match choice {
            Some((provider, offer)) => {
                self.second
                    .insert(capability, (provider.to_owned(), offer.to_owned()));
            }
            None => {
                self.second.remove(&capability);
            }
        }
    }

    /// The model to ask for a second opinion on `capability`, if one is set, available, and
    /// not `first` itself. It's found among every provider, whatever is pinned: the second
    /// opinion is a separate choice.
    #[must_use]
    pub fn second(
        &self,
        capability: Capability,
        first: (&str, &str),
    ) -> Option<(&dyn Provider, &Offer)> {
        let (name, id) = self.second.get(&capability)?;
        if (name.as_str(), id.as_str()) == first {
            return None;
        }
        self.providers
            .iter()
            .filter(|p| p.name() == name)
            .flat_map(|p| p.offers().iter().map(move |o| (p.as_ref(), o)))
            .find(|(_, o)| o.capability == capability && o.available && &o.id == id)
    }

    /// The providers, in the order they were added.
    pub fn providers(&self) -> impl Iterator<Item = &dyn Provider> {
        self.providers.iter().map(AsRef::as_ref)
    }

    /// The available offers for `capability`, best first: the preferred one, then by
    /// priority, with later-added providers winning ties. Within one provider, offers of equal
    /// priority keep the order the provider listed them in.
    #[must_use]
    pub fn ranked(&self, capability: Capability) -> Vec<(&dyn Provider, &Offer)> {
        let is = |choice: Option<&(String, String)>, p: &dyn Provider, o: &Offer| {
            choice.is_some_and(|(name, id)| name == p.name() && id == &o.id)
        };
        let preferred = self.preferred.get(&capability);
        let pinned = self.pinned.get(&capability);
        let second = self.second.get(&capability);
        let mut offers: Vec<(usize, &dyn Provider, &Offer)> = self
            .providers
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.offers().iter().map(move |o| (i, p.as_ref(), o)))
            .filter(|(_, _, o)| o.capability == capability && o.available)
            // A pinned model keeps its second opinion as its one fallback, as in use.
            .filter(|(_, p, o)| pinned.is_none() || is(pinned, *p, o) || is(second, *p, o))
            .collect();
        offers.sort_by_key(|(i, p, o)| {
            (
                std::cmp::Reverse(is(pinned, *p, o)),
                std::cmp::Reverse(is(preferred, *p, o)),
                std::cmp::Reverse(o.priority),
                std::cmp::Reverse(*i),
            )
        });
        offers.into_iter().map(|(_, p, o)| (p, o)).collect()
    }

    /// The offer that would be tried first for `capability`.
    #[must_use]
    pub fn best(&self, capability: Capability) -> Option<Source> {
        self.ranked(capability).first().map(|(p, o)| source(*p, o))
    }

    /// Answers `request` with the best text model that succeeds, trying the next when one
    /// fails. Returns the answer, where it came from, and the attempts that failed first.
    ///
    /// # Errors
    ///
    /// The last provider's error if none succeeds, or [`ProviderError::NoProvider`] if there
    /// is no text model.
    pub fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<(Completion, Source, Vec<Attempt>), ProviderError> {
        let candidates = self.ranked(Capability::TextModel);
        try_in_turn(Capability::TextModel, &candidates, |p, o| {
            p.complete(&o.id, request)
        })
    }

    /// How many tokens `text` takes in the model `source` names: the model's own count when it
    /// can give one, otherwise [`estimate_tokens`]. Returns the count and whether it's exact.
    #[must_use]
    pub fn count_tokens(&self, source: &Source, text: &str) -> (u64, bool) {
        self.providers
            .iter()
            .find(|p| p.name() == source.provider)
            .and_then(|p| p.count_tokens(&source.offer, text).ok())
            .map_or_else(|| (estimate_tokens(text), false), |n| (n, true))
    }

    /// Reads a file's text with the best extractor for its media type that succeeds, trying
    /// the next when one fails.
    ///
    /// # Errors
    ///
    /// As for [`Registry::complete`].
    pub fn extract(
        &self,
        request: &ExtractionRequest,
    ) -> Result<(Extraction, Source, Vec<Attempt>), ProviderError> {
        let candidates: Vec<_> = self
            .ranked(Capability::ExtractText)
            .into_iter()
            .filter(|(_, o)| o.reads(&request.media_type))
            .collect();
        try_in_turn(Capability::ExtractText, &candidates, |p, o| {
            p.extract(&o.id, request)
        })
    }
}

fn source(provider: &dyn Provider, offer: &Offer) -> Source {
    Source {
        provider: provider.name().to_owned(),
        offer: offer.id.clone(),
        priority: offer.priority,
    }
}

fn try_in_turn<T>(
    capability: Capability,
    candidates: &[(&dyn Provider, &Offer)],
    call: impl Fn(&dyn Provider, &Offer) -> Result<T, ProviderError>,
) -> Result<(T, Source, Vec<Attempt>), ProviderError> {
    let mut attempts = Vec::new();
    for (provider, offer) in candidates {
        match call(*provider, offer) {
            Ok(result) => return Ok((result, source(*provider, offer), attempts)),
            Err(error) => attempts.push(Attempt {
                source: source(*provider, offer),
                error,
            }),
        }
    }
    Err(attempts
        .pop()
        .map_or(ProviderError::NoProvider(capability), |a| a.error))
}

/// The built-in extractor for plain text and Markdown.
#[derive(Debug, Clone)]
pub struct PlainText {
    offers: Vec<Offer>,
}

impl PlainText {
    /// The plain-text extractor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offers: vec![Offer {
                capability: Capability::ExtractText,
                id: "plain-text".into(),
                label: Some("Plain text".into()),
                priority: priority::BUILT_IN,
                available: true,
                reason: None,
                media_types: ["text/plain", "text/markdown", "text/csv", "text/x-tex"]
                    .map(str::to_owned)
                    .to_vec(),
                extra: serde_json::Map::new(),
            }],
        }
    }
}

impl Default for PlainText {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for PlainText {
    fn name(&self) -> &'static str {
        "built-in"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn extract(
        &self,
        _offer: &str,
        request: &ExtractionRequest,
    ) -> Result<Extraction, ProviderError> {
        let bytes = std::fs::read(&request.path)
            .map_err(|e| ProviderError::Failed(format!("{}: {e}", request.path.display())))?;
        Ok(Extraction {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            method: "embedded".into(),
            pages: None,
            created: None,
            located: false,
        })
    }
}
