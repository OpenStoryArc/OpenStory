//! Envelope schema tests — the fuzzy-pipe classification boundary.
//!
//! Two schemas, two tiers:
//!   - cloud_event.schema.json (full) — validates known subtypes +
//!     typed agent payloads. Events matching this get rich ViewRecord
//!     enrichment.
//!   - cloud_event_envelope.schema.json (minimal) — validates the bare
//!     minimum (id, type, time, data.raw). Events matching ONLY this
//!     tier get passthrough as SystemEvent with raw data visible.
//!
//! The fuzzy-pipe invariant: every real event matches at least the
//! envelope. Events matching the full schema are a strict subset.
//!
//! See docs/research/architecture-audit/PRINCIPLES.md for context.

use open_story_schemas::load_schema;
use serde_json::json;

fn full_validator() -> jsonschema::Validator {
    let s = load_schema("cloud_event.schema.json").expect("full schema");
    jsonschema::validator_for(&s).expect("compile full")
}

fn envelope_validator() -> jsonschema::Validator {
    let s = load_schema("cloud_event_envelope.schema.json").expect("envelope schema");
    jsonschema::validator_for(&s).expect("compile envelope")
}

// ── Tier A: known events pass BOTH schemas ─────────────────────────────

#[test]
fn known_claude_code_event_passes_both_schemas() {
    let event = json!({
        "specversion": "1.0",
        "id": "evt-1",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:00Z",
        "datacontenttype": "application/json",
        "subtype": "message.user.prompt",
        "agent": "claude-code",
        "data": {
            "seq": 1,
            "session_id": "sess-1",
            "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hi"}]}},
            "agent_payload": {
                "_variant": "claude-code",
                "meta": {"agent": "claude-code"},
                "text": "hi"
            }
        }
    });
    assert!(full_validator().is_valid(&event), "known event must pass full schema");
    assert!(envelope_validator().is_valid(&event), "known event must also pass envelope");
}

// ── Tier B: unknown events fail full, pass envelope ────────────────────

#[test]
fn unknown_subtype_fails_full_but_passes_envelope() {
    // An event from a future agent or a new Claude Code subtype we
    // haven't added to the schema yet. It has the right envelope
    // shape (id, type, time, data.raw) but the subtype and payload
    // don't match the full schema's expectations.
    let event = json!({
        "id": "evt-future-1",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:01Z",
        "subtype": "banana.fruit.yellow",
        "data": {
            "raw": {"some_agent_field": "opaque data from the future"}
        }
    });
    // Full schema rejects (missing specversion, missing seq, unknown subtype)
    assert!(
        !full_validator().is_valid(&event),
        "unknown-subtype event should fail full schema validation"
    );
    // Envelope accepts (has id, type, time, data.raw)
    assert!(
        envelope_validator().is_valid(&event),
        "unknown-subtype event should pass envelope — it's a real event \
         worth persisting and broadcasting, just not enrichable"
    );
}

#[test]
fn minimal_raw_only_event_passes_envelope() {
    // The absolute minimum: just the four required fields + data.raw.
    // No subtype, no agent, no specversion. Still a valid event that
    // should flow through the pipeline.
    let event = json!({
        "id": "evt-bare",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:02Z",
        "data": {
            "raw": {}
        }
    });
    assert!(envelope_validator().is_valid(&event));
}

// ── Tier C: truly broken data fails both ───────────────────────────────

#[test]
fn missing_id_fails_both_schemas() {
    let event = json!({
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:03Z",
        "data": {"raw": {}}
    });
    assert!(!full_validator().is_valid(&event));
    assert!(!envelope_validator().is_valid(&event), "no id = not even an envelope");
}

#[test]
fn missing_data_raw_fails_envelope() {
    // Has id, type, time — but data has no raw field. The sovereignty
    // foundation is missing. Not a valid event.
    let event = json!({
        "id": "evt-no-raw",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:04Z",
        "data": {"some_other_field": "but no raw"}
    });
    assert!(
        !envelope_validator().is_valid(&event),
        "missing data.raw = sovereignty violation, not a valid envelope"
    );
}

#[test]
fn empty_id_fails_envelope() {
    let event = json!({
        "id": "",
        "type": "io.arc.event",
        "time": "2026-04-17T10:00:05Z",
        "data": {"raw": {}}
    });
    assert!(
        !envelope_validator().is_valid(&event),
        "empty string id is not a valid event identifier"
    );
}

// ── Real-data invariant: every live event passes the envelope ──────────

#[tokio::test]
#[ignore = "requires OpenStory on localhost:3002"]
async fn every_live_event_passes_the_envelope_schema() {
    // The strongest fuzzy-pipe assertion: fetch real events from the
    // running instance and validate each against the ENVELOPE (not the
    // full schema). Any failure means a real event in production doesn't
    // meet the minimum viable CloudEvent contract.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let envelope = envelope_validator();

    let sessions: serde_json::Value = client
        .get("http://localhost:3002/api/sessions")
        .send().await.unwrap()
        .json().await.unwrap();
    let session_ids: Vec<String> = sessions
        .get("sessions").and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .iter()
        .take(20)
        .filter_map(|s| s.get("session_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let mut total = 0usize;
    let mut failures = 0usize;
    for sid in &session_ids {
        let events: Vec<serde_json::Value> = client
            .get(format!("http://localhost:3002/api/sessions/{sid}/events"))
            .send().await.unwrap_or_else(|_| panic!("fetch"))
            .json().await.unwrap_or_default();
        for ev in &events {
            total += 1;
            if !envelope.is_valid(ev) {
                failures += 1;
                if failures <= 3 {
                    let id = ev.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    eprintln!("  ❌ event {id} fails envelope validation");
                }
            }
        }
    }

    eprintln!("\n  envelope validation: {total} events, {failures} failures");
    assert_eq!(
        failures, 0,
        "{failures}/{total} live events fail the envelope schema — \
         the fuzzy pipe's minimum contract is violated"
    );
}
