//! Stage 6: how sensitive an item is (`docs/design/ingest.md`). It decides what always waits
//! for the owner, so it errs high: rules set a floor without a model, and a model may only
//! raise it.

use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::card::Sensitivity;
use crate::providers::{Capability, CompletionRequest, Offer, Provider, Registry};
use crate::review::Attempt;

/// How much of an item's text the floor rules read, in bytes.
const SCAN_BYTES: usize = 200_000;

/// The most tokens a sensitivity answer may take.
const RATING_TOKENS: u32 = 240;

/// How much text the model sees, in bytes; enough to judge the kind of document.
const MODEL_BYTES: usize = 6_000;

/// How sensitive an item is, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// The level.
    pub level: Sensitivity,
    /// Why, in words that never quote what was found.
    pub reasons: Vec<String>,
    /// The model call, if one was made.
    pub attempts: Vec<Attempt>,
    /// The restricted values found, such as card numbers, so a card that repeats one can be
    /// caught. Kept only in memory: never written to a proposal, card or log.
    pub matched: Vec<String>,
}

static CARD_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){12,18}\d\b").expect("valid"));
static IBAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]){11,30}\b").expect("valid"));
static US_SSN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:\d{3}-\d{2}-\d{4}|(?i:ssn|social security)\D{0,20}\d{9})\b").expect("valid")
});
static PRIVATE_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").expect("valid"));
static PASSWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\s*(?:password|passcode|pin|secret)\s*[:=]\s*\S").expect("valid")
});

/// Words in the text that mark a kind of sensitive document, and what to call it.
const SENSITIVE_WORDS: [(&str, &str); 13] = [
    (
        r"(?i)\btax return\b|\bform 1040\b|\bw-2\b|\b1099\b|\bself assessment\b",
        "a tax document",
    ),
    (
        r"(?i)\bdiagnosis\b|\bprescription\b|\bpatient\b|\bmedical record\b|\blab results?\b",
        "medical information",
    ),
    (
        r"(?i)\baccount number\b|\brouting number\b|\bsort code\b|\bstatement balance\b",
        "banking details",
    ),
    (r"(?i)\bpassport\b", "a passport"),
    (r"(?i)\bdriver'?s licen[cs]e\b", "a driving licence"),
    (
        r"(?i)\bpayslip\b|\bpay stub\b|\bsalary\b|\bgross pay\b",
        "pay details",
    ),
    (
        r"(?i)\bpolicy number\b|\binsurance claim\b",
        "insurance details",
    ),
    (r"(?i)\blease agreement\b|\btenancy\b", "a lease"),
    (r"(?i)\bdate of birth\b", "a date of birth"),
    (r"(?i)\bbirth certificate\b", "a birth certificate"),
    (
        r"(?i)\blast will and testament\b|\bpower of attorney\b",
        "a legal document",
    ),
    (
        r"(?i)\bpin code\b|\bsecurity code\b|\bcvv\b",
        "security codes",
    ),
    (r"(?i)\bconfidential\b", "a document marked confidential"),
];

/// Words in a file's name that mark it as sensitive.
const SENSITIVE_NAMES: [&str; 12] = [
    "passport",
    "tax",
    "medical",
    "bank",
    "statement",
    "payslip",
    "salary",
    "insurance",
    "lease",
    "visa",
    "license",
    "licence",
];

static SENSITIVE_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    SENSITIVE_WORDS
        .iter()
        .map(|(pattern, what)| (Regex::new(pattern).expect("valid"), *what))
        .collect()
});

