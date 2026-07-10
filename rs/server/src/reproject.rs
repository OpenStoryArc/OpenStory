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

/// Rebuild only the **working set** — sessions whose last activity falls within
/// `since_days` — at boot, leaving colder sessions to be rebuilt lazily on first
/// access via `StoreState::get_or_rebuild`. This is the bounded, lazy boot: it
/// caps the eager rebuild work (and resident memory) at boot to the recent
/// window instead of every session ever recorded.
///
/// SQLite remains the source of truth: an un-seeded session is never lost, only
/// deferred — the read-through cache rebuilds it from the durable events the
/// first time it is read. `since_days == 0` seeds nothing (fully lazy); every
/// session then loads on access.
///
/// A session whose `last_event` is missing or unparseable is treated as out of
/// window (skipped, left to lazy rebuild) rather than eagerly seeded.
pub async fn reproject_working_set(store: &StoreState, since_days: u32) -> ReprojectReport {
    let mut report = ReprojectReport::default();
    if since_days == 0 {
        // Fully lazy — nothing to seed. Every session loads on first access.
        return report;
    }

    let cutoff = chrono::Utc::now() - chrono::Duration::days(since_days as i64);
    let sessions = store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();

    for row in &sessions {
        let within_window = row
            .last_event
            .as_deref()
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
            .unwrap_or(false);
        if !within_window {
            // Older than the working-set window (or no known last_event) — leave
            // it for `get_or_rebuild` to rebuild lazily on first access.
            continue;
        }
        let Some(proj) = rebuild_session(store.event_store.as_ref(), &row.id).await else {
            continue;
        };
        report.sessions_reprojected += 1;
        report.events_applied += proj.event_count();
        store.projections.insert(row.id.clone(), proj);
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_store::event_store::SessionRow;
    use serde_json::json;

    /// Seed one event + a session row (with an explicit `last_event`) into the
    /// store's real SQLite-backed EventStore.
    async fn seed_session(store: &StoreState, id: &str, last_event: &str) {
        store
            .event_store
            .insert_event(
                id,
                &json!({
                    "id": format!("{id}-evt-1"),
                    "specversion": "1.0",
                    "datacontenttype": "application/json",
                    "type": "io.arc.event",
                    "subtype": "message.user.prompt",
                    "time": last_event,
                    "source": "arc://test",
                    "data": {"raw": {}, "seq": 1, "session_id": id},
                }),
            )
            .await
            .unwrap();
        store
            .event_store
            .upsert_session(&SessionRow {
                id: id.into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 1,
                first_event: Some(last_event.to_string()),
                last_event: Some(last_event.to_string()),
                host: None,
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .unwrap();
    }

    /// Lazy boot: `reproject_working_set` eagerly seeds only sessions whose
    /// last activity is inside the window; an older session is left absent from
    /// the cache and is instead rebuilt on first access via `get_or_rebuild`.
    #[tokio::test]
    async fn lazy_boot_seeds_only_recent_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = StoreState::new(tmp.path()).unwrap();

        let recent = chrono::Utc::now().to_rfc3339();
        let old = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        seed_session(&store, "recent", &recent).await;
        seed_session(&store, "old", &old).await;

        let report = reproject_working_set(&store, 7).await;

        assert_eq!(report.sessions_reprojected, 1, "only the recent session is seeded");
        assert!(store.projections.contains("recent"), "recent is resident after boot");
        assert!(
            !store.projections.contains("old"),
            "old is left for on-access rebuild, not eagerly seeded"
        );

        // The old session is not lost — it rebuilds lazily on first access.
        assert!(
            store.get_or_rebuild("old").await.is_some(),
            "old session rebuilds from the durable store on access"
        );
        assert!(store.projections.contains("old"), "get_or_rebuild made old resident");
    }

    /// `reproject_all` is the on-demand full rebuild: it seeds *every* session
    /// regardless of age — the contrast with the working-set boot. Kept covered
    /// so the full-rebuild path stays live.
    #[tokio::test]
    async fn reproject_all_seeds_every_session_regardless_of_age() {
        let tmp = tempfile::tempdir().unwrap();
        let store = StoreState::new(tmp.path()).unwrap();

        let recent = chrono::Utc::now().to_rfc3339();
        let old = (chrono::Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        seed_session(&store, "recent", &recent).await;
        seed_session(&store, "old", &old).await;

        let report = reproject_all(&store).await;

        assert_eq!(report.sessions_reprojected, 2, "full rebuild seeds every session");
        assert!(store.projections.contains("recent"));
        assert!(store.projections.contains("old"), "full rebuild ignores the window");
    }

    /// `since_days == 0` is the fully-lazy boot: nothing is eagerly seeded, but
    /// every session still loads on first access (no data lost).
    #[tokio::test]
    async fn zero_days_seeds_nothing_but_sessions_still_load_on_access() {
        let tmp = tempfile::tempdir().unwrap();
        let store = StoreState::new(tmp.path()).unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        seed_session(&store, "s1", &now).await;

        let report = reproject_working_set(&store, 0).await;
        assert_eq!(report.sessions_reprojected, 0, "fully lazy — seeds nothing");
        assert!(store.projections.is_empty(), "no projection resident after a 0-day boot");

        // Boot still works: the session loads on access.
        assert!(
            store.get_or_rebuild("s1").await.is_some(),
            "session rebuilds lazily on first access even with a 0-day working set"
        );
    }
}
