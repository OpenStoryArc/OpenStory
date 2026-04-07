//! EventStore conformance suite — backend-parametric behavioral tests.
//!
//! This file is the contract every full-featured `EventStore` implementation
//! must honor. The pure async helpers below take `&dyn EventStore` and assert
//! on observable behavior — return values, ordering, dedup semantics, FTS
//! ranking, lifecycle invariants. They make no assumptions about which
//! backend they're running against.
//!
//! Each backend (currently SQLite; MongoDB lands in Phase 2+) gets a wrapper
//! `mod` that calls `make_store()` and forwards every helper as a
//! `#[tokio::test]`. When a new backend is added, the parity bar is "make
//! every helper green against your backend, with no helper modifications."
//!
//! That's the whole point of the suite. It's not for catching SQLite bugs
//! (the inline `#[cfg(test)]` mod in `sqlite_store.rs` does that). It's for
//! catching subtle backend divergence — BSON vs SQLite type round-tripping,
//! ordering tie-breaks, dedup semantics on duplicate-key errors, FTS score
//! ordering, custom_label preservation across upserts.
//!
//! When a helper here passes against MongoStore but fails against SqliteStore
//! (or vice versa), the trait contract is wrong, not the backend.

use std::sync::Arc;

use serde_json::{json, Value};

use open_story_patterns::{PatternEvent, StructuralTurn};
use open_story_store::event_store::{EventStore, SessionRow};
// Analytics output struct imports get added back as new helpers are
// written. Keeping the import list minimal to silence unused-import
// warnings during the Phase 5 TDD walk.
#[allow(unused_imports)]
use open_story_store::queries::{
    HourlyActivity, ProjectPulse, ProjectSession, SessionError, SessionSynopsis,
    ToolCount, ToolStep,
};

// ───────────────────────────────────────────────────────────────────────
// Canonical timestamp format
// ───────────────────────────────────────────────────────────────────────
//
// See §1.5 of `docs/research/mongo-analytics-parity-plan.md` for the
// derivation. The translator at `rs/core/src/translate.rs:473` and
// `translate_pi.rs:330` pass-through this format from the source JSONL.
// Fixture timestamps must be byte-identical to what production stores —
// never use `chrono::DateTime::to_rfc3339()` here.
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Returns a fixture timestamp at `hours_ago` hours and
/// `extra_minutes_ago` minutes before now, formatted in the canonical
/// translator format. Used by `seed_analytics_universe` to produce
/// timestamps that fall within any reasonable `days` window of `now()`.
fn ts_offset(hours_ago: i64, extra_minutes_ago: i64) -> String {
    (chrono::Utc::now()
        - chrono::Duration::hours(hours_ago)
        - chrono::Duration::minutes(extra_minutes_ago))
        .format(TS_FORMAT)
        .to_string()
}

// ───────────────────────────────────────────────────────────────────────
// Test fixtures
// ───────────────────────────────────────────────────────────────────────

fn test_event(id: &str, session_id: &str, timestamp: &str, text: &str) -> Value {
    json!({
        "id": id,
        "type": "io.arc.event",
        "subtype": "message.user.prompt",
        "source": format!("arc://transcript/{session_id}"),
        "time": timestamp,
        "data": {
            "raw": {
                "type": "user",
                "message": {"content": [{"type": "text", "text": text}]}
            },
            "seq": 1,
            "session_id": session_id
        }
    })
}

fn test_session_row(id: &str, label: Option<&str>) -> SessionRow {
    SessionRow {
        id: id.to_string(),
        project_id: Some("test-project".to_string()),
        project_name: Some("Test Project".to_string()),
        label: label.map(|s| s.to_string()),
        custom_label: None,
        branch: Some("main".to_string()),
        event_count: 0,
        first_event: Some("2025-01-14T00:00:00Z".to_string()),
        last_event: Some("2025-01-14T01:00:00Z".to_string()),
    }
}

fn test_pattern(session_id: &str, ptype: &str, started_at: &str) -> PatternEvent {
    PatternEvent {
        pattern_type: ptype.to_string(),
        session_id: session_id.to_string(),
        event_ids: vec!["evt-a".into(), "evt-b".into()],
        started_at: started_at.to_string(),
        ended_at: "2025-01-14T00:01:00Z".to_string(),
        summary: format!("{ptype} summary"),
        metadata: json!({"key": "value"}),
    }
}

fn test_turn(session_id: &str, turn_number: u32, timestamp: &str) -> StructuralTurn {
    // Build via the public StructuralTurn shape from open-story-patterns.
    // Mirror what the eval-apply pipeline emits for a minimal completed turn.
    serde_json::from_value(json!({
        "session_id": session_id,
        "turn_number": turn_number,
        "scope_depth": 0,
        "human": null,
        "thinking": null,
        "eval": null,
        "applies": [],
        "env_size": 0,
        "env_delta": 0,
        "stop_reason": "end_turn",
        "is_terminal": true,
        "timestamp": timestamp,
        "duration_ms": null,
        "event_ids": [],
    }))
    .expect("test_turn fixture must match StructuralTurn schema")
}

// ───────────────────────────────────────────────────────────────────────
// Write path conformance
// ───────────────────────────────────────────────────────────────────────

pub async fn it_inserts_a_new_event_and_returns_true(store: Arc<dyn EventStore>) {
    let event = test_event("evt-1", "sess-1", "2025-01-14T00:00:00Z", "hello");
    let inserted = store.insert_event("sess-1", &event).await.unwrap();
    assert!(inserted, "first insert of a new event id must return true");
}

pub async fn it_returns_false_when_inserting_a_duplicate_event(store: Arc<dyn EventStore>) {
    let event = test_event("evt-dup", "sess-1", "2025-01-14T00:00:00Z", "hello");
    assert!(store.insert_event("sess-1", &event).await.unwrap());
    let second = store.insert_event("sess-1", &event).await.unwrap();
    assert!(
        !second,
        "second insert with the same event id must return false (PK dedup)"
    );
}

