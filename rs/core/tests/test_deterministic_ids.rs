//! Every translated event id must be reproducible — re-translating the same
//! transcript line yields the SAME CloudEvent id.
//!
//! Why: the boot backfill re-reads recent JSONL and re-publishes. Ids are the
//! dedup key at both the translate layer and the ingest layer; a line whose
//! translation mints a random id becomes a NEW stored event on every server
//! restart. Observed in production: session 0375729d grew 16,712 → 18,254
//! stored events across two restarts, all duplicates being queue.* events —
//! the one line type that carries no `uuid` field.

use open_story_core::translate::{translate_line, TranscriptState};
use serde_json::json;

fn queue_line() -> serde_json::Value {
    // Real shape from a live transcript: queue-operation lines have no uuid.
    json!({
        "type": "queue-operation",
        "operation": "enqueue",
        "timestamp": "2026-07-01T02:39:24.256Z",
        "sessionId": "sess-q",
        "content": "you may want to checkout the latest from master"
    })
}

// describe("when the same uuid-less line is translated twice (a backfill replay)")
#[test]
fn it_should_reproduce_the_same_event_id() {
    let line = queue_line();
    let e1 = translate_line(&line, &mut TranscriptState::new("sess-q".into()));
    let e2 = translate_line(&line, &mut TranscriptState::new("sess-q".into()));
    assert_eq!(e1.len(), 1);
    assert_eq!(
        e1[0].id, e2[0].id,
        "re-translation must reproduce the same id, or every restart stores a duplicate"
    );
}

// describe("when two different uuid-less lines are translated")
#[test]
fn it_should_give_them_distinct_ids() {
    let mut other = queue_line();
    other["timestamp"] = json!("2026-07-01T02:40:00.000Z");
    let mut state = TranscriptState::new("sess-q".into());
    let e1 = translate_line(&queue_line(), &mut state);
    let e2 = translate_line(&other, &mut state);
    assert_eq!(e1.len(), 1);
    assert_eq!(e2.len(), 1);
    assert_ne!(e1[0].id, e2[0].id, "different lines must not collide");
}

// describe("when the identical uuid-less line appears twice in ONE stream")
#[test]
fn it_should_dedup_the_second_occurrence_like_uuid_lines() {
    let mut state = TranscriptState::new("sess-q".into());
    let e1 = translate_line(&queue_line(), &mut state);
    let e2 = translate_line(&queue_line(), &mut state);
    assert_eq!(e1.len(), 1);
    assert_eq!(
        e2.len(),
        0,
        "an exact duplicate line is a replay, not a new event"
    );
}

// describe("when a line carries its own uuid")
#[test]
fn it_should_keep_using_the_source_uuid() {
    let line = json!({
        "type": "user",
        "uuid": "line-uuid-1",
        "timestamp": "2026-07-01T02:39:24.256Z",
        "sessionId": "sess-q",
        "message": {"role": "user", "content": "hi"}
    });
    let events = translate_line(&line, &mut TranscriptState::new("sess-q".into()));
    assert_eq!(events[0].id, "line-uuid-1");
}
