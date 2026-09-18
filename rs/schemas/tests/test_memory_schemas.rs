//! Memory hands output shapes (requirement E-03): Reading, Enrichment,
//! Verdict. Committed like every other schema, and served by the MCP as
//! resources so a host knows a valid answer before it is asked for one.

use open_story_patterns::story::{Enrichment, Reading, Verdict};
use open_story_schemas::{canonicalize, generate, load_schema};
use serde_json::json;

fn validator(file: &str) -> jsonschema::Validator {
    let schema = load_schema(file).expect("committed schema");
    jsonschema::validator_for(&schema).expect("compile schema")
}

macro_rules! drift_test {
    ($name:ident, $ty:ty, $file:literal) => {
        #[test]
        fn $name() {
            let regen = generate::<$ty>();
            let committed = load_schema($file).expect("schema file");
            assert_eq!(
                canonicalize(&regen),
                canonicalize(&committed),
                "drift — regenerate: cargo run -p open-story-schemas --bin generate"
            );
        }
    };
}

drift_test!(reading_schema_is_up_to_date, Reading, "reading.schema.json");
drift_test!(
    enrichment_schema_is_up_to_date,
    Enrichment,
    "enrichment.schema.json"
);
drift_test!(verdict_schema_is_up_to_date, Verdict, "verdict.schema.json");

mod when_a_host_answers_a_prompt {
    use super::*;

    #[test]
    fn a_final_reading_validates() {
        let v = validator("reading.schema.json");
        let reading = json!({
            "handle": "arc0000000000001", "standing": "final",
            "paragraphs": [ { "exchanges": ["ex00000000000001", "ex00000000000002"], "intent": "answer the opening question" } ],
            "author": { "host": "claude-code", "model": "claude-fable-5-1" }
        });
        let errors: Vec<String> = v.iter_errors(&reading).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_reading_without_an_author_is_rejected() {
        let v = validator("reading.schema.json");
        let reading =
            json!({ "handle": "arc0000000000001", "standing": "final", "paragraphs": [] });
        assert!(!v.is_valid(&reading));
    }

    #[test]
    fn an_enrichment_with_slots_validates_and_a_bad_standing_does_not() {
        let e = validator("enrichment.schema.json");
        let enrichment = json!({
            "handle": "arc0000000000001", "title": "t", "question": "q", "resolution": "r", "summary": "s",
            "slots": { "decisions": ["d"], "deferrals": [], "tradeoffs": [], "failures": [], "stance": ["user: x"] },
            "author": { "host": "codex", "model": "gpt" }
        });
        assert!(e.is_valid(&enrichment));
        let r = validator("reading.schema.json");
        let bad = json!({ "handle": "a", "standing": "maybe", "paragraphs": [], "author": { "host": "h", "model": "m" } });
        assert!(!r.is_valid(&bad), "standing is provisional | final");
    }

    #[test]
    fn a_verdict_validates() {
        let v = validator("verdict.schema.json");
        let verdict = json!({ "handle": "arc0000000000001", "seam": 2, "verdict": "same_theme", "reason": "the second exchange answers the first", "author": { "host": "h", "model": "m" } });
        let errors: Vec<String> = v.iter_errors(&verdict).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}
