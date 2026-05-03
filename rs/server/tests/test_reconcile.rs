//! Integration tests for the boot-time Reconciler (Stratum 2 of the test
//! plan in `~/.claude/plans/ticklish-rolling-kazoo.md`).
//!
//! Each test uses a `tempfile::tempdir` for `data_dir`, builds a real
//! `StoreState` backed by SQLite, writes JSONL fixtures with realistic
//! CloudEvent shapes, and asserts on `EventStore` state after one or
//! more `reconcile_local` invocations.

use std::fs;

use serde_json::{json, Value};
use tempfile::tempdir;

use open_story_server::reconcile::reconcile_local;
use open_story_store::state::StoreState;

/// Build a CloudEvent-shaped JSON value with the fields the reconciler reads.
fn ce(id: &str, time: &str, host: Option<&str>, user: Option<&str>) -> Value {
    let mut v = json!({
        "specversion": "1.0",
        "id": id,
        "type": "io.arc.event",
        "subtype": "message.user.prompt",
        "source": "arc://transcript/test",
        "time": time,
        "datacontenttype": "application/json",
        "data": { "agent_payload": { "_variant": "claude-code" }, "raw": {}, "seq": 0, "session_id": "test" },
    });
    if let Some(h) = host {
        v["host"] = json!(h);
    }
    if let Some(u) = user {
        v["user"] = json!(u);
    }
    v
}

/// Write `events` as one-line-per-event JSONL into `<data_dir>/<sid>.jsonl`.
fn write_jsonl(data_dir: &std::path::Path, sid: &str, events: &[Value]) {
    fs::create_dir_all(data_dir).unwrap();
    let path = data_dir.join(format!("{sid}.jsonl"));
    let mut content = String::new();
    for e in events {
        content.push_str(&serde_json::to_string(e).unwrap());
        content.push('\n');
    }
    fs::write(path, content).unwrap();
}

#[tokio::test]
async fn reconcile_empty_data_dir_is_noop() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    assert_eq!(report.files_walked, 0);
    assert_eq!(report.events_inserted, 0);
    assert_eq!(report.events_skipped, 0);
    assert_eq!(report.sessions_upserted, 0);
    assert!(report.errors.is_empty());
    assert!(!report.did_work());

    // Store remains empty — the reconciler did not introduce sentinel rows.
    let sessions = store.event_store.list_sessions().await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn reconcile_populates_empty_store_from_jsonl() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    write_jsonl(data_dir, "sess-katie", &[
        ce("evt-1", "2026-05-01T10:00:00Z", Some("Katies-Mac-mini"), Some("katie")),
        ce("evt-2", "2026-05-01T10:00:01Z", Some("Katies-Mac-mini"), Some("katie")),
        ce("evt-3", "2026-05-01T10:05:00Z", Some("Katies-Mac-mini"), Some("katie")),
    ]);
    write_jsonl(data_dir, "sess-max", &[
        ce("evt-4", "2026-05-01T11:00:00Z", Some("Maxs-Air"), Some("max")),
    ]);

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    assert_eq!(report.files_walked, 2);
    assert_eq!(report.events_inserted, 4);
    assert_eq!(report.events_skipped, 0);
    assert_eq!(report.sessions_upserted, 2);
    assert!(report.errors.is_empty());

    let sessions = store.event_store.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 2);

    let katie = sessions.iter().find(|r| r.id == "sess-katie").expect("katie session");
    assert_eq!(katie.host.as_deref(), Some("Katies-Mac-mini"));
    assert_eq!(katie.user.as_deref(), Some("katie"));
    assert_eq!(katie.event_count, 3);
    assert_eq!(katie.first_event.as_deref(), Some("2026-05-01T10:00:00Z"));
    assert_eq!(katie.last_event.as_deref(), Some("2026-05-01T10:05:00Z"));

    let max = sessions.iter().find(|r| r.id == "sess-max").expect("max session");
    assert_eq!(max.host.as_deref(), Some("Maxs-Air"));
    assert_eq!(max.user.as_deref(), Some("max"));
    assert_eq!(max.event_count, 1);
}

