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

// ═══════════════════════════════════════════════════════════════════
// The record envelope and the two remaining payload kinds
// ═══════════════════════════════════════════════════════════════════

use open_story_patterns::story::{Keep, MemoryRecord, Saga};

drift_test!(
    memory_record_schema_is_up_to_date,
    MemoryRecord,
    "memory_record.schema.json"
);
drift_test!(saga_schema_is_up_to_date, Saga, "saga.schema.json");
drift_test!(keep_schema_is_up_to_date, Keep, "keep.schema.json");

mod when_a_record_is_checked_off_the_wire {
    use super::*;

    #[test]
    fn a_stored_record_validates_and_one_without_an_author_does_not() {
        let v = validator("memory_record.schema.json");
        let rec = json!({
            "id": "enrichment:arc0000000000001:-:claude-code:m", "session_id": "s", "handle": "arc0000000000001",
            "kind": "enrichment", "author": { "host": "claude-code", "model": "m" }, "created_at": "2026-09-18T20:00:00Z",
            "payload": { "handle": "arc0000000000001", "title": "t" }
        });
        let errors: Vec<String> = v.iter_errors(&rec).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        let mut bad = rec.clone();
        bad.as_object_mut().unwrap().remove("author");
        assert!(!v.is_valid(&bad));
        let mut bad_kind = rec.clone();
        bad_kind["kind"] = json!("gossip");
        assert!(!v.is_valid(&bad_kind), "kind is a closed set");
    }

    #[test]
    fn saga_and_keep_have_shapes() {
        let s = validator("saga.schema.json");
        assert!(s.is_valid(&json!({ "handle": "a", "handles": ["a", "b"], "reason": "same problem", "author": { "host": "h", "model": "m" } })));
        assert!(!s.is_valid(&json!({ "handle": "a", "handles": ["a"], "reason": "r", "author": { "host": "h", "model": "m" } })), "a saga links at least two arcs");
        let k = validator("keep.schema.json");
        assert!(k.is_valid(&json!({ "handle": "a", "reason": "produced the decision", "author": { "host": "h", "model": "m" } })));
        assert!(
            !k.is_valid(&json!({ "handle": "a", "author": { "host": "h", "model": "m" } })),
            "a keep needs a reason"
        );
    }
}
