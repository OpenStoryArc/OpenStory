//! Shape A drift + Shape B/C/D coverage for the remaining five schemas:
//!   - PatternEvent
//!   - StructuralTurn
//!   - IngestBatch
//!   - SessionRow
//!   - FtsSearchResult
//!   - BroadcastMessage
//!
//! These are smaller, leafier types than CloudEvent / ViewRecord. Test
//! coverage is tighter — one drift test + one good + one bad + one
//! round-trip per schema — which matches their surface area.

use open_story_bus::IngestBatch;
use open_story_patterns::{PatternEvent, StructuralTurn};
use open_story_schemas::{canonicalize, generate, load_schema};
use open_story_server::broadcast::BroadcastMessage;
use open_story_store::event_store::SessionRow;
use open_story_store::queries::FtsSearchResult;
use serde_json::{json, Value};

// ── Shape A: drift ─────────────────────────────────────────────────────

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

drift_test!(pattern_event_schema_is_up_to_date, PatternEvent, "pattern_event.schema.json");
drift_test!(structural_turn_schema_is_up_to_date, StructuralTurn, "structural_turn.schema.json");
drift_test!(ingest_batch_schema_is_up_to_date, IngestBatch, "ingest_batch.schema.json");
drift_test!(session_row_schema_is_up_to_date, SessionRow, "session_row.schema.json");
drift_test!(fts_search_result_schema_is_up_to_date, FtsSearchResult, "fts_search_result.schema.json");
drift_test!(broadcast_message_schema_is_up_to_date, BroadcastMessage, "broadcast_message.schema.json");

fn validator_for(basename: &str) -> jsonschema::Validator {
    let schema = load_schema(basename).expect("schema");
    jsonschema::validator_for(&schema).expect("compile")
}

// ── PatternEvent ───────────────────────────────────────────────────────

#[test]
fn pattern_event_accepts_real_shape() {
    let v = validator_for("pattern_event.schema.json");
    let fixture = json!({
        "pattern_type": "eval_apply.eval",
        "session_id": "sess-1",
        "event_ids": ["evt-1", "evt-2"],
        "started_at": "2026-04-15T10:00:00Z",
        "ended_at": "2026-04-15T10:00:02Z",
        "summary": "user asked a question",
        "metadata": {"some": "data"}
    });
    assert!(v.is_valid(&fixture));
}

#[test]
fn pattern_event_rejects_missing_session_id() {
    let v = validator_for("pattern_event.schema.json");
    let bad = json!({
        "pattern_type": "eval_apply.eval",
        "event_ids": [],
        "started_at": "2026-04-15T10:00:00Z",
        "ended_at": "2026-04-15T10:00:02Z",
        "summary": "x",
        "metadata": {}
    });
    assert!(!v.is_valid(&bad));
}

// ── IngestBatch ────────────────────────────────────────────────────────

#[test]
fn ingest_batch_accepts_empty_events_array() {
    let v = validator_for("ingest_batch.schema.json");
    let fixture = json!({
        "session_id": "sess-1",
        "project_id": "proj-1",
        "events": []
    });
    assert!(v.is_valid(&fixture));
}

#[test]
fn ingest_batch_rejects_missing_session_id() {
    let v = validator_for("ingest_batch.schema.json");
    let bad = json!({
        "project_id": "proj-1",
        "events": []
    });
    assert!(!v.is_valid(&bad));
}

// ── SessionRow ─────────────────────────────────────────────────────────

#[test]
fn session_row_accepts_fully_populated_row() {
    let v = validator_for("session_row.schema.json");
    let fixture = json!({
        "id": "sess-1",
        "project_id": "proj",
        "project_name": "My Project",
        "label": "fix the thing",
        "custom_label": null,
        "branch": "main",
        "event_count": 42,
        "first_event": "2026-04-15T10:00:00Z",
        "last_event": "2026-04-15T10:10:00Z"
    });
    assert!(v.is_valid(&fixture));
}

