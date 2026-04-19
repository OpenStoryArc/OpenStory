//! Runtime schema validation tests — the three-tier classification.
//!
//! Tests that from_cloud_event_value correctly classifies events:
//!   Tier A: fully deserializable → rich ViewRecord enrichment
//!   Tier B: envelope-valid, can't fully type → SystemEvent passthrough
//!   Tier C: fails envelope → empty (below sovereignty floor)
//!
//! These tests exercise the runtime envelope schema validation that
//! sits at the deserialization boundary — the first point where raw
//! JSON meets the type system.

use open_story_views::from_cloud_event::{from_cloud_event, from_cloud_event_value};
use open_story_views::unified::RecordBody;
use serde_json::json;

// ── Tier A: fully typed events use the normal enrichment path ──────────

#[test]
fn tier_a_known_event_produces_rich_view_record() {
    let event_json = json!({
        "specversion": "1.0",
        "id": "evt-tier-a",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:00Z",
        "datacontenttype": "application/json",
        "subtype": "message.user.prompt",
        "data": {
            "seq": 1,
            "session_id": "sess-1",
            "raw": {"type": "user"},
            "agent_payload": {
                "_variant": "claude-code",
                "meta": {"agent": "claude-code"},
                "text": "hello world"
            }
        }
    });
    let records = from_cloud_event_value(&event_json);
    assert!(!records.is_empty(), "Tier A: known event must produce records");
    assert!(
        matches!(&records[0].body, RecordBody::UserMessage(_)),
        "Tier A: known user prompt should produce UserMessage, got {:?}",
        records[0].body
    );
}

// ── Tier B: envelope-valid but not fully deserializable ────────────────

#[test]
fn tier_b_envelope_valid_but_missing_specversion_still_produces_record() {
    // This event has id + type + time + data.raw (passes envelope)
    // but is missing specversion + datacontenttype (fails CloudEvent
    // deserialization). The schema fallback should catch it and produce
    // a SystemEvent passthrough.
    let event_json = json!({
        "id": "evt-tier-b",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:01Z",
        "subtype": "some.future.subtype",
        "data": {
            "raw": {"agent": "future-agent", "content": "opaque data"}
        }
    });

    // Sanity: this CANNOT deserialize as a typed CloudEvent
    assert!(
        serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event_json.clone()).is_err(),
        "this event must fail CloudEvent deserialization (missing specversion)"
    );

    let records = from_cloud_event_value(&event_json);
    assert!(
        !records.is_empty(),
        "Tier B: envelope-valid event must produce at least one record \
         even when full deserialization fails — this is the fuzzy pipe"
    );
    // The record should carry the subtype
    match &records[0].body {
        RecordBody::SystemEvent(se) => {
            assert!(
                se.subtype.contains("future"),
                "Tier B record should carry the original subtype, got: {}",
                se.subtype
            );
        }
        _ => panic!("Tier B should produce SystemEvent"),
    }
}

#[test]
fn tier_b_preserves_event_id_and_timestamp() {
    let event_json = json!({
        "id": "evt-preserve-me",
        "type": "io.arc.event",
        "time": "2026-04-17T12:34:56Z",
        "data": {"raw": {"x": 1}}
    });
    let records = from_cloud_event_value(&event_json);
    assert!(!records.is_empty());
    assert_eq!(records[0].id, "evt-preserve-me");
    assert_eq!(records[0].timestamp, "2026-04-17T12:34:56Z");
}

// ── Tier C: below the sovereignty floor ────────────────────────────────

#[test]
fn tier_c_missing_id_produces_no_record() {
    let event_json = json!({
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:02Z",
        "data": {"raw": {}}
    });
    let records = from_cloud_event_value(&event_json);
    assert!(
        records.is_empty(),
        "Tier C: missing id = not an event, no record should be produced"
    );
}

#[test]
fn tier_c_missing_data_raw_produces_no_record() {
    let event_json = json!({
        "id": "evt-no-raw",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:03Z",
        "data": {"some_field": "but no raw"}
    });
    let records = from_cloud_event_value(&event_json);
    assert!(
        records.is_empty(),
        "Tier C: missing data.raw = sovereignty violation, no record"
    );
}

#[test]
fn tier_c_not_even_json_object_produces_no_record() {
    let records = from_cloud_event_value(&json!("just a string"));
    assert!(records.is_empty(), "Tier C: non-object = not an event");
}

// ── Cross-check: from_cloud_event_value is a superset of from_cloud_event ──

#[test]
fn value_path_produces_same_records_as_typed_path_for_known_events() {
    // For events that CAN be fully typed, both paths should agree.
    let event_json = json!({
        "specversion": "1.0",
        "id": "evt-cross",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:04Z",
        "datacontenttype": "application/json",
        "subtype": "message.assistant.text",
        "agent": "claude-code",
        "data": {
            "seq": 1,
            "session_id": "sess-cross",
            "raw": {},
            "agent_payload": {
                "_variant": "claude-code",
                "meta": {"agent": "claude-code"},
                "text": "response text",
                "model": "claude-opus-4-6"
            }
        }
    });
    let ce: open_story_core::cloud_event::CloudEvent =
        serde_json::from_value(event_json.clone()).unwrap();

    let from_typed = from_cloud_event(&ce);
    let from_value = from_cloud_event_value(&event_json);

    assert_eq!(
        from_typed.len(),
        from_value.len(),
        "both paths should produce the same number of records"
    );
    for (t, v) in from_typed.iter().zip(from_value.iter()) {
        assert_eq!(t.id, v.id, "record ids should match");
    }
}