pub async fn it_treats_event_id_as_a_global_primary_key(store: Arc<dyn EventStore>) {
    // Same event id under two different sessions should still dedup —
    // the contract is "id is the global primary key", not "(session, id)".
    let event = test_event("evt-shared", "sess-a", "2025-01-14T00:00:00Z", "hi");
    assert!(store.insert_event("sess-a", &event).await.unwrap());
    let second = store.insert_event("sess-b", &event).await.unwrap();
    assert!(
        !second,
        "duplicate event id across sessions must dedup (global PK)"
    );
}

pub async fn it_reports_the_count_of_new_events_in_a_batch(store: Arc<dyn EventStore>) {
    let events = vec![
        test_event("b1", "sess-batch", "2025-01-14T00:00:01Z", "one"),
        test_event("b2", "sess-batch", "2025-01-14T00:00:02Z", "two"),
        test_event("b3", "sess-batch", "2025-01-14T00:00:03Z", "three"),
    ];
    let count = store.insert_batch("sess-batch", &events).await.unwrap();
    assert_eq!(count, 3, "batch insert must report all 3 as new");
}

pub async fn it_dedupes_a_batch_against_already_stored_events(store: Arc<dyn EventStore>) {
    let existing = test_event("b-existing", "sess-batch", "2025-01-14T00:00:01Z", "old");
    store.insert_event("sess-batch", &existing).await.unwrap();

    let events = vec![
        test_event("b-existing", "sess-batch", "2025-01-14T00:00:01Z", "old"), // dup
        test_event("b-new", "sess-batch", "2025-01-14T00:00:02Z", "new"),
    ];
    let count = store.insert_batch("sess-batch", &events).await.unwrap();
    assert_eq!(count, 1, "batch must report only the new event as inserted");
}

pub async fn it_handles_an_empty_batch(store: Arc<dyn EventStore>) {
    let count = store.insert_batch("sess-empty", &[]).await.unwrap();
    assert_eq!(count, 0);
}

pub async fn it_upserts_a_session_and_lists_it(store: Arc<dyn EventStore>) {
    let row = test_session_row("sess-up", Some("first label"));
    store.upsert_session(&row).await.unwrap();

    let sessions = store.list_sessions().await.unwrap();
    let found = sessions
        .iter()
        .find(|s| s.id == "sess-up")
        .expect("upserted session must appear in list_sessions");
    assert_eq!(found.label.as_deref(), Some("first label"));
    assert_eq!(found.project_id.as_deref(), Some("test-project"));
}

pub async fn it_updates_an_existing_session_on_upsert(store: Arc<dyn EventStore>) {
    let mut row = test_session_row("sess-update", Some("v1"));
    store.upsert_session(&row).await.unwrap();

    row.label = Some("v2".to_string());
    row.event_count = 99;
    store.upsert_session(&row).await.unwrap();

    let sessions = store.list_sessions().await.unwrap();
    let found = sessions.iter().find(|s| s.id == "sess-update").unwrap();
    assert_eq!(found.label.as_deref(), Some("v2"));
    assert_eq!(found.event_count, 99);
}

/// Critical contract: `upsert_session` (called from boot replay + live
/// ingest) must NEVER overwrite a `custom_label` set by the user via
/// `update_session_label`. The trait doc on `SessionRow.custom_label`
/// states this explicitly.
pub async fn it_never_overwrites_a_user_set_custom_label(store: Arc<dyn EventStore>) {
    let row = test_session_row("sess-cl", Some("auto label"));
    store.upsert_session(&row).await.unwrap();

    // User sets a custom label
    store
        .update_session_label("sess-cl", "user picked this")
        .await
        .unwrap();

    // Boot replay style: re-upsert the projection (no custom_label set on the row)
    let mut row2 = test_session_row("sess-cl", Some("regenerated auto label"));
    row2.event_count = 123;
    store.upsert_session(&row2).await.unwrap();

    let sessions = store.list_sessions().await.unwrap();
    let found = sessions.iter().find(|s| s.id == "sess-cl").unwrap();
    assert_eq!(
        found.custom_label.as_deref(),
        Some("user picked this"),
        "custom_label set by user must survive subsequent upsert_session calls"
    );
    // Auto label should still update
    assert_eq!(found.label.as_deref(), Some("regenerated auto label"));
    assert_eq!(found.event_count, 123);
    // display_label prefers custom_label
    assert_eq!(found.display_label(), Some("user picked this"));
}

pub async fn it_persists_and_queries_a_detected_pattern(store: Arc<dyn EventStore>) {
    let pattern = test_pattern("sess-pat", "test.cycle", "2025-01-14T00:00:00Z");
    store.insert_pattern("sess-pat", &pattern).await.unwrap();

    let patterns = store.session_patterns("sess-pat", None).await.unwrap();
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].pattern_type, "test.cycle");
    assert_eq!(patterns[0].metadata["key"], "value");
}

pub async fn it_filters_session_patterns_by_type(store: Arc<dyn EventStore>) {
    store
        .insert_pattern(
            "sess-pat-f",
            &test_pattern("sess-pat-f", "test.cycle", "2025-01-14T00:00:00Z"),
        )
        .await
        .unwrap();
    store
        .insert_pattern(
            "sess-pat-f",
            &test_pattern("sess-pat-f", "git.workflow", "2025-01-14T00:00:01Z"),
        )
        .await
        .unwrap();
    store
        .insert_pattern(
            "sess-pat-f",
            &test_pattern("sess-pat-f", "test.cycle", "2025-01-14T00:00:02Z"),
        )
        .await
        .unwrap();

    let cycles = store
        .session_patterns("sess-pat-f", Some("test.cycle"))
        .await
        .unwrap();
    assert_eq!(cycles.len(), 2);
    assert!(cycles.iter().all(|p| p.pattern_type == "test.cycle"));

    let all = store.session_patterns("sess-pat-f", None).await.unwrap();
    assert_eq!(all.len(), 3);
}

pub async fn it_persists_and_queries_a_structural_turn(store: Arc<dyn EventStore>) {
    let turn = test_turn("sess-turn", 1, "2025-01-14T00:00:00Z");
    store.insert_turn("sess-turn", &turn).await.unwrap();

    let turns = store.session_turns("sess-turn").await.unwrap();
    assert_eq!(turns.len(), 1, "inserted turn must come back from session_turns");
    assert_eq!(turns[0].turn_number, 1);
    assert_eq!(turns[0].session_id, "sess-turn");
    assert_eq!(turns[0].timestamp, "2025-01-14T00:00:00Z");
}