#[test]
fn session_row_accepts_minimal_row() {
    let v = validator_for("session_row.schema.json");
    let fixture = json!({
        "id": "sess-1",
        "project_id": null,
        "project_name": null,
        "label": null,
        "custom_label": null,
        "branch": null,
        "event_count": 0,
        "first_event": null,
        "last_event": null
    });
    assert!(v.is_valid(&fixture));
}

// ── FtsSearchResult ────────────────────────────────────────────────────

#[test]
fn fts_search_result_accepts_real_shape() {
    let v = validator_for("fts_search_result.schema.json");
    let fixture = json!({
        "event_id": "evt-1",
        "session_id": "sess-1",
        "record_type": "user_message",
        "snippet": "a match …",
        "rank": -1.23
    });
    assert!(v.is_valid(&fixture));
}

#[test]
fn fts_search_result_rejects_non_numeric_rank() {
    let v = validator_for("fts_search_result.schema.json");
    let bad = json!({
        "event_id": "evt-1",
        "session_id": "sess-1",
        "record_type": "user_message",
        "snippet": "x",
        "rank": "not a number"
    });
    assert!(!v.is_valid(&bad));
}

// ── BroadcastMessage ───────────────────────────────────────────────────

#[test]
fn broadcast_message_accepts_enriched_minimal() {
    let v = validator_for("broadcast_message.schema.json");
    let fixture = json!({
        "kind": "enriched",
        "session_id": "sess-1",
        "records": [],
        "ephemeral": [],
        "filter_deltas": {}
    });
    assert!(v.is_valid(&fixture));
}

#[test]
fn broadcast_message_accepts_plan_saved() {
    let v = validator_for("broadcast_message.schema.json");
    let fixture = json!({"kind": "plan_saved", "session_id": "sess-x"});
    assert!(v.is_valid(&fixture));
}

#[test]
fn broadcast_message_rejects_unknown_kind() {
    let v = validator_for("broadcast_message.schema.json");
    let bad = json!({"kind": "banana", "session_id": "s"});
    assert!(!v.is_valid(&bad));
}

#[test]
fn broadcast_message_rejects_missing_kind_tag() {
    let v = validator_for("broadcast_message.schema.json");
    let bad = json!({"session_id": "s", "records": [], "ephemeral": [], "filter_deltas": {}});
    assert!(!v.is_valid(&bad));
}

// ── StructuralTurn — smoke coverage ────────────────────────────────────

#[test]
fn structural_turn_accepts_minimal_turn() {
    let v = validator_for("structural_turn.schema.json");
    let fixture = json!({
        "session_id": "sess-1",
        "turn_number": 0,
        "scope_depth": 0,
        "human": null,
        "thinking": null,
        "eval": null,
        "applies": [],
        "env_size": 0,
        "env_delta": 0,
        "stop_reason": "end_turn",
        "is_terminal": true,
        "timestamp": "2026-04-15T10:00:00Z",
        "duration_ms": null,
        "event_ids": ["evt-1"]
    });
    assert!(v.is_valid(&fixture));
}

// ── Shape D: Rust → JSON → validate round-trip ─────────────────────────

#[test]
fn ingest_batch_round_trips_via_schema() {
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::EventData;

    let v = validator_for("ingest_batch.schema.json");
    let ce = CloudEvent::new(
        "arc://test".into(),
        "io.arc.event".into(),
        EventData::new(json!({}), 1, "sess-1".into()),
        Some("message.user.prompt".into()),
        Some("evt-rt".into()),
        Some("2026-04-15T10:00:00Z".into()),
        None, None, None,
    );
    let batch = IngestBatch {
        session_id: "sess-1".into(),
        project_id: "proj-1".into(),
        events: vec![ce],
    };
    let json_val: Value = serde_json::to_value(&batch).unwrap();
    assert!(v.is_valid(&json_val));
    let back: IngestBatch = serde_json::from_value(json_val).unwrap();
    assert_eq!(back.session_id, "sess-1");
    assert_eq!(back.events.len(), 1);
}
