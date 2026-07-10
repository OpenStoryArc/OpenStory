//! `reproject` — rebuild the in-memory read model (`SessionProjection`) from
//! the durable EventStore.
//!
//! Boot rehydrates the EventStore (reconcile JSONL→SQLite + boot_from_sqlite)
//! but never rebuilds projections; those are repopulated only when the file
//! watcher re-reads a source. So a session whose source is gone or beyond
//! `boot_window` (pi-mono `--no-session`, old sessions) is durably present yet
//! shows an empty live projection after restart — and `/api/sessions` serves
//! token totals (and live label/branch) from that projection, so it reads 0.
//!
//! `reproject_all` closes that divergence: fold every stored session's events
//! through a fresh `SessionProjection::append` (which dedups by id) and publish
//! the rebuilt projection. It is the first of the explicit state-management
//! operations (see `docs/research/state-management-interface.md`) and is
//! idempotent — re-running, or the watcher re-reading afterward, dedups against
//! the rebuilt `seen_ids`.

use open_story_store::rebuild::rebuild_session;
use open_story_store::state::StoreState;

#[derive(Debug, Default)]
pub struct ReprojectReport {
    pub sessions_reprojected: usize,
    pub events_applied: usize,
}

/// Rebuild every session's projection from the durable EventStore.
///
/// Read-only with respect to events (it only reads `session_events` and writes
/// the derived in-memory read model). Safe to call at boot or on demand.
pub async fn reproject_all(store: &StoreState) -> ReprojectReport {
    let mut report = ReprojectReport::default();
    let sessions = store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();

    for row in &sessions {
        let Some(proj) = rebuild_session(store.event_store.as_ref(), &row.id).await else {
            continue;
        };
        report.sessions_reprojected += 1;
        report.events_applied += proj.event_count();
        store.projections.insert(row.id.clone(), proj);
    }

    report
}
