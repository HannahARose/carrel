//! Ingest's sensitivity stage: the floor rules, and a model that may only raise the level.

use carrel_core::card::Sensitivity;
use carrel_core::ingest::sensitivity::{LOCATION_REASON, assess, floor, restricted_matches};
use carrel_core::providers::{ProviderError, Registry};
use serde_json::json;

mod common;
use common::Scripted;

fn level(name: &str, text: &str) -> Sensitivity {
    floor(name, text, Sensitivity::Personal).level
}

#[test]
fn restricted_values_are_checked_not_just_shaped() {
    assert_eq!(
        level("a.txt", "Card 4111 1111 1111 1111"),
        Sensitivity::Restricted
    );
    assert_eq!(
        level("a.txt", "Order 4111 1111 1111 1112"),
        Sensitivity::Personal,
        "fails the Luhn check, so it's just a long number"
    );
    assert_eq!(
        level("a.txt", "Pay to GB82 WEST 1234 5698 7654 32"),
        Sensitivity::Restricted
    );
    assert_eq!(
        level("a.txt", "Ref GB83 WEST 1234 5698 7654 32"),
        Sensitivity::Personal,
        "wrong IBAN check digits"
    );
    assert_eq!(level("a.txt", "SSN 078-05-1120"), Sensitivity::Restricted);
    assert_eq!(
        level("a.txt", "-----BEGIN OPENSSH PRIVATE KEY-----\nabc"),
        Sensitivity::Restricted
    );
    assert_eq!(
        level("a.txt", "wifi\npassword: hunter2\n"),
        Sensitivity::Restricted
    );
    assert_eq!(
        restricted_matches("Card 4111 1111 1111 1111 and 078-05-1120"),
        ["078-05-1120", "4111 1111 1111 1111"]
    );
}

#[test]
fn sensitive_words_and_names_raise_the_floor_with_reasons_that_quote_nothing() {
    let tax = floor("notes.txt", "My 2024 tax return", Sensitivity::Personal);
    assert_eq!(tax.level, Sensitivity::Sensitive);
    assert_eq!(tax.reasons, ["mentions a tax document"]);

    let named = floor("Passport scan.pdf", "", Sensitivity::Personal);
    assert_eq!(named.level, Sensitivity::Sensitive);
    assert_eq!(named.reasons, ["its name mentions passport"]);

    let card = floor("a.txt", "Card 4111 1111 1111 1111", Sensitivity::Personal);
    assert!(card.reasons.iter().all(|r| !r.contains("4111")));
    assert_eq!(card.matched, ["4111 1111 1111 1111"]);

    let plain = floor("Lamp notes.txt", "Solder the LED", Sensitivity::Personal);
    assert_eq!(plain.level, Sensitivity::Personal);
    assert_eq!(plain.reasons, Vec::<String>::new());
    assert_eq!(
        floor("a.txt", "", Sensitivity::Sensitive).level,
        Sensitivity::Sensitive,
        "never below the destination's default"
    );
}

fn registry_of(model: Scripted) -> Registry {
    let mut registry = Registry::new();
    registry.add(Box::new(model));
    registry
}

#[test]
fn a_model_may_raise_the_level_but_is_only_offered_levels_at_or_above_the_floor() {
    let model = Scripted::new(
        "a",
        100,
        8192,
        vec![Ok(
            json!({"level": "sensitive", "reason": "a letter about a court case"}),
        )],
    );
    let asked = model.asked.clone();
    let assessment = assess(
        &registry_of(model),
        "letter.txt",
        "Dear Sam",
        Sensitivity::Personal,
    );
    assert_eq!(assessment.level, Sensitivity::Sensitive);
    assert_eq!(assessment.reasons, ["a letter about a court case"]);
    assert_eq!(assessment.attempts.len(), 1);
    let schema = asked.lock().unwrap()[0].schema.clone().unwrap();
    assert_eq!(
        schema["properties"]["level"]["enum"],
        json!(["personal", "sensitive", "restricted"])
    );

    let model = Scripted::new(
        "a",
        100,
        8192,
        vec![Ok(json!({"level": "public", "reason": "nothing private"}))],
    );
    let assessment = assess(&registry_of(model), "tax.pdf", "", Sensitivity::Personal);
    assert_eq!(
        assessment.level,
        Sensitivity::Sensitive,
        "a lower answer can't lower the floor"
    );
}

#[test]
fn restricted_items_and_failed_models_keep_the_floor() {
    let model = Scripted::new("a", 100, 8192, Vec::new());
    let asked = model.asked.clone();
    let assessment = assess(
        &registry_of(model),
        "a.txt",
        "Card 4111 1111 1111 1111",
        Sensitivity::Personal,
    );
    assert_eq!(assessment.level, Sensitivity::Restricted);
    assert!(asked.lock().unwrap().is_empty(), "no model is asked");

    let model = Scripted::new(
        "a",
        100,
        8192,
        vec![Err(ProviderError::Unavailable("asleep".into()))],
    );
    let assessment = assess(&registry_of(model), "a.txt", "hi", Sensitivity::Personal);
    assert_eq!(assessment.level, Sensitivity::Personal);
    assert_eq!(assessment.attempts[0].result, "unavailable");
}

#[test]
fn a_model_that_declines_passes_the_rating_to_the_next() {
    let mut registry = Registry::new();
    registry.add(Box::new(Scripted::new(
        "fast",
        100,
        8192,
        vec![Err(ProviderError::Declined("unsafe".into()))],
    )));
    registry.add(Box::new(Scripted::new(
        "careful",
        50,
        8192,
        vec![Ok(json!({"level": "sensitive", "reason": "a lab result"}))],
    )));
    let assessment = assess(
        &registry,
        "blood test.txt",
        "Ferritin 12",
        Sensitivity::Personal,
    );
    assert_eq!(assessment.level, Sensitivity::Sensitive);
    let results: Vec<(&str, &str)> = assessment
        .attempts
        .iter()
        .map(|a| (a.provider.as_str(), a.result.as_str()))
        .collect();
    assert_eq!(results, [("fast", "declined"), ("careful", "ok")]);
}

#[test]
fn a_located_photo_is_personal_but_no_higher() {
    let mut public = floor(
        "IMG_0550.jpeg",
        "Image: photo, with a location",
        Sensitivity::Public,
    );
    public.located();
    assert_eq!(public.level, Sensitivity::Personal);
    assert_eq!(public.reasons, vec![LOCATION_REASON.to_owned()]);
    public.located();
    assert_eq!(public.reasons.len(), 1, "the reason is given once");

    let mut sensitive = floor("medical-scan.jpeg", "", Sensitivity::Sensitive);
    sensitive.located();
    assert_eq!(sensitive.level, Sensitivity::Sensitive);
}
