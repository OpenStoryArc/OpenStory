//! Schema tests for `CloudEvent`.
//!
//! Shapes A/B/C/D from SCHEMA_TESTS.md. Fixtures derived from real captured
//! sessions (pi-mono fixtures + synthetic claude-code/hermes events).

use open_story_core::cloud_event::CloudEvent;
use open_story_schemas::{canonicalize, generate, load_schema};
use serde_json::{json, Value};

// ── Shape A: drift ─────────────────────────────────────────────────────

#[test]
fn cloud_event_schema_is_up_to_date() {
    let regenerated = generate::<CloudEvent>();
    let committed = load_schema("cloud_event.schema.json").expect(
        "cloud_event.schema.json missing — run: cargo run -p open-story-schemas --bin generate",
    );
    assert_eq!(
        canonicalize(&regenerated),
        canonicalize(&committed),
        "drift — regenerate: cargo run -p open-story-schemas --bin generate"
    );
}

// ── Validator helper ───────────────────────────────────────────────────

fn validator() -> jsonschema::Validator {
    let schema = load_schema("cloud_event.schema.json").expect("schema");
    jsonschema::validator_for(&schema).expect("compile cloud_event schema")
}

// ── Fixtures ───────────────────────────────────────────────────────────

fn claude_code_text_event() -> Value {
    json!({
        "specversion": "1.0",
        "id": "evt-cc-1",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-15T10:00:00Z",
        "datacontenttype": "application/json",
        "subtype": "message.assistant.text",
        "agent": "claude-code",
        "data": {
            "seq": 1,
            "session_id": "sess-test",
            "raw": {"type": "assistant", "message": {"content": [{"type": "text", "text": "hi"}]}},
            "agent_payload": {
                "_variant": "claude-code",
                "meta": {"agent": "claude-code"},
                "uuid": "cc-1",
                "text": "hi"
            }
        }
    })
}

fn pi_mono_tool_use_event() -> Value {
    json!({
        "specversion": "1.0",
        "id": "evt-pi-1",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-15T10:00:01Z",
        "datacontenttype": "application/json",
        "subtype": "message.assistant.tool_use",
        "agent": "pi-mono",
        "data": {
            "seq": 1,
            "session_id": "sess-pi",
            "raw": {"type": "message", "message": {"role": "assistant"}},
            "agent_payload": {
                "_variant": "pi-mono",
                "meta": {"agent": "pi-mono"},
                "tool": "read",
                "tool_call_id": "toolu_abc",
                "args": {"path": "/tmp/x.toml"}
            }
        }
    })
}

fn hermes_delegated_event() -> Value {
    json!({
        "specversion": "1.0",
        "id": "evt-hm-1",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-15T10:00:02Z",
        "datacontenttype": "application/json",
        "subtype": "message.assistant.tool_use",
        "agent": "hermes",
        "data": {
            "seq": 1,
            "session_id": "sess-hm",
            "raw": {},
            "agent_payload": {
                "_variant": "hermes",
                "meta": {"agent": "hermes"},
                "tool": "read_file",
                "tool_use_id": "hm-1"
            }
        }
    })
}

fn event_without_agent_payload() -> Value {
    // Legitimate state: translator couldn't type the line.
    // agent_payload is absent; raw alone carries the info.
    json!({
        "specversion": "1.0",
        "id": "evt-raw-1",
        "source": "arc://test",
        "type": "io.arc.event",
        "time": "2026-04-15T10:00:03Z",
        "datacontenttype": "application/json",
        "data": {
            "seq": 1,
            "session_id": "sess-raw",
            "raw": {"unknown": "shape"}
        }
    })
}

// ── Shape B: known-good ────────────────────────────────────────────────

#[test]
fn accepts_claude_code_text_event() {
    let v = validator();
    let ev = claude_code_text_event();
    assert!(v.is_valid(&ev), "claude-code text event must validate");
}

#[test]
fn accepts_pi_mono_tool_use_event() {
    let v = validator();
    let ev = pi_mono_tool_use_event();
    assert!(v.is_valid(&ev), "pi-mono tool_use event must validate");
}

#[test]
fn accepts_hermes_delegated_event() {
    let v = validator();
    let ev = hermes_delegated_event();
    assert!(v.is_valid(&ev), "hermes event must validate");
}

#[test]
fn accepts_event_without_agent_payload() {
    let v = validator();
    let ev = event_without_agent_payload();
    assert!(
        v.is_valid(&ev),
        "CloudEvent with raw-only data (translator couldn't type) must validate"
    );
}

// ── Shape C: known-bad ─────────────────────────────────────────────────

#[test]
fn rejects_missing_specversion() {
    let v = validator();
    let mut bad = claude_code_text_event();
    bad.as_object_mut().unwrap().remove("specversion");
    assert!(!v.is_valid(&bad), "missing specversion must reject");
}

#[test]
fn rejects_missing_id() {
    let v = validator();
    let mut bad = claude_code_text_event();
    bad.as_object_mut().unwrap().remove("id");
    assert!(!v.is_valid(&bad));
}

#[test]
fn rejects_agent_payload_variant_typo() {
    // The single most load-bearing field tag in the envelope.
    let v = validator();
    let mut bad = pi_mono_tool_use_event();
    bad["data"]["agent_payload"]["_variant"] = json!("pimono"); // missing the hyphen
    assert!(
        !v.is_valid(&bad),
        "typo in _variant tag must reject — this is what catches a serde rename drift"
    );
}

#[test]
fn rejects_missing_data_field() {
    let v = validator();
    let mut bad = claude_code_text_event();
    bad.as_object_mut().unwrap().remove("data");
    assert!(!v.is_valid(&bad));
}

// ── Shape D: round-trip ────────────────────────────────────────────────

#[test]
fn round_trips_each_agent_variant() {
    let v = validator();
    for fixture in [
        claude_code_text_event(),
        pi_mono_tool_use_event(),
        hermes_delegated_event(),
    ] {
        assert!(v.is_valid(&fixture), "fixture validates");
        let ce: CloudEvent = serde_json::from_value(fixture.clone())
            .expect("deserialize as CloudEvent");
        let reserialized = serde_json::to_value(&ce).unwrap();
        assert!(
            v.is_valid(&reserialized),
            "re-serialized value must still validate"
        );
        // Deep equality isn't required (serde may elide default fields);
        // validation + deserializing back proves the loop closes.
        let back: CloudEvent = serde_json::from_value(reserialized).unwrap();
        assert_eq!(back.id, ce.id);
        assert_eq!(back.subtype, ce.subtype);
        assert_eq!(back.agent, ce.agent);
    }
}