pub async fn it_upserts_a_plan_idempotently(store: Arc<dyn EventStore>) {
    // The trait exposes upsert_plan as write-only — no read accessor on the
    // trait itself (plans are read via PlanStore in the project). The
    // conformance bar here is just "doesn't error on insert + update".
    store
        .upsert_plan("plan-1", "sess-plan", "# Plan v1\n\nStep 1")
        .await
        .unwrap();
    store
        .upsert_plan("plan-1", "sess-plan", "# Plan v2\n\nStep 1, revised")
        .await
        .unwrap();
}

// ───────────────────────────────────────────────────────────────────────
// Read path conformance
// ───────────────────────────────────────────────────────────────────────

pub async fn it_returns_session_events_ordered_by_timestamp(store: Arc<dyn EventStore>) {
    // Insert out of order; they must come back ordered by `time`.
    store
        .insert_event(
            "sess-ord",
            &test_event("o3", "sess-ord", "2025-01-14T00:00:03Z", "third"),
        )
        .await
        .unwrap();
    store
        .insert_event(
            "sess-ord",
            &test_event("o1", "sess-ord", "2025-01-14T00:00:01Z", "first"),
        )
        .await
        .unwrap();
    store
        .insert_event(
            "sess-ord",
            &test_event("o2", "sess-ord", "2025-01-14T00:00:02Z", "second"),
        )
        .await
        .unwrap();

    let events = store.session_events("sess-ord").await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["id"], "o1");
    assert_eq!(events[1]["id"], "o2");
    assert_eq!(events[2]["id"], "o3");
}

pub async fn it_round_trips_an_event_payload_losslessly(store: Arc<dyn EventStore>) {
    let original = test_event("rt", "sess-rt", "2025-01-14T00:00:00Z", "round-trip me");
    store.insert_event("sess-rt", &original).await.unwrap();

    let events = store.session_events("sess-rt").await.unwrap();
    assert_eq!(events.len(), 1);
    let stored = &events[0];

    // The full CloudEvent envelope must round-trip (id, type, subtype, time,
    // source, data). This is the BSON-vs-JSON divergence trap that bites
    // Mongo backends — int32/int64, datetime, nested objects, arrays.
    assert_eq!(stored["id"], original["id"]);
    assert_eq!(stored["type"], original["type"]);
    assert_eq!(stored["subtype"], original["subtype"]);
    assert_eq!(stored["time"], original["time"]);
    assert_eq!(stored["source"], original["source"]);
    assert_eq!(stored["data"]["seq"], original["data"]["seq"]);
    assert_eq!(stored["data"]["session_id"], original["data"]["session_id"]);
    assert_eq!(
        stored["data"]["raw"]["message"]["content"][0]["text"],
        original["data"]["raw"]["message"]["content"][0]["text"]
    );
}

pub async fn it_returns_no_events_for_an_unknown_session(store: Arc<dyn EventStore>) {
    let events = store.session_events("nonexistent-session").await.unwrap();
    assert!(events.is_empty());
}

pub async fn it_starts_with_an_empty_session_list(store: Arc<dyn EventStore>) {
    let sessions = store.list_sessions().await.unwrap();
    assert!(sessions.is_empty());
}

pub async fn it_returns_the_full_payload_for_a_known_event(store: Arc<dyn EventStore>) {
    let event = test_event("fp", "sess-fp", "2025-01-14T00:00:00Z", "payload");
    store.insert_event("sess-fp", &event).await.unwrap();

    let payload = store.full_payload("fp").await.unwrap();
    assert!(payload.is_some(), "full_payload must return Some for known event");
    let parsed: Value = serde_json::from_str(&payload.unwrap()).unwrap();
    assert_eq!(parsed["id"], "fp");
}

pub async fn it_returns_none_for_an_unknown_event_payload(store: Arc<dyn EventStore>) {
    let payload = store.full_payload("does-not-exist").await.unwrap();
    assert!(payload.is_none());
}

pub async fn it_exports_a_session_as_newline_delimited_json(store: Arc<dyn EventStore>) {
    store
        .insert_event(
            "sess-exp",
            &test_event("e1", "sess-exp", "2025-01-14T00:00:01Z", "one"),
        )
        .await
        .unwrap();
    store
        .insert_event(
            "sess-exp",
            &test_event("e2", "sess-exp", "2025-01-14T00:00:02Z", "two"),
        )
        .await
        .unwrap();

    let jsonl = store.export_session_jsonl("sess-exp").await.unwrap();
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let v: Value = serde_json::from_str(line).expect("each line must parse as JSON");
        assert!(v.get("id").is_some());
    }
}

pub async fn it_exports_an_empty_session_as_an_empty_string(store: Arc<dyn EventStore>) {
    let jsonl = store.export_session_jsonl("nonexistent").await.unwrap();
    assert!(jsonl.is_empty());
}

// ───────────────────────────────────────────────────────────────────────
// Lifecycle conformance
// ───────────────────────────────────────────────────────────────────────

pub async fn it_deletes_a_session_and_all_its_data(store: Arc<dyn EventStore>) {
    store
        .insert_event(
            "sess-del",
            &test_event("d1", "sess-del", "2025-01-14T00:00:01Z", "one"),
        )
        .await
        .unwrap();
    store
        .insert_event(
            "sess-del",
            &test_event("d2", "sess-del", "2025-01-14T00:00:02Z", "two"),
        )
        .await
        .unwrap();
    store
        .upsert_session(&test_session_row("sess-del", Some("doomed")))
        .await
        .unwrap();
    store
        .insert_pattern(
            "sess-del",
            &test_pattern("sess-del", "test.cycle", "2025-01-14T00:00:00Z"),
        )
        .await
        .unwrap();
    store
        .upsert_plan("plan-del", "sess-del", "# Doomed plan")
        .await
        .unwrap();

    let deleted = store.delete_session("sess-del").await.unwrap();
    assert_eq!(deleted, 2, "delete_session must report event count deleted");

    assert!(store
        .session_events("sess-del")
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .list_sessions()
        .await
        .unwrap()
        .iter()
        .all(|s| s.id != "sess-del"));
    assert!(store
        .session_patterns("sess-del", None)
        .await
        .unwrap()
        .is_empty());
}

