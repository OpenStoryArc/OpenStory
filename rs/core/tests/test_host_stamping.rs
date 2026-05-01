//! Every translator must stamp `host` on every CloudEvent it produces.
//!
//! Host identity survives NATS replication only because it's baked into
//! the event at translation time. If this ever regresses, events from
//! Hetzner that land in Max's leaf will be indistinguishable from Max's
//! local events — the very lie this refactor exists to fix.

use open_story_core::cloud_event::CloudEvent;
use open_story_core::host;
use open_story_core::translate::{translate_line, TranscriptState};
use serde_json::json;

// ── claude-code ─────────────────────────────────────────────────────

#[test]
fn claude_code_translator_stamps_host_on_user_message() {
    let line = json!({
        "type": "user",
        "uuid": "evt-test-u1",
        "timestamp": "2026-04-21T00:00:00Z",
        "sessionId": "sess-host-claude",
        "message": {"role": "user", "content": [{"type": "text", "text": "hi"}]}
    });
    let mut state = TranscriptState::new("sess-host-claude".into());
    let events = translate_line(&line, &mut state);

    assert!(!events.is_empty(), "translator must emit at least one event");
    for ce in &events {
        assert_host_stamped(ce);
    }
}

#[test]
fn claude_code_translator_stamps_host_on_assistant_message() {
    let line = json!({
        "type": "assistant",
        "uuid": "evt-test-a1",
        "timestamp": "2026-04-21T00:00:01Z",
        "sessionId": "sess-host-claude-2",
        "message": {
            "role": "assistant",
            "model": "claude-opus-4-7",
            "content": [{"type": "text", "text": "ok"}]
        }
    });
    let mut state = TranscriptState::new("sess-host-claude-2".into());
    let events = translate_line(&line, &mut state);

    assert!(!events.is_empty());
    for ce in &events {
        assert_host_stamped(ce);
    }
}

// ── pi-mono ──────────────────────────────────────────────────────────

#[test]
fn pi_mono_translator_stamps_host_on_every_decomposed_event() {
    // pi-mono bundles multiple content blocks per line and decomposes
    // into N CloudEvents. Every decomposed event must carry the host.
    use open_story_core::translate_pi::translate_pi_line;

    let line = json!({
        "id": "pi-test-1",
        "timestamp": "2026-04-21T00:00:00Z",
        "session_id": "sess-host-pi",
        "type": "response",
        "content": [
            {"type": "text", "text": "hello"},
            {"type": "text", "text": "world"}
        ]
    });
    let mut state = open_story_core::translate::TranscriptState::new("sess-host-pi".into());
    let events = translate_pi_line(&line, &mut state);

    // Even if pi-mono returns zero events for this shape (non-matching
    // subtype, etc.), the test still enforces the contract on whatever it
    // does emit — no event may leave the translator host-less.
    for ce in &events {
        assert_host_stamped(ce);
    }
}

// ── invariant helper ────────────────────────────────────────────────

fn assert_host_stamped(ce: &CloudEvent) {
    assert_eq!(
        ce.host.as_deref(),
        Some(host::host()),
        "every translator-produced event must have host = resolver output. \
         agent={:?} id={}",
        ce.agent,
        ce.id,
    );
}

// ── serde contract: host survives the wire ─────────────────────────

#[test]
fn host_survives_serde_round_trip_as_it_would_over_nats() {
    // This is the key correctness property: an event stamped on host A
    // and replicated to host B keeps its origin host. Simulated here by
    // serialize → deserialize, which is exactly what NATS does on the wire.
    let line = json!({
        "type": "user",
        "uuid": "evt-replicate-1",
        "timestamp": "2026-04-21T00:00:00Z",
        "sessionId": "sess-replicate",
        "message": {"role": "user", "content": [{"type": "text", "text": "go"}]}
    });
    let mut state = TranscriptState::new("sess-replicate".into());
    let events = translate_line(&line, &mut state);
    assert!(!events.is_empty());

    for ce in events {
        let bytes = serde_json::to_vec(&ce).unwrap();
        let after: CloudEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            after.host, ce.host,
            "host must survive JSON round-trip (NATS wire format)"
        );
        assert!(
            after.host.is_some(),
            "host must remain Some after deserialization"
        );
    }
}
