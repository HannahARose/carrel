//! Models through llama.cpp, as a [`Provider`].

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, priority,
};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde_json::Value;

/// The context size used when `CARREL_LLAMA_CONTEXT` doesn't set one; most models support
/// at least this, and it keeps memory use moderate.
const DEFAULT_CONTEXT: u32 = 8192;

/// The longest answer when a request doesn't set one, in tokens.
const DEFAULT_MAX_TOKENS: u32 = 1024;

/// A fixed seed, so the same prompt gives the same answer.
const SEED: u32 = 0x00ca_7e11;

/// llama.cpp's backend, which can be started once per process.
fn backend() -> Option<&'static LlamaBackend> {
    static BACKEND: OnceLock<Option<LlamaBackend>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            // llama.cpp is talkative. Only its warnings and errors go to the log, which Carrel
            // shows when a provider fails.
            let _ = tracing_subscriber::fmt()
                .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .without_time()
                .try_init();
            llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default());
            LlamaBackend::init().ok()
        })
        .as_ref()
}

/// Local GGUF models, loaded when first asked and kept loaded until another is asked for.
pub struct LlamaProvider {
    backend: Option<&'static LlamaBackend>,
    models: Vec<(String, PathBuf)>,
    offers: Vec<Offer>,
    context: u32,
    loaded: Mutex<Option<(String, LlamaModel)>>,
}

impl LlamaProvider {
    /// Offers every `.gguf` file in `models_dir`, or explains how to add one. The context size
    /// is `CARREL_LLAMA_CONTEXT`, or 8,192 tokens.
    pub fn new(models_dir: Option<&Path>) -> Self {
        let context = std::env::var("CARREL_LLAMA_CONTEXT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_CONTEXT);
        Self::with_context(models_dir, context)
    }

    /// As [`LlamaProvider::new`], with a context of `context` tokens.
    pub fn with_context(models_dir: Option<&Path>, context: u32) -> Self {
        let backend = backend();
        let models = models_dir.map(find_models).unwrap_or_default();
        let mut offers: Vec<Offer> = models
            .iter()
            .map(|(id, path)| {
                let mut offer = model_offer(id, context);
                offer.label = path.file_name().map(|n| n.to_string_lossy().into_owned());
                offer
            })
            .collect();
        if offers.is_empty() {
            let mut offer = model_offer("none", context);
            offer.available = false;
            offer.reason = Some(match models_dir {
                Some(dir) => format!(
                    "no model yet: download one with `carrel models`, or put a .gguf file in {}",
                    dir.display()
                ),
                None => "no models folder: set CARREL_MODELS_DIR".to_owned(),
            });
            offers.push(offer);
        }
        if backend.is_none() {
            for offer in &mut offers {
                offer.available = false;
                offer.reason = Some("llama.cpp couldn't start".into());
            }
        }
        Self {
            backend,
            models,
            offers,
            context,
            loaded: Mutex::new(None),
        }
    }
}

fn model_offer(id: &str, context: u32) -> Offer {
    let mut extra = serde_json::Map::new();
    extra.insert("context_tokens".into(), Value::from(context));
    extra.insert("structured_output".into(), Value::Bool(true));
    Offer {
        capability: Capability::TextModel,
        id: id.to_owned(),
        label: None,
        priority: priority::PORTABLE,
        available: true,
        reason: None,
        media_types: Vec::new(),
        extra,
    }
}

/// The `.gguf` files in `dir`, by name, identified by their file stem.
fn find_models(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut models: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        })
        .filter_map(|p| Some((p.file_stem()?.to_str()?.to_owned(), p)))
        .collect();
    models.sort();
    models
}

fn failed(what: &str) -> impl FnOnce(String) -> ProviderError + '_ {
    move |e| ProviderError::Failed(format!("{what}: {e}"))
}

impl Provider for LlamaProvider {
    fn name(&self) -> &'static str {
        "llama"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn complete(
        &self,
        offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        self.with_model(offer, |backend, model| {
            generate(backend, model, offer, self.context, request)
        })
    }

