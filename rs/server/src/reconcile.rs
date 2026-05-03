//! Boot-time reconciler — ensures `EventStore` contains every event in JSONL.
//!
//! The reconciler is the v1 implementation of CONSTELLATION R1
//! (`docs/research/CONSTELLATION.md`). Walks `data_dir/*.jsonl`, replays
//! every line through `event_store.insert_event` (PK-deduped, idempotent),
//! then upserts a `SessionRow` derived from the events.
//!
//! ## Properties
//!
//! - **Idempotent.** Running twice is the same as running once: the second
//!   run reports zero new inserts. PK dedup makes the operation safe to
//!   call unconditionally.
//! - **No-op on empty.** A fresh contributor with no JSONL files sees a
//!   zero-line walk in single-digit milliseconds — indistinguishable from
//!   the previous boot path.
//! - **Local only.** Reads from `data_dir`, writes to whatever EventStore
//!   the `StoreState` is configured for. No network I/O. No NATS interaction.
//!   No effect on JetStream durable consumer position.
//! - **Strictly additive.** Cannot remove or modify existing events. Cannot
//!   regress monotone session-row fields (`event_count`, `first_event`,
//!   `last_event`) — those use MAX/MIN semantics in the upsert path.
//!   Cannot blank out nullable string fields (`label`, `branch`,
//!   `project_id`, `project_name`, `host`, `user`) — those use
//!   COALESCE-style protection.
//!
//! ## Invocation points
//!
//! - `state::create_state` calls this immediately after
//!   `StoreState::with_backend` returns, before any consumer subscribes
//!   to NATS. The boot path is sequential — reconciliation completes
//!   before live ingest starts, so there is no race with the persist
//!   consumer.
//! - The `open-story reconcile` CLI subcommand calls this for explicit
//!   operator invocation (e.g. after manually copying JSONL from another
//!   machine, or after a backend switch when the operator does not want
//!   to wait for the next restart).

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use open_story_store::event_store::SessionRow;
use open_story_store::persistence::SessionStore;
use open_story_store::state::StoreState;

/// Summary of one reconciliation pass.
///
/// Per-session errors are recorded in `errors` and processing continues;
/// the function only returns `Err` for setup-level failures (e.g.
/// inability to open `data_dir`).
#[derive(Debug, Default, Clone)]
pub struct ReconcileReport {
    pub files_walked: usize,
    pub events_inserted: usize,
    pub events_skipped: usize,
    pub sessions_upserted: usize,
    pub errors: Vec<String>,
    pub elapsed: Duration,
}

impl ReconcileReport {
    /// True if the reconciler made any changes to the EventStore.
    pub fn did_work(&self) -> bool {
        self.events_inserted > 0 || self.sessions_upserted > 0
    }
}

/// Walk `data_dir/*.jsonl` and ensure every event is present in
/// `store.event_store`.
///
/// See module docs for invariants. Errors on individual events/sessions
/// are recorded in `report.errors` and processing continues.
pub async fn reconcile_local(
    data_dir: &Path,
    store: &mut StoreState,
) -> Result<ReconcileReport> {
    let start = Instant::now();
    let mut report = ReconcileReport::default();

    let session_store = SessionStore::new(data_dir)?;
    let session_ids = session_store.list_sessions();

    for sid in session_ids {
        let events = session_store.load_session(&sid);
        if events.is_empty() {
            continue;
        }

        for event in &events {
            match store.event_store.insert_event(&sid, event).await {
                Ok(true) => report.events_inserted += 1,
                Ok(false) => report.events_skipped += 1,
                Err(e) => report.errors.push(format!("{sid}: insert: {e}")),
            }
        }

        let row = session_row_from_events(&sid, &events);
        match store.event_store.upsert_session(&row).await {
            Ok(()) => report.sessions_upserted += 1,
            Err(e) => report.errors.push(format!("{sid}: upsert: {e}")),
        }
        report.files_walked += 1;
    }

    report.elapsed = start.elapsed();
    Ok(report)
}

