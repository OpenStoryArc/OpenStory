//! Persist consumer — stores every CloudEvent to durable storage.
//!
//! Actor contract:
//!   subscribes: events.>
//!   publishes:  nothing (pure sink)
//!   owns:       event_store, session_store, FTS index, **sessions table**
//!
//! Responsibilities:
//!   1. Insert into the durable EventStore (SQLite or MongoDB).
//!      Dedup is the EventStore's PRIMARY KEY job — `insert_event`
//!      returns `Ok(false)` for duplicates and we treat that as "skipped"
//!      without further work.
//!   2. Append to JSONL backup (only on successful insert — duplicates
//!      shouldn't pollute the sovereignty escape hatch).
//!   3. Index in FTS for full-text search (only on successful insert).
//!   4. **(new at commit 1.5)** Upsert the sessions-table row after
//!      the batch's events are durable. PersistConsumer is the single
//!      writer of `SessionRow` — the old code path where `ingest_events`
//!      also called `upsert_session` is retired in commit 1.6.
//!
//! The SessionRow upsert reads `label` / `branch` / `event_count` from
//! the shared projection DashMap. Under eventual consistency there is
//! a one-batch lag: ProjectionsConsumer may not have processed the
//! current batch yet when PersistConsumer writes the row. Corrects on
//! the next batch. This is the acknowledged consequence of independent
//! actor subscriptions (see the eventual-consistency principle in the
//! Phase 1 plan).

use std::sync::Arc;

use dashmap::DashMap;
use open_story_core::cloud_event::CloudEvent;
use open_story_store::analysis;
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::persistence::SessionStore;
use open_story_store::projection::SessionProjection;
use open_story_views::from_cloud_event::from_cloud_event;

/// True if the event's `time` is synthesized at translation (e.g.
/// `file.snapshot`, whose source line has no timestamp) rather than carried
/// by the source. Such events must not define session recency. Unknown or
/// missing subtypes are treated as real activity. See
/// [`open_story_core::subtype::Subtype::time_is_synthesized`].
fn time_is_synthesized(ce: &CloudEvent) -> bool {
    ce.subtype
        .as_deref()
        .and_then(|s| s.parse::<open_story_core::subtype::Subtype>().ok())
        .map(|st| st.time_is_synthesized())
        .unwrap_or(false)
}

/// State owned by the persist consumer actor.
pub struct PersistConsumer {
    /// Shared event store (Arc — SQLite handles internal locking).
    event_store: Arc<dyn EventStore>,
    /// JSONL backup store (owned — only this actor writes).
    session_store: SessionStore,
    /// Shared projection map (read-only from this consumer's POV).
    projections: Arc<DashMap<String, SessionProjection>>,
    /// Shared project-id map — written here when `project_id` arrives on
    /// the batch envelope; read by other consumers / API.
    session_projects: Arc<DashMap<String, String>>,
    /// Shared project-display-name map (read-only here — ingest_events
    /// still owns derivation until 1.6).
    session_project_names: Arc<DashMap<String, String>>,
}

/// Result of processing one batch of events.
pub struct PersistResult {
    /// Number of events persisted (after PK dedup).
    pub persisted: usize,
    /// Number of events skipped (PK collision — already persisted).
    pub skipped: usize,
}

impl PersistConsumer {
    /// Create a new persist consumer with its shared state.
    pub fn new(
        event_store: Arc<dyn EventStore>,
        session_store: SessionStore,
        projections: Arc<DashMap<String, SessionProjection>>,
        session_projects: Arc<DashMap<String, String>>,
        session_project_names: Arc<DashMap<String, String>>,
    ) -> Self {
        Self {
            event_store,
            session_store,
            projections,
            session_projects,
            session_project_names,
        }
    }