    fn count_tokens(&self, offer: &str, text: &str) -> Result<u64, ProviderError> {
        self.with_model(offer, |_, model| {
            Ok(model.vocab().tokenize(text.as_bytes(), false, true).len() as u64)
        })
    }
}

impl LlamaProvider {
    /// Runs `f` with the model `offer`, loading it first unless it's the one already loaded.
    fn with_model<T>(
        &self,
        offer: &str,
        f: impl FnOnce(&LlamaBackend, &LlamaModel) -> Result<T, ProviderError>,
    ) -> Result<T, ProviderError> {
        let backend = self
            .backend
            .ok_or_else(|| ProviderError::Unavailable("llama.cpp couldn't start".into()))?;
        let (_, path) = self
            .models
            .iter()
            .find(|(id, _)| id == offer)
            .ok_or_else(|| ProviderError::Unsupported(format!("no model {offer}")))?;
        let mut loaded = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if loaded.as_ref().is_none_or(|(id, _)| id != offer) {
            // Free the old model before loading the next.
            *loaded = None;
            let params = LlamaModelParams::default().with_n_gpu_layers(u32::MAX);
            let model = LlamaModel::load_from_file(backend, path, &params).map_err(|e| {
                ProviderError::Failed(format!("can't load {}: {e}", path.display()))
            })?;
            *loaded = Some((offer.to_owned(), model));
        }
        let (_, model) = loaded.as_ref().expect("loaded above");
        f(backend, model)
    }
}

/// The prompt in the model's own chat format, or plain text if it has none.
fn format_prompt(model: &LlamaModel, request: &CompletionRequest) -> Result<String, ProviderError> {
    let Ok(template) = model.chat_template(None) else {
        let mut text = request
            .system
            .clone()
            .map(|s| s + "\n\n")
            .unwrap_or_default();
        text.push_str(&request.prompt);
        text.push('\n');
        return Ok(text);
    };
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(
            LlamaChatMessage::new("system".into(), system.clone())
                .map_err(|e| failed("system message")(e.to_string()))?,
        );
    }
    messages.push(
        LlamaChatMessage::new("user".into(), request.prompt.clone())
            .map_err(|e| failed("prompt")(e.to_string()))?,
    );
    match model.apply_chat_template(&template, &messages, true) {
        Ok(prompt) => Ok(prompt),
        // llama.cpp only knows the template formats it was built with; a newer model's
        // template is rendered as the Jinja it is.
        Err(builtin) => {
            let source = template
                .to_str()
                .map_err(|e| failed("chat template")(e.to_string()))?;
            let vocab = model.vocab();
            let eos = String::from_utf8_lossy(&vocab.token_to_piece(vocab.eos(), true, None))
                .into_owned();
            render_template(source, request, &eos)
                .map_err(|jinja| failed("chat template")(format!("{builtin}; as Jinja: {jinja}")))
        }
    }
}

/// Renders a model's Jinja chat template for `request`, as Hugging Face's tokenizers do. The
/// beginning-of-sequence token is left out because tokenizing adds it.
fn render_template(source: &str, request: &CompletionRequest, eos: &str) -> Result<String, String> {
    let mut env = minijinja::Environment::new();
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function(
        "raise_exception",
        |message: String| -> Result<String, minijinja::Error> {
            Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                message,
            ))
        },
    );
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(minijinja::context! { role => "system", content => system });
    }
    messages.push(minijinja::context! { role => "user", content => &request.prompt });
    env.render_str(
        source,
        minijinja::context! {
            messages => messages,
            add_generation_prompt => true,
            bos_token => "",
            eos_token => eos,
            enable_thinking => false,
        },
    )
    .map_err(|e| e.to_string())
}

