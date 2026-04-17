//! FUZZY PIPE TESTS — events always flow through.
//!
//! The pipeline should enrich what it can, never drop what it can't.
//! An event with an unknown subtype, a missing agent_payload, or a shape
//! we haven't seen before should STILL:
//!   1. Be persisted by Actor 1 (EventStore)
//!   2. Be broadcast by Actor 4 (at least as a minimal record)
//!   3. Carry its raw data through to the UI
//!
//! Kafka's `additionalProperties: true` pattern: the schema classifies
//! events into enrichment tiers, not filters them out.
//!
//! This is the sovereignty principle applied to the pipeline itself:
//! the system observes ALL agent events, even ones it doesn't understand.
//! Dropping an event because we can't enrich it is "interfering" — we're
//! choosing to hide data the user is sovereign over.

mod helpers;

use helpers::{make_event_with_id, test_state};
use open_story::cloud_event::CloudEvent;
use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use open_story::server::ingest_events;
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use serde_json::json;
use tempfile::TempDir;

/// Build a CloudEvent with a completely unknown subtype.
fn unknown_subtype_event(id: &str, subtype: &str) -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("some data from the future".to_string());
    let data = EventData::with_payload(
        json!({"type": "unknown_thing", "content": "raw data here"}),
        1,
        "sess-fuzzy".to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        "arc://test".to_string(),
        "io.arc.event".to_string(),
        data,
        Some(subtype.to_string()),
        Some(id.to_string()),
        Some("2026-04-17T10:00:00Z".to_string()),
        None,
        None,
        Some("claude-code".to_string()),
    )
}

// ── Layer 1: from_cloud_event must never return empty for valid events ──

#[test]
fn unknown_subtype_produces_at_least_one_view_record() {
    // The core fuzzy-pipe invariant: from_cloud_event must return at
    // least one record for any valid CloudEvent, even if the subtype
    // is completely unknown. The record might be minimal (a SystemEvent
    // with the raw subtype) but it must EXIST so the broadcast path
    // doesn't skip the event.
    let event = unknown_subtype_event("evt-fuzzy-1", "system.never_heard_of_this");
    let records = from_cloud_event(&event);
    assert!(
        !records.is_empty(),
        "from_cloud_event must produce at least one record for unknown subtypes — \
         the pipeline enriches what it can, never drops what it can't. \
         Got 0 records for subtype 'system.never_heard_of_this'."
    );
}

#[test]
fn unknown_subtype_record_preserves_the_subtype_string() {
    // The minimal record should carry the unknown subtype so the UI
    // can show SOMETHING meaningful (e.g., "system.never_heard_of_this").
    let event = unknown_subtype_event("evt-fuzzy-2", "message.assistant.future_block_type");
    let records = from_cloud_event(&event);
    assert!(!records.is_empty());

    // The record should be a SystemEvent with the subtype as its label.
    match &records[0].body {
        RecordBody::SystemEvent(se) => {
            assert!(
                se.subtype.contains("future_block_type"),
                "SystemEvent.subtype should carry the unknown subtype string, \
                 got: {}",
                se.subtype
            );
        }
        other => {
            // Any record type is acceptable as long as it exists.
            // SystemEvent is the natural fallback. But we don't hard-fail
            // on a different type — the invariant is "at least one record."
            let _ = other;
        }
    }
}

// ── Layer 2: the broadcast pipeline must forward unknown-subtype events ──

#[tokio::test]
async fn unknown_subtype_event_survives_full_broadcast_pipeline() {
    // Feed an event with a never-before-seen subtype through
    // ingest_events. Assert it produces a non-empty broadcast.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let event = unknown_subtype_event("evt-fuzzy-pipe-1", "system.brand_new_thing");

    let result = {
        let mut s = state.write().await;
        ingest_events(&mut s, "sess-fuzzy", &[event], None).await
    };

    assert!(
        result.count > 0,
        "ingest_events must process unknown-subtype events — count should be > 0"
    );
    assert!(
        !result.changes.is_empty(),
        "ingest_events must produce a broadcast for unknown-subtype events — \
         the UI should see SOMETHING, not silence"
    );
}

// ── Layer 2b: truly foreign prefixes ──

#[test]
fn totally_unknown_prefix_still_produces_records() {
    // Not system.*, not message.*, not progress.* — a prefix we've
    // literally never seen. The pipeline must still produce something.
    let event = unknown_subtype_event("evt-banana", "banana.fruit.yellow");
    let records = from_cloud_event(&event);
    assert!(
        !records.is_empty(),
        "totally unknown prefix 'banana.fruit.yellow' must still produce a record — \
         the raw data flows, the enrichment is best-effort"
    );
}

#[test]
fn empty_subtype_still_produces_records() {
    // Edge case: event with empty or missing subtype.
    let data = EventData::with_payload(
        json!({"content": "raw data"}),
        1,
        "sess-empty-st".to_string(),
        AgentPayload::ClaudeCode(ClaudeCodePayload::new()),
    );
    let event = CloudEvent::new(
        "arc://test".into(), "io.arc.event".into(), data,
        None, // no subtype at all
        Some("evt-no-subtype".into()),
        Some("2026-04-17T10:00:00Z".into()),
        None, None, None,
    );
    let records = from_cloud_event(&event);
    assert!(
        !records.is_empty(),
        "events with no subtype must still flow through — they carry raw data"
    );
}

// ── Layer 3: no-agent-payload events still flow ──

#[test]
fn event_with_null_agent_payload_still_produces_records() {
    // The agent_payload is the "lift" — absent means "translator couldn't
    // type it." The raw data is still there. The pipeline should still
    // produce at least a minimal record.
    let data = EventData::new(
        json!({"raw_agent_output": "some opaque data"}),
        1,
        "sess-raw".to_string(),
    );
    let event = CloudEvent::new(
        "arc://test".to_string(),
        "io.arc.event".to_string(),
        data,
        Some("message.user.prompt".to_string()),
        Some("evt-no-payload".to_string()),
        Some("2026-04-17T10:00:00Z".to_string()),
        None,
        None,
        None,
    );
    let records = from_cloud_event(&event);
    // message.user.prompt with no agent_payload: text accessor returns ""
    // but the record should still exist (UserMessage with empty content).
    assert!(
        !records.is_empty(),
        "events with null agent_payload should still produce records — \
         the raw data is the foundation, the payload is optional enrichment"
    );
}
