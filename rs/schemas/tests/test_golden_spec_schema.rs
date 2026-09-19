//! GoldenSpec schema: drift + validation (memory hands, requirement X-01).
//!
//! A GoldenSpec is the typed, immutable description a story golden is
//! derived from. Its schema is committed like every other serialization
//! boundary so fixtures under rs/patterns/tests/fixtures/story/ can be
//! checked against it without Rust.

use open_story_patterns::golden::{ExchangeKind, ExchangeSpec, GoldenSpec, PromptClass, TurnSpec};
use open_story_schemas::{canonicalize, generate, load_schema};

fn validator() -> jsonschema::Validator {
    let schema = load_schema("golden_spec.schema.json").expect("committed golden_spec schema");
    jsonschema::validator_for(&schema).expect("compile golden_spec schema")
}

fn example_spec() -> GoldenSpec {
    GoldenSpec {
        session_id: "golden-single-arc".to_string(),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: 1800,
        exchanges: vec![
            ExchangeSpec {
                kind: ExchangeKind::Human,
                prompt_class: PromptClass::Short,
                gap_before_secs: 0,
                turns: vec![TurnSpec {
                    verb: "read".to_string(),
                    objects: vec!["src/lib.rs".to_string()],
                    tools: vec![("Read".to_string(), 2)],
                    rich: true,
                }],
            },
            ExchangeSpec {
                kind: ExchangeKind::Injected,
                prompt_class: PromptClass::Long,
                gap_before_secs: 5,
                turns: vec![TurnSpec {
                    verb: "checked".to_string(),
                    objects: vec![],
                    tools: vec![("Bash".to_string(), 1)],
                    rich: false,
                }],
            },
        ],
    }
}

mod when_a_golden_spec_is_serialized {
    use super::*;

    #[test]
    fn it_validates_against_its_emitted_schema() {
        let value = serde_json::to_value(example_spec()).expect("spec → json");
        let v = validator();
        let errors: Vec<String> = v.iter_errors(&value).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "spec must validate, errors: {errors:?}");
        assert_eq!(value["exchanges"][1]["kind"], "injected");
        assert_eq!(value["exchanges"][0]["turns"][0]["tools"][0][1], 2);
    }

    #[test]
    fn the_committed_schema_has_no_drift() {
        let regen = generate::<GoldenSpec>();
        let committed = load_schema("golden_spec.schema.json").expect("schema file");
        assert_eq!(
            canonicalize(&regen),
            canonicalize(&committed),
            "drift — regenerate: cargo run -p open-story-schemas --bin generate"
        );
    }

    #[test]
    fn a_spec_with_an_unknown_exchange_kind_is_rejected() {
        let mut value = serde_json::to_value(example_spec()).expect("spec → json");
        value["exchanges"][0]["kind"] = serde_json::json!("robot");
        assert!(
            !validator().is_valid(&value),
            "unknown kind must be rejected by the schema"
        );
    }
}