fn generate(
    backend: &LlamaBackend,
    model: &LlamaModel,
    offer: &str,
    context: u32,
    request: &CompletionRequest,
) -> Result<Completion, ProviderError> {
    let vocab = model.vocab();
    let prompt = format_prompt(model, request)?;
    let tokens = vocab.tokenize(prompt.as_bytes(), true, true);
    let max_tokens = request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    let needed = u32::try_from(tokens.len())
        .unwrap_or(u32::MAX)
        .saturating_add(max_tokens);
    if needed > context {
        return Err(ProviderError::Unsupported(format!(
            "too long for {offer}: {} prompt tokens plus {max_tokens} for the answer exceed its context of {context}",
            tokens.len()
        )));
    }

    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(context))
        .with_n_batch(context);
    let mut ctx = model
        .new_context(backend, params)
        .map_err(|e| failed("context")(e.to_string()))?;
    let mut sampler = match &request.schema {
        Some(schema) => {
            let grammar =
                llama_cpp_2::json_schema_to_grammar(&schema.to_string()).map_err(|e| {
                    ProviderError::Unsupported(format!("can't turn the schema into a grammar: {e}"))
                })?;
            let grammar = LlamaSampler::grammar(model, &grammar, "root")
                .map_err(|e| ProviderError::Unsupported(format!("grammar: {e}")))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::temp(0.2), LlamaSampler::dist(SEED)])
        }
        None => LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.95, 1),
            LlamaSampler::min_p(0.05, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(SEED),
        ]),
    };

    let mut batch = LlamaBatch::new(tokens.len().max(1), 1);
    let last = tokens.len().saturating_sub(1);
    for (i, token) in tokens.iter().enumerate() {
        let pos = i32::try_from(i).expect("within the context");
        batch
            .add(*token, pos, &[0], i == last)
            .map_err(|e| failed("batch")(e.to_string()))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| failed("prompt")(e.to_string()))?;

    let mut answer = Vec::new();
    let start = i32::try_from(tokens.len()).expect("within the context");
    let mut finished = false;
    for position in (start..).take(max_tokens as usize) {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if vocab.is_eog(token) {
            finished = true;
            break;
        }
        answer.extend(vocab.token_to_piece(token, false, None));
        batch.clear();
        batch
            .add(token, position, &[0], true)
            .map_err(|e| failed("batch")(e.to_string()))?;
        ctx.decode(&mut batch)
            .map_err(|e| failed("generation")(e.to_string()))?;
    }

    let text = String::from_utf8_lossy(&answer).trim().to_owned();
    let json = match &request.schema {
        None => None,
        Some(_) => Some(serde_json::from_str(&text).map_err(|_| {
            if finished {
                ProviderError::Failed(format!("{offer} returned JSON that doesn't parse"))
            } else {
                ProviderError::Failed(format!(
                    "{offer}'s answer was cut off at {max_tokens} tokens before the JSON ended"
                ))
            }
        })?),
    };
    Ok(Completion {
        text,
        json,
        model: format!("llama.cpp/{offer}"),
        usage: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template in the style of Gemma's, which llama.cpp's built-in formats don't cover.
    const GEMMA_STYLE: &str = "{{ bos_token }}{% if messages[0]['role'] == 'system' %}{% set first = messages[0]['content'] + '\n\n' %}{% set rest = messages[1:] %}{% else %}{% set first = '' %}{% set rest = messages %}{% endif %}{% for m in rest %}{% if m['role'] not in ['user', 'assistant'] %}{{ raise_exception('unknown role') }}{% endif %}<start_of_turn>{{ 'model' if m['role'] == 'assistant' else m['role'] }}\n{{ (first if loop.first else '') + m['content'] | trim }}<end_of_turn>\n{% endfor %}{% if add_generation_prompt %}<start_of_turn>model\n{% endif %}";

    #[test]
    fn templates_llama_cpp_does_not_know_are_rendered_as_jinja() {
        let request = CompletionRequest {
            system: Some("Be brief.".into()),
            prompt: "  Name this file.  ".into(),
            ..CompletionRequest::default()
        };
        let prompt = render_template(GEMMA_STYLE, &request, "<eos>").unwrap();
        assert_eq!(
            prompt,
            "<start_of_turn>user\nBe brief.\n\nName this file.<end_of_turn>\n<start_of_turn>model\n"
        );
    }

    #[test]
    fn a_template_that_raises_says_why() {
        let request = CompletionRequest {
            prompt: "x".into(),
            ..CompletionRequest::default()
        };
        let error =
            render_template("{{ raise_exception('no system role') }}", &request, "").unwrap_err();
        assert!(error.contains("no system role"), "{error}");
    }

    /// The models folder for tests: `CARREL_TEST_LLAMA_MODELS`, which CI sets to a folder with
    /// a tiny model. Tests that need a model pass without checking anything when it isn't set.
    fn models() -> Option<PathBuf> {
        let dir = std::env::var_os("CARREL_TEST_LLAMA_MODELS").map(PathBuf::from);
        if dir.is_none() {
            eprintln!("CARREL_TEST_LLAMA_MODELS isn't set; skipping");
        }
        dir
    }

    fn first_model(provider: &LlamaProvider) -> String {
        provider.offers()[0].id.clone()
    }

    #[test]
    fn without_models_the_offer_says_how_to_add_one() {
        let empty = std::env::temp_dir().join("carrel-no-models-here");
        let provider = LlamaProvider::with_context(Some(&empty), 512);
        let offers = provider.offers();
        assert_eq!(offers.len(), 1);
        assert!(!offers[0].available);
        assert!(offers[0].reason.as_deref().unwrap().contains(".gguf"));
        assert_eq!(offers[0].priority, priority::PORTABLE);
        assert!(matches!(
            provider.complete("none", &CompletionRequest::default()),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn a_model_answers_prompts() {
        let Some(dir) = models() else { return };
        let provider = LlamaProvider::with_context(Some(&dir), 2048);
        let model = first_model(&provider);
        assert!(provider.offers()[0].available);
        let answer = provider
            .complete(
                &model,
                &CompletionRequest {
                    system: Some("Answer briefly.".into()),
                    prompt: "Once upon a time".into(),
                    max_tokens: Some(24),
                    ..CompletionRequest::default()
                },
            )
            .unwrap();
        assert_ne!(answer.text, "");
        assert_eq!(answer.model, format!("llama.cpp/{model}"));
        assert_eq!(answer.json, None);
    }

    #[test]
    fn schemas_constrain_answers_to_valid_json() {
        let Some(dir) = models() else { return };
        let provider = LlamaProvider::with_context(Some(&dir), 2048);
        let model = first_model(&provider);
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {"enum": ["file", "folder", "bundle"]},
                "sealed": {"type": "boolean"}
            },
            "required": ["kind", "sealed"],
            "additionalProperties": false
        });
        let answer = provider
            .complete(
                &model,
                &CompletionRequest {
                    prompt: "A Rust project folder with Cargo.toml. What kind of item is it?"
                        .into(),
                    schema: Some(schema),
                    max_tokens: Some(64),
                    ..CompletionRequest::default()
                },
            )
            .unwrap();
        let json = answer.json.expect("JSON with a schema");
        assert!(
            ["file", "folder", "bundle"].contains(&json["kind"].as_str().unwrap()),
            "{json}"
        );
        assert!(json["sealed"].is_boolean(), "{json}");
    }

    #[test]
    fn the_model_counts_tokens() {
        let Some(dir) = models() else { return };
        let provider = LlamaProvider::with_context(Some(&dir), 512);
        let model = first_model(&provider);
        let short = provider.count_tokens(&model, "Hello").unwrap();
        let long = provider
            .count_tokens(&model, &"Hello there, oscillators. ".repeat(20))
            .unwrap();
        assert!(short >= 1);
        assert!(long > short * 10, "{short} vs {long}");
        assert!(matches!(
            provider.count_tokens("no-such-model", "x"),
            Err(ProviderError::Unsupported(_))
        ));
    }

    #[test]
    fn prompts_longer_than_the_context_are_unsupported() {
        let Some(dir) = models() else { return };
        let provider = LlamaProvider::with_context(Some(&dir), 256);
        let model = first_model(&provider);
        let long = "the oscillators drift in and out of phase ".repeat(200);
        let Err(ProviderError::Unsupported(why)) = provider.complete(
            &model,
            &CompletionRequest {
                prompt: long,
                ..CompletionRequest::default()
            },
        ) else {
            panic!("expected unsupported");
        };
        assert!(why.contains("too long"), "{why}");
    }
}