#[tokio::test]
async fn reconcile_is_idempotent() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    write_jsonl(data_dir, "sess-1", &[
        ce("a", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
        ce("b", "2026-05-01T10:00:01Z", Some("h"), Some("u")),
    ]);

    let r1 = reconcile_local(data_dir, &mut store).await.unwrap();
    assert_eq!(r1.events_inserted, 2);
    assert_eq!(r1.events_skipped, 0);

    // Second run — every event is already present.
    let r2 = reconcile_local(data_dir, &mut store).await.unwrap();
    assert_eq!(r2.events_inserted, 0, "second run should insert nothing");
    assert_eq!(r2.events_skipped, 2, "second run should skip both events via PK dedup");
    assert_eq!(r2.sessions_upserted, 1);
    assert!(r2.errors.is_empty());

    // State unchanged: still one session with 2 events.
    let sessions = store.event_store.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].event_count, 2);
}

#[tokio::test]
async fn reconcile_partial_drift_heals() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    // Pre-seed the store with one event of session sess-1 — simulating a
    // run where the persist consumer wrote that event but later events
    // were stranded in JSONL only.
    let e1 = ce("evt-1", "2026-05-01T10:00:00Z", Some("h"), Some("u"));
    store.event_store.insert_event("sess-1", &e1).await.unwrap();

    // JSONL has all three events (the disk-truth view).
    write_jsonl(data_dir, "sess-1", &[
        e1.clone(),
        ce("evt-2", "2026-05-01T10:00:01Z", Some("h"), Some("u")),
        ce("evt-3", "2026-05-01T10:00:02Z", Some("h"), Some("u")),
    ]);

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    // Only the two missing events should be inserted; the pre-existing one
    // is skipped via PK dedup.
    assert_eq!(report.events_inserted, 2);
    assert_eq!(report.events_skipped, 1);
    assert_eq!(report.sessions_upserted, 1);

    let events = store.event_store.session_events("sess-1").await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn reconcile_handles_corrupt_jsonl_line() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    // Hand-craft a JSONL with a corrupt line in the middle — the
    // SessionStore's load_session uses `serde_json::from_str` per line
    // and silently drops malformed lines, so the reconciler should still
    // process the valid ones.
    let path = data_dir.join("sess-corrupt.jsonl");
    fs::create_dir_all(data_dir).unwrap();
    let mut content = String::new();
    content.push_str(&serde_json::to_string(&ce("a", "2026-05-01T10:00:00Z", Some("h"), Some("u"))).unwrap());
    content.push('\n');
    content.push_str("{ this is not valid JSON ::: ]]\n");
    content.push_str(&serde_json::to_string(&ce("b", "2026-05-01T10:00:01Z", Some("h"), Some("u"))).unwrap());
    content.push('\n');
    fs::write(&path, content).unwrap();

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    // The malformed line was dropped at parse time → we see 2 valid events.
    assert_eq!(report.events_inserted, 2);
    assert_eq!(report.sessions_upserted, 1);

    let events = store.event_store.session_events("sess-corrupt").await.unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn reconcile_preserves_existing_custom_label() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    // Populate the store via the reconciler first.
    write_jsonl(data_dir, "sess-1", &[
        ce("evt-1", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
    ]);
    reconcile_local(data_dir, &mut store).await.unwrap();

    // The user gives the session a custom label via the dedicated API path.
    store
        .event_store
        .update_session_label("sess-1", "User-Picked Name")
        .await
        .unwrap();

    // Append a new event to JSONL and reconcile again — the reconciler
    // upserts the session row but must not blank out the custom label.
    write_jsonl(data_dir, "sess-1", &[
        ce("evt-1", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
        ce("evt-2", "2026-05-01T10:00:01Z", Some("h"), Some("u")),
    ]);
    reconcile_local(data_dir, &mut store).await.unwrap();

    let sessions = store.event_store.list_sessions().await.unwrap();
    let row = sessions.iter().find(|r| r.id == "sess-1").expect("sess-1");
    assert_eq!(
        row.custom_label.as_deref(),
        Some("User-Picked Name"),
        "reconciler must not overwrite custom_label"
    );
    assert_eq!(row.event_count, 2);
}

#[tokio::test]
async fn reconcile_session_row_does_not_regress_frontier() {
    use open_story_store::event_store::SessionRow;

    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    // Pre-seed the store with a session row whose frontier is *ahead* of
    // what JSONL knows — simulating a live persist consumer that bumped
    // event_count + last_event from a NATS event the reconciler hasn't
    // yet written to JSONL.
    let advanced_row = SessionRow {
        id: "sess-1".to_string(),
        project_id: Some("p1".to_string()),
        project_name: Some("Project One".to_string()),
        label: Some("Big Label".to_string()),
        custom_label: None,
        branch: Some("main".to_string()),
        event_count: 100,
        first_event: Some("2026-05-01T10:00:00Z".to_string()),
        last_event: Some("2026-05-01T20:00:00Z".to_string()),
        host: Some("h".to_string()),
        user: Some("u".to_string()),
    };
    store.event_store.upsert_session(&advanced_row).await.unwrap();

    // JSONL has only a stale snapshot — 3 events earlier in the day.
    write_jsonl(data_dir, "sess-1", &[
        ce("evt-1", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
        ce("evt-2", "2026-05-01T10:00:01Z", Some("h"), Some("u")),
        ce("evt-3", "2026-05-01T10:00:02Z", Some("h"), Some("u")),
    ]);

    reconcile_local(data_dir, &mut store).await.unwrap();

    let sessions = store.event_store.list_sessions().await.unwrap();
    let row = sessions.iter().find(|r| r.id == "sess-1").expect("sess-1");

    // event_count: MAX(100, 3) → 100 (NOT regressed to 3).
    assert_eq!(row.event_count, 100, "event_count must not regress");
    // last_event: MAX preserves the later timestamp.
    assert_eq!(row.last_event.as_deref(), Some("2026-05-01T20:00:00Z"),
        "last_event must not regress");
    // first_event: MIN — the stale snapshot's earlier first matches what
    // was already there; either way the same value wins.
    assert_eq!(row.first_event.as_deref(), Some("2026-05-01T10:00:00Z"));
    // COALESCE-protected nullable fields kept their pre-existing values
    // — the reconciler's `None` did not blank them.
    assert_eq!(row.project_id.as_deref(), Some("p1"));
    assert_eq!(row.project_name.as_deref(), Some("Project One"));
    assert_eq!(row.label.as_deref(), Some("Big Label"));
    assert_eq!(row.branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn reconcile_reports_accurate_counts() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    write_jsonl(data_dir, "sess-1", &[
        ce("a", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
        ce("b", "2026-05-01T10:00:01Z", Some("h"), Some("u")),
    ]);
    write_jsonl(data_dir, "sess-2", &[
        ce("c", "2026-05-01T11:00:00Z", Some("h"), Some("u")),
    ]);
    write_jsonl(data_dir, "sess-3", &[
        ce("d", "2026-05-01T12:00:00Z", Some("h"), Some("u")),
        ce("e", "2026-05-01T12:00:01Z", Some("h"), Some("u")),
        ce("f", "2026-05-01T12:00:02Z", Some("h"), Some("u")),
    ]);

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    assert_eq!(report.files_walked, 3);
    assert_eq!(
        report.events_inserted + report.events_skipped,
        6,
        "every JSONL event must be either inserted or skipped"
    );
    assert_eq!(report.events_inserted, 6);
    assert_eq!(report.events_skipped, 0);
    assert_eq!(report.sessions_upserted, report.files_walked);
    assert!(report.errors.is_empty());
    assert!(report.elapsed.as_nanos() > 0);
}

#[tokio::test]
async fn reconcile_skips_empty_jsonl_files() {
    let tmp = tempdir().unwrap();
    let data_dir = tmp.path();
    let mut store = StoreState::new(data_dir).unwrap();

    // Write an empty .jsonl file alongside a real one.
    fs::create_dir_all(data_dir).unwrap();
    fs::write(data_dir.join("sess-empty.jsonl"), "").unwrap();
    write_jsonl(data_dir, "sess-real", &[
        ce("a", "2026-05-01T10:00:00Z", Some("h"), Some("u")),
    ]);

    let report = reconcile_local(data_dir, &mut store).await.unwrap();

    // The empty file is listed by SessionStore::list_sessions but yields
    // zero events, so the reconciler skips it (no upsert, no insert).
    assert_eq!(report.events_inserted, 1);
    assert_eq!(report.sessions_upserted, 1);
    assert_eq!(report.files_walked, 1);

    let sessions = store.event_store.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "sess-real");
}
