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
