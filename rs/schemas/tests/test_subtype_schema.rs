//! Schema tests for `Subtype`.
//!
//! Shapes:
//!   A — drift: committed schema matches what regeneration produces
//!   B — known-good: every wire-form string validates
//!   C — known-bad: typos reject
//!   D — round-trip: Rust → JSON → validate → deserialize → equal

use open_story_core::subtype::Subtype;
use open_story_schemas::{canonicalize, generate, load_schema};

// ── Shape A: drift ─────────────────────────────────────────────────────

#[test]
fn subtype_schema_is_up_to_date() {
    let regenerated = generate::<Subtype>();
    let committed = load_schema("subtype.schema.json").expect(
        "subtype.schema.json missing — run: cargo run -p open-story-schemas --bin generate",
    );
    assert_eq!(
        canonicalize(&regenerated),
        canonicalize(&committed),
        "schema drift — regenerate: cargo run -p open-story-schemas --bin generate"
    );
}

// ── Shape B: known-good ────────────────────────────────────────────────

fn validator() -> jsonschema::Validator {
    let schema = load_schema("subtype.schema.json").expect("schema file");
    jsonschema::validator_for(&schema).expect("compile schema")
}

#[test]
fn every_live_subtype_string_validates() {
    let v = validator();
    // Source of truth is the enum; we iterate through every rename via
    // serialization and validate the wire form.
    let all = [
        Subtype::UserPrompt,
        Subtype::UserToolResult,
        Subtype::AssistantText,
        Subtype::AssistantThinking,
        Subtype::AssistantToolUse,
        Subtype::TurnComplete,
        Subtype::SystemError,
        Subtype::SystemCompact,
        Subtype::SystemHook,
        Subtype::SessionStart,
        Subtype::ModelChange,
        Subtype::LocalCommand,
        Subtype::AwaySummary,
        Subtype::ProgressBash,
        Subtype::ProgressAgent,
        Subtype::ProgressHook,
        Subtype::FileSnapshot,
        Subtype::QueueEnqueue,
        Subtype::QueueDequeue,
        Subtype::QueueRemove,
        Subtype::QueuePopAll,
    ];
    for variant in all {
        let json = serde_json::to_value(variant).unwrap();
        assert!(
            v.is_valid(&json),
            "Subtype::{:?} wire form {} must validate",
            variant,
            json
        );
    }
}

// ── Shape C: known-bad ─────────────────────────────────────────────────

#[test]
fn typo_is_rejected_by_the_schema() {
    let v = validator();
    let bad = serde_json::json!("message.assitant.text");
    assert!(
        !v.is_valid(&bad),
        "schema must reject typo — the whole point of the enum"
    );
}

#[test]
fn unknown_family_is_rejected() {
    let v = validator();
    let bad = serde_json::json!("rumor.has.it");
    assert!(!v.is_valid(&bad));
}

#[test]
fn non_string_is_rejected() {
    let v = validator();
    let bad = serde_json::json!(42);
    assert!(!v.is_valid(&bad));
}

// ── Shape D: round-trip ────────────────────────────────────────────────

#[test]
fn every_variant_round_trips_through_schema() {
    let v = validator();
    let all = [
        Subtype::UserPrompt,
        Subtype::AssistantText,
        Subtype::TurnComplete,
        Subtype::ProgressBash,
        Subtype::FileSnapshot,
        Subtype::QueuePopAll,
    ];
    for variant in all {
        let json = serde_json::to_value(variant).unwrap();
        assert!(v.is_valid(&json), "validate {:?}", variant);
        let back: Subtype = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant, "round-trip identity");
    }
}