pub async fn it_only_deletes_the_targeted_session(store: Arc<dyn EventStore>) {
    store
        .insert_event(
            "sess-keep",
            &test_event("k1", "sess-keep", "2025-01-14T00:00:00Z", "keep"),
        )
        .await
        .unwrap();
    store
        .insert_event(
            "sess-go",
            &test_event("g1", "sess-go", "2025-01-14T00:00:00Z", "go"),
        )
        .await
        .unwrap();
    store
        .upsert_session(&test_session_row("sess-keep", None))
        .await
        .unwrap();
    store
        .upsert_session(&test_session_row("sess-go", None))
        .await
        .unwrap();

    store.delete_session("sess-go").await.unwrap();

    let surviving = store.session_events("sess-keep").await.unwrap();
    assert_eq!(surviving.len(), 1);
    let sessions = store.list_sessions().await.unwrap();
    assert!(sessions.iter().any(|s| s.id == "sess-keep"));
    assert!(sessions.iter().all(|s| s.id != "sess-go"));
}

pub async fn it_returns_zero_when_deleting_a_nonexistent_session(store: Arc<dyn EventStore>) {
    let n = store.delete_session("never-existed").await.unwrap();
    assert_eq!(n, 0);
}

// ───────────────────────────────────────────────────────────────────────
// FTS conformance
// ───────────────────────────────────────────────────────────────────────

pub async fn it_indexes_text_and_finds_it_via_full_text_search(store: Arc<dyn EventStore>) {
    store
        .index_fts(
            "evt-fts-1",
            "sess-fts",
            "user_message",
            "fix the authentication bug in login",
        )
        .await
        .unwrap();
    store
        .index_fts(
            "evt-fts-2",
            "sess-fts",
            "assistant_message",
            "looking at the database connection pool",
        )
        .await
        .unwrap();

    let results = store.search_fts("authentication", 10, None).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event_id, "evt-fts-1");
    assert_eq!(results[0].session_id, "sess-fts");
    assert_eq!(results[0].record_type, "user_message");
}

pub async fn it_scopes_full_text_search_to_a_session_when_asked(store: Arc<dyn EventStore>) {
    store
        .index_fts("a1", "sess-a", "user_message", "deploy the application")
        .await
        .unwrap();
    store
        .index_fts("b1", "sess-b", "user_message", "deploy the database")
        .await
        .unwrap();

    let results = store
        .search_fts("deploy", 10, Some("sess-a"))
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].session_id, "sess-a");
}

pub async fn it_returns_no_results_for_an_empty_search_query(store: Arc<dyn EventStore>) {
    store
        .index_fts("x1", "sess-x", "user_message", "hello world")
        .await
        .unwrap();
    let results = store.search_fts("", 10, None).await.unwrap();
    assert!(results.is_empty());
}

pub async fn it_returns_no_results_when_nothing_matches(store: Arc<dyn EventStore>) {
    store
        .index_fts("x1", "sess-x", "user_message", "hello world")
        .await
        .unwrap();
    let results = store.search_fts("nonexistent", 10, None).await.unwrap();
    assert!(results.is_empty());
}

pub async fn it_caps_full_text_search_results_at_the_limit(store: Arc<dyn EventStore>) {
    for i in 0..10 {
        store
            .index_fts(
                &format!("evt-{i}"),
                "sess-lim",
                "user_message",
                "common search term",
            )
            .await
            .unwrap();
    }
    let results = store.search_fts("common", 3, None).await.unwrap();
    assert_eq!(results.len(), 3);
}

pub async fn it_counts_indexed_full_text_records(store: Arc<dyn EventStore>) {
    assert_eq!(store.fts_count().await.unwrap(), 0);
    store
        .index_fts("c1", "sess-c", "user_message", "hello")
        .await
        .unwrap();
    store
        .index_fts("c2", "sess-c", "user_message", "world")
        .await
        .unwrap();
    assert_eq!(store.fts_count().await.unwrap(), 2);
}

// ───────────────────────────────────────────────────────────────────────
// Analytics fixture — the "Analytics Universe"
// ───────────────────────────────────────────────────────────────────────
//
// One shared seed function used by every analytics conformance helper.
// See §7 of `docs/research/mongo-analytics-parity-plan.md` for the
// declarative spec. Topology: 2 projects, 3 sessions, ~25 events.
//
// Timestamps are computed relative to `Utc::now()` at seed time so the
// `days` parameter on time-windowed queries always sees them. Format
// is the canonical translator format (§1.5) — byte-identical to what
// the JSONL pass-through produces in production.

/// Build an analytics-fixture event with the right shape for the
/// SQLite/Mongo path (`payload.data.agent_payload.tool` for tool_use,
/// `payload.data.agent_payload.text` for messages, etc.).
fn analytics_event(
    id: &str,
    session_id: &str,
    timestamp: &str,
    subtype: &str,
    agent_payload: Value,
) -> Value {
    json!({
        "id": id,
        "type": "io.arc.event",
        "subtype": subtype,
        "source": format!("arc://transcript/{session_id}"),
        "time": timestamp,
        "data": {
            "raw": {},
            "seq": 1,
            "session_id": session_id,
            "agent_payload": agent_payload,
        }
    })
}

/// Tool-use event helper.
fn tool_use_event(id: &str, sid: &str, ts: &str, tool: &str, file: Option<&str>) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(f) = file {
        args.insert("file_path".into(), Value::String(f.to_string()));
    }
    analytics_event(
        id,
        sid,
        ts,
        "message.assistant.tool_use",
        json!({
            "_variant": "claude-code",
            "meta": {"agent": "claude-code"},
            "tool": tool,
            "args": args,
        }),
    )
}

/// User-prompt event helper.
fn user_prompt_event(id: &str, sid: &str, ts: &str, text: &str) -> Value {
    analytics_event(
        id,
        sid,
        ts,
        "message.user.prompt",
        json!({
            "_variant": "claude-code",
            "meta": {"agent": "claude-code"},
            "text": text,
        }),
    )
}

/// System-error event helper.
fn error_event(id: &str, sid: &str, ts: &str, message: &str) -> Value {
    analytics_event(
        id,
        sid,
        ts,
        "system.error",
        json!({
            "_variant": "claude-code",
            "meta": {"agent": "claude-code"},
            "text": message,
        }),
    )
}