/// Build a `SessionRow` from a slice of CloudEvent JSON values.
///
/// Derives:
/// - `event_count` from the slice length.
/// - `first_event` / `last_event` from MIN/MAX of the `time` fields
///   (does not assume the slice is sorted).
/// - `host` / `user` from the first event that carries each (forward
///   scan — events without these fields are skipped).
///
/// Leaves `project_id`, `project_name`, `label`, `branch`, `custom_label`
/// as `None`. The persist consumer fills these from the projection
/// snapshot during live ingest; the COALESCE-style upsert preserves any
/// existing values rather than blanking them out, so this reconciler's
/// `None` for those fields is the right thing.
fn session_row_from_events(session_id: &str, events: &[Value]) -> SessionRow {
    let event_count = events.len() as u64;

    // first_event = MIN(time), last_event = MAX(time) — scan, don't assume order.
    let mut first_event: Option<String> = None;
    let mut last_event: Option<String> = None;
    for e in events {
        if let Some(t) = e.get("time").and_then(|v| v.as_str()) {
            first_event = match first_event {
                Some(curr) if curr.as_str() <= t => Some(curr),
                _ => Some(t.to_string()),
            };
            last_event = match last_event {
                Some(curr) if curr.as_str() >= t => Some(curr),
                _ => Some(t.to_string()),
            };
        }
    }

    // host / user — first non-None encountered (forward scan).
    let host = events
        .iter()
        .find_map(|e| {
            e.get("host")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    let user = events
        .iter()
        .find_map(|e| {
            e.get("user")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    SessionRow {
        id: session_id.to_string(),
        project_id: None,
        project_name: None,
        label: None,
        custom_label: None,
        branch: None,
        event_count,
        first_event,
        last_event,
        host,
        user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(time: &str, host: Option<&str>, user: Option<&str>) -> Value {
        let mut v = json!({ "time": time });
        if let Some(h) = host {
            v["host"] = json!(h);
        }
        if let Some(u) = user {
            v["user"] = json!(u);
        }
        v
    }

    #[test]
    fn empty_event_slice_yields_zero_count_and_none_fields() {
        let row = session_row_from_events("sid", &[]);
        assert_eq!(row.id, "sid");
        assert_eq!(row.event_count, 0);
        assert_eq!(row.first_event, None);
        assert_eq!(row.last_event, None);
        assert_eq!(row.host, None);
        assert_eq!(row.user, None);
        assert_eq!(row.label, None);
        assert_eq!(row.branch, None);
        assert_eq!(row.project_id, None);
        assert_eq!(row.project_name, None);
        assert_eq!(row.custom_label, None);
    }

    #[test]
    fn single_event_with_host_and_user() {
        let events = vec![ev("2026-05-02T10:00:00Z", Some("Maxs-Air"), Some("max"))];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.event_count, 1);
        assert_eq!(row.first_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.last_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.host.as_deref(), Some("Maxs-Air"));
        assert_eq!(row.user.as_deref(), Some("max"));
    }

    #[test]
    fn multiple_sorted_events_use_first_and_last_times() {
        let events = vec![
            ev("2026-05-02T10:00:00Z", Some("h"), Some("u")),
            ev("2026-05-02T10:01:00Z", Some("h"), Some("u")),
            ev("2026-05-02T10:02:00Z", Some("h"), Some("u")),
        ];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.event_count, 3);
        assert_eq!(row.first_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.last_event.as_deref(), Some("2026-05-02T10:02:00Z"));
    }

    #[test]
    fn unsorted_events_use_min_max_not_array_position() {
        let events = vec![
            ev("2026-05-02T10:01:00Z", Some("h"), Some("u")), // middle
            ev("2026-05-02T10:02:00Z", Some("h"), Some("u")), // latest
            ev("2026-05-02T10:00:00Z", Some("h"), Some("u")), // earliest
        ];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.first_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.last_event.as_deref(), Some("2026-05-02T10:02:00Z"));
    }

    #[test]
    fn host_user_forward_scan_finds_first_non_none() {
        let events = vec![
            ev("2026-05-02T10:00:00Z", None, None),
            ev("2026-05-02T10:01:00Z", Some("Maxs-Air"), Some("max")),
            ev("2026-05-02T10:02:00Z", Some("Other-Host"), Some("other")),
        ];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.host.as_deref(), Some("Maxs-Air"));
        assert_eq!(row.user.as_deref(), Some("max"));
    }

    #[test]
    fn malformed_host_field_yields_none_no_panic() {
        let events = vec![json!({
            "time": "2026-05-02T10:00:00Z",
            "host": 12345,
            "user": ["nested", "array"],
        })];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.host, None);
        assert_eq!(row.user, None);
    }

    #[test]
    fn missing_time_field_skipped_in_min_max_computation() {
        let events = vec![
            json!({ "host": "h", "user": "u" }), // no time field
            json!({ "time": "2026-05-02T10:00:00Z" }),
        ];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.event_count, 2);
        assert_eq!(row.first_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.last_event.as_deref(), Some("2026-05-02T10:00:00Z"));
    }

    #[test]
    fn no_events_have_time_field_yields_none_first_last() {
        let events = vec![
            json!({ "host": "h" }),
            json!({ "user": "u" }),
        ];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.event_count, 2);
        assert_eq!(row.first_event, None);
        assert_eq!(row.last_event, None);
    }

    #[test]
    fn custom_label_is_always_none_in_reconciler_row() {
        // The reconciler never writes a custom_label. The COALESCE upsert
        // means existing user-set custom_label values on rows in the
        // EventStore are preserved.
        let events = vec![ev("2026-05-02T10:00:00Z", Some("h"), Some("u"))];
        let row = session_row_from_events("sid", &events);
        assert_eq!(row.custom_label, None);
    }

    #[test]
    fn report_did_work_reflects_inserts_or_upserts() {
        let mut r = ReconcileReport::default();
        assert!(!r.did_work());
        r.events_inserted = 1;
        assert!(r.did_work());
        r.events_inserted = 0;
        r.sessions_upserted = 1;
        assert!(r.did_work());
        r.events_skipped = 999;
        r.sessions_upserted = 0;
        assert!(!r.did_work()); // skips alone are not work
    }
}