    /// Process a batch of CloudEvents — persist, index, and upsert the
    /// session row. `project_id` is the batch envelope's project
    /// (from the watcher); `None` for batches where the source doesn't
    /// know its project yet (pi-mono imports, etc.).
    pub async fn process_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
        project_id: Option<&str>,
    ) -> PersistResult {
        let event_store = &*self.event_store;
        let session_store = &self.session_store;
        let mut persisted = 0;
        let mut skipped = 0;

        // Remember the project id for this session if the batch carries one.
        if let Some(pid) = project_id {
            // Keep the record idempotent — only insert once per session to
            // avoid churning the DashMap on every batch.
            if !self.session_projects.contains_key(session_id) {
                self.session_projects
                    .insert(session_id.to_string(), pid.to_string());
            }
        } else if !self.session_projects.contains_key(session_id) {
            if let Some(cwd) = events.iter().find_map(|event| {
                let value = serde_json::to_value(event).ok()?;
                analysis::extract_cwd(&value)
            }) {
                let resolved = analysis::resolve_project(&cwd, &[]);
                self.session_projects
                    .insert(session_id.to_string(), resolved.project_id);
                self.session_project_names
                    .insert(session_id.to_string(), resolved.project_name);
            }
        }

        // Batch the three durable writes (SQLite insert, JSONL append, FTS
        // index) at the grain the bus already delivers — one IngestBatch — so
        // a backfilled transcript costs ~3 fsyncs, not 3 per event. The
        // per-event path topped out at ~44 ev/s and was CPU-invariant: a
        // serial fsync wall, not a compute one. Dedup semantics are preserved
        // exactly — JSONL append and FTS still gate on which events were
        // freshly inserted (the `insert_batch_returning` flags).
        let mut ces = Vec::with_capacity(events.len());
        let mut vals: Vec<serde_json::Value> = Vec::with_capacity(events.len());
        for ce in events {
            if let Ok(val) = serde_json::to_value(ce) {
                ces.push(ce);
                vals.push(val);
            }
        }

        // EventStore PK is the dedup boundary. Flags are per-event: true on a
        // fresh insert, false when the event_id already existed.
        let flags = event_store
            .insert_batch_returning(session_id, &vals)
            .await
            .unwrap_or_else(|_| vec![false; vals.len()]);

        let mut new_vals: Vec<&serde_json::Value> = Vec::new();
        let mut fts: Vec<(String, String, String, String)> = Vec::new();
        for (i, &inserted) in flags.iter().enumerate() {
            if !inserted {
                skipped += 1;
                continue;
            }
            // Only freshly-inserted events touch the sovereignty escape hatch
            // (JSONL) and the search index — duplicates must not pollute them.
            new_vals.push(&vals[i]);
            for vr in from_cloud_event(ces[i]) {
                if let Some(text) = open_story_store::extract::extract_text(&vr) {
                    let record_type = open_story_store::extract::record_type_str(&vr.body);
                    fts.push((
                        vr.id.clone(),
                        session_id.to_string(),
                        record_type.to_string(),
                        text,
                    ));
                }
            }
            persisted += 1;
        }

        let _ = session_store.append_batch(session_id, &new_vals);
        let _ = event_store.index_fts_batch(&fts).await;

        // Upsert the session row AFTER the events are durable. Takes a
        // tight projection snapshot and drops the DashMap Ref before
        // the async upsert call — shard-lock-across-await deadlock guard.
        if !events.is_empty() {
            let (label, branch, event_count) = match self.projections.get(session_id) {
                Some(r) => {
                    let p = r.value();
                    (
                        p.label().map(|s| s.to_string()),
                        p.branch().map(|s| s.to_string()),
                        p.event_count() as u64,
                    )
                }
                None => (None, None, 0),
            };
            let project_id_snapshot = self
                .session_projects
                .get(session_id)
                .map(|r| r.value().clone());
            let project_name_snapshot = self
                .session_project_names
                .get(session_id)
                .map(|r| r.value().clone());

            // first_event / last_event define session recency (sort order
            // and the UI time filter). Scan MIN/MAX over the batch — don't
            // assume arrival order — and exclude events whose `time` is
            // synthesized at translation (file.snapshot: no source timestamp,
            // stamped Utc::now()). Including them re-stamps a dead session to
            // boot time on every restart. See `Subtype::time_is_synthesized`.
            let mut first_event_ts: Option<String> = None;
            let mut last_event_ts: Option<String> = None;
            for ce in events {
                if ce.time.is_empty() || time_is_synthesized(ce) {
                    continue;
                }
                let t = ce.time.as_str();
                if first_event_ts.as_deref().is_none_or(|c| t < c) {
                    first_event_ts = Some(ce.time.clone());
                }
                if last_event_ts.as_deref().is_none_or(|c| t > c) {
                    last_event_ts = Some(ce.time.clone());
                }
            }

            // Host and user are stamped at translation time onto every
            // CloudEvent; agent platform is the CloudEvent extension
            // attribute that identifies the translator. We lift them off the
            // first event in the batch — every event in a given session should
            // agree on these, so the first is canonical. Events without these
            // stamps (pre-migration) leave the row's value at None; the
            // EventStore upserts use a COALESCE-style write so a None batch
            // never blanks out an already-stamped row.
            let host = events.first().and_then(|ce| ce.host.clone());
            let user = events.first().and_then(|ce| ce.user.clone());
            let origin_agent = events.first().and_then(|ce| ce.agent.clone());

            let row = SessionRow {
                id: session_id.to_string(),
                project_id: project_id_snapshot,
                project_name: project_name_snapshot,
                label,
                custom_label: None, // never set from projection — user PUT only
                branch,
                event_count,
                first_event: first_event_ts,
                last_event: last_event_ts,
                host,
                user,
                origin_agent,
            };
            let _ = event_store.upsert_session(&row).await;
        }

        PersistResult { persisted, skipped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use serde_json::json;

    fn test_event(id: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("test content".to_string());
        let data = EventData::with_payload(
            json!({}),
            0,
            "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/sess-1".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(id.to_string()),
            None,
            None,
            None,
            None,
        )
    }

    fn test_event_with_cwd(id: &str, cwd: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.cwd = Some(cwd.to_string());
        payload.text = Some("test content".to_string());
        let data = EventData::with_payload(
            json!({}),
            0,
            "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/sess-1".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(id.to_string()),
            None,
            None,
            None,
            None,
        )
    }

    fn make_consumer() -> (PersistConsumer, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let session_store = SessionStore::new(tmp.path()).expect("create session store");
        let event_store: Arc<dyn EventStore> = Arc::new(
            open_story_store::sqlite_store::SqliteStore::new(tmp.path())
                .expect("create sqlite store"),
        );
        (
            PersistConsumer::new(
                event_store,
                session_store,
                Arc::new(DashMap::new()),
                Arc::new(DashMap::new()),
                Arc::new(DashMap::new()),
            ),
            tmp,
        )
    }

    /// Helper: build a test event with an explicit subtype and time.
    fn event_st(id: &str, subtype: &str, time: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("x".to_string());
        let data = EventData::with_payload(
            json!({}),
            0,
            "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/sess-1".into(),
            "io.arc.event".into(),
            data,
            Some(subtype.into()),
            Some(id.to_string()),
            Some(time.to_string()),
            None,
            None,
            Some("claude-code".into()),
        )
    }

    #[tokio::test]
    async fn file_snapshot_time_does_not_advance_last_event() {
        // The "Last Hour" bug at the live-ingest layer: a batch carrying a
        // real message (10:00) plus a boot-stamped file.snapshot (3 weeks
        // later) must set last_event to the real message time, not the
        // snapshot's synthesized Utc::now().
        let (mut consumer, _tmp) = make_consumer();
        let real = event_st("real-1", "message.user.prompt", "2026-05-02T10:00:00Z");
        let snap = event_st("snap-1", "file.snapshot", "2026-05-25T00:42:35Z");

        consumer
            .process_batch("sess-snap", &[real, snap], None)
            .await;

        let sessions = consumer.event_store.list_sessions().await.unwrap();
        let row = sessions
            .iter()
            .find(|r| r.id == "sess-snap")
            .expect("session row exists");
        assert_eq!(row.first_event.as_deref(), Some("2026-05-02T10:00:00Z"));
        assert_eq!(row.last_event.as_deref(), Some("2026-05-02T10:00:00Z"));
    }

    #[tokio::test]
    async fn snapshot_only_batch_leaves_bounds_none() {
        // A batch of only file.snapshot events carries no real activity
        // time — bounds stay None so the COALESCE upsert never blanks or
        // inflates an existing row.
        let (mut consumer, _tmp) = make_consumer();
        let snap = event_st("snap-only", "file.snapshot", "2026-05-25T00:42:35Z");

        consumer.process_batch("sess-snaponly", &[snap], None).await;

        let sessions = consumer.event_store.list_sessions().await.unwrap();
        let row = sessions
            .iter()
            .find(|r| r.id == "sess-snaponly")
            .expect("session row exists");
        assert!(
            row.first_event.is_none(),
            "no real activity → no first_event"
        );
        assert!(row.last_event.is_none(), "no real activity → no last_event");
    }

    /// Helper: build a test event stamped with a given host (or none).
    fn test_event_with_host(id: &str, host: Option<&str>) -> CloudEvent {
        let ce = test_event(id);
        match host {
            Some(h) => ce.with_host(h),
            None => ce,
        }
    }

    #[tokio::test]
    async fn persist_consumer_stamps_host_on_session_row_from_first_event() {
        // Contract: host is stamped on every CloudEvent at translation
        // time. PersistConsumer reads it off the first event and writes it
        // onto the session row. This is the moment origin identity enters
        // the query layer.
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event_with_host("evt-host-1", Some("Maxs-Air"));
        let e2 = test_event_with_host("evt-host-2", Some("Maxs-Air"));

        consumer
            .process_batch("sess-host-stamp", &[e1, e2], None)
            .await;

        let sessions = consumer.event_store.list_sessions().await.unwrap();
        let row = sessions
            .iter()
            .find(|r| r.id == "sess-host-stamp")
            .expect("session row exists after ingest");
        assert_eq!(row.host.as_deref(), Some("Maxs-Air"));
    }

    #[tokio::test]
    async fn persist_consumer_leaves_host_none_when_events_lack_host() {
        // Backwards-compat: pre-migration events have no host. The row
        // stays with host: None — not a crash, not a fake value.
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event_with_host("evt-legacy-1", None);

        consumer.process_batch("sess-legacy", &[e1], None).await;

        let sessions = consumer.event_store.list_sessions().await.unwrap();
        let row = sessions
            .iter()
            .find(|r| r.id == "sess-legacy")
            .expect("session row exists");
        assert!(
            row.host.is_none(),
            "legacy events must not fabricate a host"
        );
    }

    #[tokio::test]
    async fn derives_project_from_cwd_when_batch_has_no_path_project() {
        let (mut consumer, _tmp) = make_consumer();
        let event = test_event_with_cwd("evt-cwd", "/Users/maxglassie/projects/OpenStory/rs");

        let result = consumer.process_batch("sess-cwd", &[event], None).await;

        assert_eq!(result.persisted, 1);
        assert_eq!(
            consumer
                .session_projects
                .get("sess-cwd")
                .map(|entry| entry.value().clone()),
            Some("-Users-maxglassie-projects-OpenStory-rs".to_string())
        );
        assert_eq!(
            consumer
                .session_project_names
                .get("sess-cwd")
                .map(|entry| entry.value().clone()),
            Some("OpenStory".to_string())
        );
    }

    #[tokio::test]
    async fn dedup_skips_duplicate_event_ids_via_pk() {
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event("evt-1");
        let e2 = test_event("evt-1"); // same ID
        let e3 = test_event("evt-2"); // different ID

        let result = consumer.process_batch("sess-1", &[e1, e2, e3], None).await;
        assert_eq!(result.persisted, 2, "should persist 2 unique events");
        assert_eq!(
            result.skipped, 1,
            "should skip 1 duplicate via PK collision"
        );
    }

    #[tokio::test]
    async fn dedup_state_persists_across_batches() {
        let (mut consumer, _tmp) = make_consumer();

        let e1 = test_event("evt-1");
        let result1 = consumer.process_batch("sess-1", &[e1], None).await;
        assert_eq!(result1.persisted, 1);

        // Same event_id in a fresh batch — the EventStore PK still rejects it.
        let e1_again = test_event("evt-1");
        let result2 = consumer.process_batch("sess-1", &[e1_again], None).await;
        assert_eq!(result2.persisted, 0);
        assert_eq!(result2.skipped, 1);
    }

    // ── Actor ownership contract tests ────────────────────────────────
    //
    // Locks in that PersistConsumer is the single owner of these three
    // side effects. The sibling tests in server/src/ingest.rs currently
    // show ingest_events doing two of them in parallel (insert_event +
    // index_fts) — that's the documented dual-write that's safe via
    // idempotence but queued for removal when Actor 4 decomposes. JSONL
    // append was the third and was removed 2026-04-15 because it wasn't
    // idempotent. See docs/research/architecture-audit/DUAL_WRITE_AUDIT.md.

    #[tokio::test]
    async fn persist_consumer_owns_jsonl_append() {
        let (mut consumer, tmp) = make_consumer();
        let e1 = test_event("evt-persist-jsonl-1");
        consumer.process_batch("sess-j", &[e1], None).await;

        // JSONL file must exist and contain the event. PersistConsumer is
        // the only writer after the 2026-04-15 fix.
        let jsonl_path = tmp.path().join("sess-j.jsonl");
        assert!(jsonl_path.exists(), "PersistConsumer must create the JSONL");
        let lines: Vec<String> = std::fs::read_to_string(&jsonl_path)
            .unwrap()
            .lines()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(lines.len(), 1, "one event → one line");
        let parsed: serde_json::Value = serde_json::from_str(&lines[0])
            .expect("PersistConsumer must write valid JSON per line");
        assert_eq!(parsed["id"].as_str(), Some("evt-persist-jsonl-1"));
    }

    #[tokio::test]
    async fn persist_consumer_indexes_fts_for_durable_events() {
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event("evt-fts-1");
        consumer.process_batch("sess-fts", &[e1], None).await;

        // FTS is shared with ingest_events today (documented dual-write,
        // idempotent via INSERT OR IGNORE). This test ensures PersistConsumer
        // fulfils its half of the contract.
        let results = consumer
            .event_store
            .search_fts("test content", 10, None)
            .await
            .unwrap();
        assert!(
            results.iter().any(|r| r.event_id == "evt-fts-1"),
            "PersistConsumer must index durable events into FTS5"
        );
    }

    #[tokio::test]
    async fn persist_consumer_inserts_into_event_store() {
        let (mut consumer, _tmp) = make_consumer();
        let e1 = test_event("evt-insert-1");
        let result = consumer.process_batch("sess-ins", &[e1], None).await;

        assert_eq!(result.persisted, 1);
        let events = consumer
            .event_store
            .session_events("sess-ins")
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"].as_str(), Some("evt-insert-1"));
    }
}