/// Seed the shared analytics universe into a fresh store. Idempotent
/// per-store but not safe to call twice on the same store (event ids
/// would collide and dedup would silently no-op).
///
/// The fixture is small (~25 events, 3 sessions, 2 projects) and
/// intentionally has no within-session timestamp ties — the 1-minute
/// increments give every event a distinct timestamp. Tied counts in
/// `top_tools` are allowed; the C2 canonical-sort handles them.
async fn seed_analytics_universe(store: &dyn EventStore) {
    // ── Sessions ──
    // sess-A1: proj-alpha, 12 events, started ~24h ago
    // sess-A2: proj-alpha, 8  events, started ~12h ago
    // sess-B1: proj-beta,  5  events, started ~6h  ago
    let session_rows = [
        ("sess-A1", "proj-alpha", "Alpha", "build feature X", ts_offset(24, 11), ts_offset(24, 0)),
        ("sess-A2", "proj-alpha", "Alpha", "fix auth bug",    ts_offset(12, 7),  ts_offset(12, 0)),
        ("sess-B1", "proj-beta",  "Beta",  "explore data",    ts_offset(6, 4),   ts_offset(6, 0)),
    ];
    for (id, pid, pname, label, first, last) in &session_rows {
        store
            .upsert_session(&SessionRow {
                id: id.to_string(),
                project_id: Some(pid.to_string()),
                project_name: Some(pname.to_string()),
                label: Some(label.to_string()),
                custom_label: None,
                branch: Some("main".to_string()),
                event_count: 0, // updated lazily — analytics queries don't depend on this
                first_event: Some(first.clone()),
                last_event: Some(last.clone()),
            })
            .await
            .unwrap();
    }

    // ── sess-A1 events ── 12 events: 1 prompt, 5 tool_use, 5 results, 1 error
    let a1_events = vec![
        user_prompt_event("a1-p1",  "sess-A1", &ts_offset(24, 11), "build feature X"),
        tool_use_event(   "a1-t1",  "sess-A1", &ts_offset(24, 10), "Edit", Some("src/main.rs")),
        tool_use_event(   "a1-t2",  "sess-A1", &ts_offset(24, 9),  "Edit", Some("src/main.rs")),
        tool_use_event(   "a1-t3",  "sess-A1", &ts_offset(24, 8),  "Bash", None),
        tool_use_event(   "a1-t4",  "sess-A1", &ts_offset(24, 7),  "Read", Some("src/main.rs")),
        tool_use_event(   "a1-t5",  "sess-A1", &ts_offset(24, 6),  "Read", Some("Cargo.toml")),
        analytics_event("a1-r1",  "sess-A1", &ts_offset(24, 5), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a1-r2",  "sess-A1", &ts_offset(24, 4), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a1-r3",  "sess-A1", &ts_offset(24, 3), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a1-r4",  "sess-A1", &ts_offset(24, 2), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a1-r5",  "sess-A1", &ts_offset(24, 1), "message.user.tool_result", json!({"text":"ok"})),
        error_event(      "a1-e1",  "sess-A1", &ts_offset(24, 0),  "compile error: trait bound not satisfied"),
    ];
    for e in &a1_events {
        store.insert_event("sess-A1", e).await.unwrap();
    }

    // ── sess-A2 events ── 8 events: 1 prompt, 3 tool_use, 3 results, 1 error
    let a2_events = vec![
        user_prompt_event("a2-p1",  "sess-A2", &ts_offset(12, 7),  "fix auth bug"),
        tool_use_event(   "a2-t1",  "sess-A2", &ts_offset(12, 6),  "Read", Some("tests/auth.rs")),
        tool_use_event(   "a2-t2",  "sess-A2", &ts_offset(12, 5),  "Read", Some("tests/auth.rs")),
        tool_use_event(   "a2-t3",  "sess-A2", &ts_offset(12, 4),  "Bash", None),
        analytics_event("a2-r1",  "sess-A2", &ts_offset(12, 3), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a2-r2",  "sess-A2", &ts_offset(12, 2), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("a2-r3",  "sess-A2", &ts_offset(12, 1), "message.user.tool_result", json!({"text":"ok"})),
        error_event(      "a2-e1",  "sess-A2", &ts_offset(12, 0),  "test failure: assertion left != right"),
    ];
    for e in &a2_events {
        store.insert_event("sess-A2", e).await.unwrap();
    }

    // ── sess-B1 events ── 5 events: 1 prompt, 2 tool_use, 2 results
    let b1_events = vec![
        user_prompt_event("b1-p1",  "sess-B1", &ts_offset(6, 4),  "explore data"),
        tool_use_event(   "b1-t1",  "sess-B1", &ts_offset(6, 3),  "Grep", Some("data/raw/")),
        tool_use_event(   "b1-t2",  "sess-B1", &ts_offset(6, 2),  "Glob", Some("data/raw/*.csv")),
        analytics_event("b1-r1",  "sess-B1", &ts_offset(6, 1), "message.user.tool_result", json!({"text":"ok"})),
        analytics_event("b1-r2",  "sess-B1", &ts_offset(6, 0), "message.user.tool_result", json!({"text":"ok"})),
    ];
    for e in &b1_events {
        store.insert_event("sess-B1", e).await.unwrap();
    }
}

// ───────────────────────────────────────────────────────────────────────
// Analytics conformance helpers
// ───────────────────────────────────────────────────────────────────────
//
// Each helper is tagged with its §1.6 parity category:
//   C1 — strict assert_eq! (math is well-defined)
//   C2 — canonical-sort then assert_eq! (tie order is implementation-defined)
//   C3 — API redesign required (cosmetic field doing too much work)
//
// All helpers seed the analytics universe at the start; the seed is
// idempotent per-store but not safe to call twice on the same store.

/// §8.10 — `query_project_context` returns recent sessions for a
/// project, ordered by `last_event DESC`. Distinct `last_event` values
/// in the fixture make the order unambiguous → C1 strict equality.
pub async fn it_returns_project_context_recent_sessions(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_project_context("proj-alpha", 5).await;

    // proj-alpha has 2 sessions (sess-A1, sess-A2). sess-A2 was created
    // ~12h ago and sess-A1 ~24h ago, so A2 is more recent → first.
    assert_eq!(result.len(), 2, "proj-alpha has 2 sessions");
    assert_eq!(result[0].session_id, "sess-A2", "most recent first");
    assert_eq!(result[0].label.as_deref(), Some("fix auth bug"));
    assert_eq!(result[1].session_id, "sess-A1");
    assert_eq!(result[1].label.as_deref(), Some("build feature X"));
}

/// §8.11 — `query_project_context` is scoped to the requested project.
/// Querying for `proj-beta` must NOT return any `proj-alpha` sessions.
pub async fn it_scopes_project_context_to_the_project(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_project_context("proj-beta", 5).await;

    assert_eq!(result.len(), 1, "proj-beta has exactly 1 session");
    assert_eq!(result[0].session_id, "sess-B1");
    assert_eq!(result[0].label.as_deref(), Some("explore data"));
}

/// §8.7 — `query_project_pulse` aggregates by project across the time
/// window, sorted by `total_events DESC`. C1 because counts are
/// mathematically defined.
pub async fn it_returns_project_pulse_grouped_by_project(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    // Note: project_pulse uses `SUM(s.event_count)` from the sessions
    // table — but our fixture leaves event_count at 0 (analytics
    // queries don't depend on it for THIS query, but project_pulse
    // does). Update event_count via re-upsert before querying.
    let updates = [
        ("sess-A1", "proj-alpha", "Alpha", "build feature X", ts_offset(24, 11), ts_offset(24, 0), 12u64),
        ("sess-A2", "proj-alpha", "Alpha", "fix auth bug",    ts_offset(12, 7),  ts_offset(12, 0), 8u64),
        ("sess-B1", "proj-beta",  "Beta",  "explore data",    ts_offset(6, 4),   ts_offset(6, 0),  5u64),
    ];
    for (id, pid, pname, label, first, last, count) in &updates {
        store
            .upsert_session(&SessionRow {
                id: id.to_string(),
                project_id: Some(pid.to_string()),
                project_name: Some(pname.to_string()),
                label: Some(label.to_string()),
                custom_label: None,
                branch: Some("main".to_string()),
                event_count: *count,
                first_event: Some(first.clone()),
                last_event: Some(last.clone()),
            })
            .await
            .unwrap();
    }

    // generous days window — fixtures are within 24h of now
    let result = store.query_project_pulse(365).await;

    assert_eq!(result.len(), 2, "two projects with last_event in window");
    // proj-alpha: 2 sessions, 12+8=20 events → first by total_events DESC
    assert_eq!(result[0].project_id, "proj-alpha");
    assert_eq!(result[0].project_name.as_deref(), Some("Alpha"));
    assert_eq!(result[0].session_count, 2);
    assert_eq!(result[0].event_count, 20);
    // proj-beta: 1 session, 5 events
    assert_eq!(result[1].project_id, "proj-beta");
    assert_eq!(result[1].session_count, 1);
    assert_eq!(result[1].event_count, 5);
}

/// §8.6 — `query_session_errors` returns errors in `timestamp ASC`
/// order. Distinct timestamps → C1 strict equality.
pub async fn it_returns_session_errors_in_timestamp_order(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_session_errors("sess-A1").await;

    assert_eq!(result.len(), 1, "sess-A1 has 1 error event");
    assert_eq!(result[0].message, "compile error: trait bound not satisfied");

    let result_a2 = store.query_session_errors("sess-A2").await;
    assert_eq!(result_a2.len(), 1, "sess-A2 has 1 error event");
    assert_eq!(result_a2[0].message, "test failure: assertion left != right");

    let result_b1 = store.query_session_errors("sess-B1").await;
    assert!(result_b1.is_empty(), "sess-B1 has no error events");
}

/// §8.1 — `query_session_synopsis` returns combined session metadata +
/// counts + top tools. C1 for the metadata/counts, C2 for top_tools
/// (canonical-sort by `(count DESC, tool ASC)`).
pub async fn it_returns_a_session_synopsis(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_session_synopsis("sess-A1").await;
    let synopsis = result.expect("sess-A1 must have a synopsis");

    // C1 — metadata
    assert_eq!(synopsis.session_id, "sess-A1");
    assert_eq!(synopsis.label.as_deref(), Some("build feature X"));
    assert_eq!(synopsis.project_id.as_deref(), Some("proj-alpha"));
    assert_eq!(synopsis.project_name.as_deref(), Some("Alpha"));

    // C1 — counts: 5 tool_use events, 1 system.error event
    assert_eq!(synopsis.tool_count, 5, "5 tool_use events in sess-A1");
    assert_eq!(synopsis.error_count, 1, "1 system.error in sess-A1");

    // C2 — top tools by count.
    // sess-A1 tool distribution: Edit ×2, Bash ×1, Read ×2.
    // Canonical sort: (count DESC, tool ASC).
    // Both Edit (2) and Read (2) tie at count=2 → sort by tool name
    // → [Edit(2), Read(2), Bash(1)].
    let mut canonical = synopsis.top_tools.clone();
    canonical.sort_by(|a, b| (b.count, &a.tool).cmp(&(a.count, &b.tool)));
    assert_eq!(canonical.len(), 3, "3 distinct tools in sess-A1");
    assert_eq!(canonical[0], ToolCount { tool: "Edit".into(), count: 2 });
    assert_eq!(canonical[1], ToolCount { tool: "Read".into(), count: 2 });
    assert_eq!(canonical[2], ToolCount { tool: "Bash".into(), count: 1 });
}

/// §8.2 — `query_session_synopsis` returns None for an unknown session.
pub async fn it_returns_none_for_unknown_session_synopsis(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;
    let result = store.query_session_synopsis("does-not-exist").await;
    assert!(result.is_none(), "unknown session must return None");
}

/// §8.3 — `query_tool_journey` returns the sequence of tool calls in
/// timestamp ASC order. C1 strict equality (timestamps are distinct).
pub async fn it_returns_tool_journey_in_timestamp_order(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_tool_journey("sess-A1").await;
    let tools: Vec<&str> = result.iter().map(|s| s.tool.as_str()).collect();
    let files: Vec<Option<&str>> = result.iter().map(|s| s.file.as_deref()).collect();

    // sess-A1 tool_use events in timestamp order:
    // a1-t1: Edit src/main.rs
    // a1-t2: Edit src/main.rs
    // a1-t3: Bash (no file)
    // a1-t4: Read src/main.rs
    // a1-t5: Read Cargo.toml
    assert_eq!(tools, vec!["Edit", "Edit", "Bash", "Read", "Read"]);
    assert_eq!(
        files,
        vec![
            Some("src/main.rs"),
            Some("src/main.rs"),
            None,
            Some("src/main.rs"),
            Some("Cargo.toml"),
        ]
    );
}

/// §8.4 — `query_tool_journey` for an unknown session returns empty.
pub async fn it_returns_tool_journey_empty_for_unknown_session(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;
    let result = store.query_tool_journey("does-not-exist").await;
    assert!(result.is_empty());
}

/// §8.9 — `query_session_efficiency` returns the most recent 50
/// sessions with tool/error counts. The SQL query uses
/// `ORDER BY last_event DESC LIMIT 50`, but the `SessionEfficiency`
/// struct doesn't carry `last_event` — so the Vec ordering is opaque
/// at the API surface and we test on SET membership instead.
/// C2 with canonical sort by `session_id` ASC.
pub async fn it_returns_session_efficiency_for_recent_sessions(
    store: Arc<dyn EventStore>,
) {
    seed_analytics_universe(&*store).await;

    let result = store.query_session_efficiency().await;

    // Canonical sort by session_id so any backend-specific Vec
    // ordering is normalized away.
    let mut canonical = result.clone();
    canonical.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // Three sessions in fixture.
    assert_eq!(canonical.len(), 3, "3 sessions in fixture");

    assert_eq!(canonical[0].session_id, "sess-A1");
    assert_eq!(canonical[0].label.as_deref(), Some("build feature X"));
    assert_eq!(canonical[0].tool_count, 5, "sess-A1 has 5 tool_use events");
    assert_eq!(canonical[0].error_count, 1);

    assert_eq!(canonical[1].session_id, "sess-A2");
    assert_eq!(canonical[1].label.as_deref(), Some("fix auth bug"));
    assert_eq!(canonical[1].tool_count, 3, "sess-A2 has 3 tool_use events");
    assert_eq!(canonical[1].error_count, 1);

    assert_eq!(canonical[2].session_id, "sess-B1");
    assert_eq!(canonical[2].label.as_deref(), Some("explore data"));
    assert_eq!(canonical[2].tool_count, 2, "sess-B1 has 2 tool_use events");
    assert_eq!(canonical[2].error_count, 0);
}

/// §8.12 — `query_recent_files` returns distinct files modified
/// (Edit/Write/NotebookEdit) in a project's sessions, most-recent
/// first. C2 — same-timestamp ties have implementation-defined order,
/// so the helper sorts both outputs alphabetically before comparing
/// set membership.
pub async fn it_returns_recent_files_for_a_project(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_recent_files("proj-alpha", 5).await;

    // Edit/Write events in proj-alpha sessions (sess-A1 + sess-A2):
    //   sess-A1: Edit src/main.rs (×2)
    //   sess-A2: (no edits — only Reads)
    // Distinct files modified: ["src/main.rs"]
    let mut canonical = result.clone();
    canonical.sort();
    assert_eq!(canonical, vec!["src/main.rs".to_string()]);

    // Scoping: proj-beta has no edit events at all → empty result
    let beta = store.query_recent_files("proj-beta", 5).await;
    assert!(beta.is_empty(), "proj-beta has no edit events");
}

/// §8.13 — `query_productivity_by_hour` buckets events by UTC hour.
/// Both backends compute the same hour from the same `Z`-suffixed
/// source data, so this is C1 strict equality on the bucket counts —
/// but the EXACT hour values depend on `Utc::now()` at test time.
/// The assertion compares the result Vec from each backend to the
/// expected Vec computed from the same `chrono` ops at fixture seed
/// time.
pub async fn it_buckets_productivity_by_hour(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_productivity_by_hour(365).await;

    // The fixture has events at three anchor offsets: 24h ago, 12h
    // ago, 6h ago. Each anchor produces ~12, 8, 5 events at one
    // distinct hour (within the 1-minute spread). Total = 25 events.
    let total: u64 = result.iter().map(|h| h.event_count).sum();
    assert_eq!(total, 25, "25 events total in the fixture");

    // Hours are 0..=23
    for h in &result {
        assert!(h.hour < 24, "hour bucket out of range: {}", h.hour);
    }

    // Buckets sorted by hour ASC (the SQL impl uses ORDER BY hour)
    let mut sorted = result.clone();
    sorted.sort_by_key(|h| h.hour);
    assert_eq!(sorted, result, "result must be ordered by hour ASC");

    // The fixture has events at exactly 3 anchors (24h, 12h, 6h ago)
    // so we expect at most 3 distinct hour buckets. They MAY collapse
    // into fewer buckets if two anchors land in the same UTC hour
    // (e.g., test runs at xx:30 and 6h ago is xx:30 of the previous
    // hour-aligned bucket). The total events stays 25 either way.
    assert!(
        result.len() <= 3,
        "fixture spans at most 3 distinct hours, got {}",
        result.len()
    );
    assert!(!result.is_empty(), "must have at least one bucket");
}

/// §8.5 — `query_file_impact` returns files with read/write counts
/// per file, ordered by `(reads + writes) DESC`. The Rust-side post
/// sort makes the order deterministic → C1.
pub async fn it_returns_file_impact_with_reads_and_writes(store: Arc<dyn EventStore>) {
    seed_analytics_universe(&*store).await;

    let result = store.query_file_impact("sess-A1").await;

    // sess-A1 tool calls touching files:
    // src/main.rs: Edit×2 (writes=2), Read×1 (reads=1) → total=3
    // Cargo.toml:  Read×1 (reads=1)                   → total=1
    // (Bash without a file is excluded)
    //
    // Sorted by (reads+writes) DESC:
    // 1. src/main.rs (3)
    // 2. Cargo.toml  (1)
    assert_eq!(result.len(), 2, "2 files touched in sess-A1");
    assert_eq!(result[0].file, "src/main.rs");
    assert_eq!(result[0].reads, 1);
    assert_eq!(result[0].writes, 2);
    assert_eq!(result[1].file, "Cargo.toml");
    assert_eq!(result[1].reads, 1);
    assert_eq!(result[1].writes, 0);
}

// ───────────────────────────────────────────────────────────────────────
// Backend wrappers
// ───────────────────────────────────────────────────────────────────────
//
// Each backend mod creates a fresh store and runs every helper above as
// its own #[tokio::test]. When MongoStore lands, add a parallel `mod
// mongo_backend` with the same shape — every test must pass against
// both backends or the trait contract is wrong.

/// All conformance test names — single source of truth for both backends.
///
/// Adding a new helper above? Add it here too. The macro then expands the
/// list once per backend module, so the SQLite and Mongo wrappers can never
/// drift apart.
macro_rules! for_each_conformance_test {
    ($macro:ident) => {
        // Writes
        $macro!(it_inserts_a_new_event_and_returns_true);
        $macro!(it_returns_false_when_inserting_a_duplicate_event);
        $macro!(it_treats_event_id_as_a_global_primary_key);
        $macro!(it_reports_the_count_of_new_events_in_a_batch);
        $macro!(it_dedupes_a_batch_against_already_stored_events);
        $macro!(it_handles_an_empty_batch);
        $macro!(it_upserts_a_session_and_lists_it);
        $macro!(it_updates_an_existing_session_on_upsert);
        $macro!(it_never_overwrites_a_user_set_custom_label);
        $macro!(it_persists_and_queries_a_detected_pattern);
        $macro!(it_filters_session_patterns_by_type);
        $macro!(it_persists_and_queries_a_structural_turn);
        $macro!(it_upserts_a_plan_idempotently);
        // Reads
        $macro!(it_returns_session_events_ordered_by_timestamp);
        $macro!(it_round_trips_an_event_payload_losslessly);
        $macro!(it_returns_no_events_for_an_unknown_session);
        $macro!(it_starts_with_an_empty_session_list);
        $macro!(it_returns_the_full_payload_for_a_known_event);
        $macro!(it_returns_none_for_an_unknown_event_payload);
        $macro!(it_exports_a_session_as_newline_delimited_json);
        $macro!(it_exports_an_empty_session_as_an_empty_string);
        // Lifecycle
        $macro!(it_deletes_a_session_and_all_its_data);
        $macro!(it_only_deletes_the_targeted_session);
        $macro!(it_returns_zero_when_deleting_a_nonexistent_session);
        // FTS
        $macro!(it_indexes_text_and_finds_it_via_full_text_search);
        $macro!(it_scopes_full_text_search_to_a_session_when_asked);
        $macro!(it_returns_no_results_for_an_empty_search_query);
        $macro!(it_returns_no_results_when_nothing_matches);
        $macro!(it_caps_full_text_search_results_at_the_limit);
        $macro!(it_counts_indexed_full_text_records);
        // Analytics — Phase 5
        $macro!(it_returns_project_context_recent_sessions);
        $macro!(it_scopes_project_context_to_the_project);
        $macro!(it_returns_project_pulse_grouped_by_project);
        $macro!(it_returns_session_errors_in_timestamp_order);
        $macro!(it_returns_a_session_synopsis);
        $macro!(it_returns_none_for_unknown_session_synopsis);
        $macro!(it_returns_tool_journey_in_timestamp_order);
        $macro!(it_returns_tool_journey_empty_for_unknown_session);
        $macro!(it_returns_file_impact_with_reads_and_writes);
        $macro!(it_returns_session_efficiency_for_recent_sessions);
        $macro!(it_returns_recent_files_for_a_project);
        $macro!(it_buckets_productivity_by_hour);
    };
}

mod sqlite_backend {
    use super::*;
    use open_story_store::sqlite_store::SqliteStore;

    fn make_store() -> Arc<dyn EventStore> {
        Arc::new(SqliteStore::in_memory().expect("create in-memory sqlite store"))
    }

    macro_rules! sqlite_conformance_test {
        ($name:ident) => {
            #[tokio::test]
            async fn $name() {
                super::$name(make_store()).await;
            }
        };
    }

    for_each_conformance_test!(sqlite_conformance_test);
}

/// MongoDB backend wrapper. Spins up a fresh `mongo:7` testcontainer per
/// test (cheap — Mongo boots in ~1s) and runs the same conformance helpers
/// against `MongoStore`. Gated behind the `mongo` feature so the suite
/// doesn't require Docker for users who only want SQLite.
///
/// Run with: `cargo test -p open-story-store --features mongo --test event_store_conformance`
#[cfg(feature = "mongo")]
mod mongo_backend {
    use super::*;
    use open_story_store::mongo_store::MongoStore;
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage};

    /// A running mongo container — kept alive for the duration of the test.
    /// Drop = container teardown via testcontainers.
    struct MongoFixture {
        _container: ContainerAsync<GenericImage>,
        store: Arc<dyn EventStore>,
    }

    async fn start_mongo() -> MongoFixture {
        let container = GenericImage::new("mongo", "7")
            .with_exposed_port(ContainerPort::Tcp(27017))
            .with_wait_for(WaitFor::message_on_stdout("Waiting for connections"))
            .start()
            .await
            .expect("start mongo:7 testcontainer (is Docker running?)");

        let host = container
            .get_host()
            .await
            .expect("mongo container host");
        let port = container
            .get_host_port_ipv4(27017)
            .await
            .expect("mongo container port");
        let uri = format!("mongodb://{host}:{port}");

        // Each test gets its own database name so collections never collide.
        // Mongo creates databases lazily on first write — no setup needed.
        let db_name = format!("openstory_conformance_{}", uuid::Uuid::new_v4().simple());
        let store: Arc<dyn EventStore> = Arc::new(
            MongoStore::connect(&uri, &db_name)
                .await
                .expect("connect MongoStore"),
        );
        MongoFixture {
            _container: container,
            store,
        }
    }

    macro_rules! mongo_conformance_test {
        ($name:ident) => {
            #[tokio::test]
            async fn $name() {
                let fixture = start_mongo().await;
                super::$name(fixture.store.clone()).await;
            }
        };
    }

    for_each_conformance_test!(mongo_conformance_test);
}