/// Whether `digits` (with spaces or dashes) pass the Luhn check card numbers use.
fn luhn(candidate: &str) -> bool {
    let digits: Vec<u32> = candidate.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Whether `candidate` is an IBAN with valid check digits.
fn iban(candidate: &str) -> bool {
    let compact: String = candidate.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 15 || compact.len() > 34 {
        return false;
    }
    let rearranged = format!("{}{}", &compact[4..], &compact[..4]);
    let mut remainder: u64 = 0;
    for c in rearranged.chars() {
        let value = match c {
            '0'..='9' => u64::from(c.to_digit(10).unwrap_or(0)),
            'A'..='Z' => u64::from(c as u8 - b'A' + 10),
            _ => return false,
        };
        let width = if value >= 10 { 100 } else { 10 };
        remainder = (remainder * width + value) % 97;
    }
    remainder == 1
}

/// The strings in `text` that the floor rules treat as restricted, such as card numbers, so
/// a card that repeats one can be caught.
#[must_use]
pub fn restricted_matches(text: &str) -> Vec<String> {
    let text = &text[..text.floor_char_boundary(SCAN_BYTES)];
    let mut found: Vec<String> = CARD_NUMBER
        .find_iter(text)
        .map(|m| m.as_str())
        .filter(|m| luhn(m))
        .chain(IBAN.find_iter(text).map(|m| m.as_str()).filter(|m| iban(m)))
        .chain(US_SSN.find_iter(text).map(|m| m.as_str()))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The floor: the level the rules alone give, never below `default` (the destination's).
#[must_use]
pub fn floor(name: &str, text: &str, default: Sensitivity) -> Assessment {
    let scanned = &text[..text.floor_char_boundary(SCAN_BYTES)];
    let mut level = default;
    let mut reasons = Vec::new();
    let mut raise = |to: Sensitivity, why: String, level: &mut Sensitivity| {
        if to > *level {
            *level = to;
        }
        if to >= Sensitivity::Sensitive && !reasons.contains(&why) {
            reasons.push(why);
        }
    };

    if CARD_NUMBER.find_iter(scanned).any(|m| luhn(m.as_str())) {
        raise(
            Sensitivity::Restricted,
            "contains what looks like a card number".into(),
            &mut level,
        );
    }
    if IBAN.find_iter(scanned).any(|m| iban(m.as_str())) {
        raise(
            Sensitivity::Restricted,
            "contains what looks like a bank account number (IBAN)".into(),
            &mut level,
        );
    }
    if US_SSN.is_match(scanned) {
        raise(
            Sensitivity::Restricted,
            "contains what looks like a social security number".into(),
            &mut level,
        );
    }
    if PRIVATE_KEY.is_match(scanned) {
        raise(
            Sensitivity::Restricted,
            "contains a private key".into(),
            &mut level,
        );
    }
    if PASSWORD.is_match(scanned) {
        raise(
            Sensitivity::Restricted,
            "contains what looks like a password".into(),
            &mut level,
        );
    }
    for (pattern, what) in SENSITIVE_PATTERNS.iter() {
        if pattern.is_match(scanned) {
            raise(
                Sensitivity::Sensitive,
                format!("mentions {what}"),
                &mut level,
            );
        }
    }
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    for word in SENSITIVE_NAMES {
        if words.contains(&word) {
            raise(
                Sensitivity::Sensitive,
                format!("its name mentions {word}"),
                &mut level,
            );
        }
    }
    Assessment {
        level,
        reasons,
        attempts: Vec::new(),
        matched: restricted_matches(text),
    }
}

/// The reason given for an item that records where it was made.
pub const LOCATION_REASON: &str = "has the place it was taken";

impl Assessment {
    /// Notes that the item records where it was made, such as a photo's location: at least
    /// `personal`, but no higher, since most phone photos have one and raising them all would
    /// bury the ones that are sensitive.
    pub fn located(&mut self) {
        self.level = self.level.max(Sensitivity::Personal);
        if !self.reasons.iter().any(|r| r == LOCATION_REASON) {
            self.reasons.push(LOCATION_REASON.to_owned());
        }
    }
}

fn level_name(level: Sensitivity) -> &'static str {
    match level {
        Sensitivity::Public => "public",
        Sensitivity::Personal => "personal",
        Sensitivity::Sensitive => "sensitive",
        Sensitivity::Restricted => "restricted",
    }
}

/// The floor, then the best text model's judgment, which may only raise it. A model that
/// declines or fails passes the item to the next, as drafting does.
#[must_use]
pub fn assess(registry: &Registry, name: &str, text: &str, default: Sensitivity) -> Assessment {
    let mut assessment = floor(name, text, default);
    if assessment.level == Sensitivity::Restricted {
        return assessment;
    }
    for (provider, offer) in registry.ranked(Capability::TextModel) {
        if rate(provider, offer, name, text, &mut assessment) {
            break;
        }
    }
    assessment
}

/// One model's rating, raising `assessment` if it's higher. Whether the model answered.
fn rate(
    provider: &dyn Provider,
    offer: &Offer,
    name: &str,
    text: &str,
    assessment: &mut Assessment,
) -> bool {
    let allowed: Vec<&str> = [
        Sensitivity::Public,
        Sensitivity::Personal,
        Sensitivity::Sensitive,
        Sensitivity::Restricted,
    ]
    .into_iter()
    .filter(|l| *l >= assessment.level)
    .map(level_name)
    .collect();
    let excerpt = &text[..text.floor_char_boundary(MODEL_BYTES)];
    let mut prompt = format!("Name: {name}\n");
    if !assessment.reasons.is_empty() {
        writeln!(prompt, "Already noticed: {}", assessment.reasons.join("; ")).unwrap();
    }
    writeln!(prompt, "\nText:\n{excerpt}").unwrap();
    let request = CompletionRequest {
        system: Some(
            "Rate how sensitive this item from a personal archive is. public: fine for anyone to see. \
             personal: the owner's own material, not harmful if seen. sensitive: health, money, legal, \
             identity or other private matters. restricted: would cause real harm if seen, such as \
             account numbers, passwords or identity documents. When in doubt, choose the higher level."
                .into(),
        ),
        prompt,
        schema: Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["level", "reason"],
            "properties": {
                "level": {"enum": allowed},
                "reason": {"type": "string", "description": "One short sentence, at most 15 words, without quoting private details"}
            }
        })),
        // Room for a local model's fuller reason, so the JSON isn't cut off.
        max_tokens: Some(RATING_TOKENS),
    };
    let attempt = |result: &str, message: Option<String>| Attempt {
        stage: "sensitivity".into(),
        provider: provider.name().to_owned(),
        offer: offer.id.clone(),
        result: result.into(),
        message,
    };
    let mut answered = false;
    match provider.complete(&offer.id, &request) {
        Ok(completion) => {
            let json = completion.json.unwrap_or_default();
            let level = match json["level"].as_str() {
                Some("restricted") => Some(Sensitivity::Restricted),
                Some("sensitive") => Some(Sensitivity::Sensitive),
                Some("personal") => Some(Sensitivity::Personal),
                Some("public") => Some(Sensitivity::Public),
                _ => None,
            };
            match level {
                Some(level) => {
                    assessment.attempts.push(attempt("ok", None));
                    answered = true;
                    if level > assessment.level {
                        assessment.level = level;
                        if level >= Sensitivity::Sensitive
                            && let Some(reason) = json["reason"].as_str()
                        {
                            assessment.reasons.push(reason.trim().to_owned());
                        }
                    }
                }
                None => assessment
                    .attempts
                    .push(attempt("failed", Some("the answer wasn't a level".into()))),
            }
        }
        Err(error) => {
            let result = error.result();
            assessment
                .attempts
                .push(attempt(result, Some(error.to_string())));
        }
    }
    answered
}
