//! `TestActors` — synchronous driver that runs the four actor-consumers
//! in sequence against a shared `AppState`. Unit tests only.
//!
//! Phase 1 commit 1.2 of the TDD plan
//! (`/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`).
//!
//! Each of the 53 `ingest_events(&mut state, sid, &events, pid)` call
//! sites in the test suite becomes a one-line
//! `actors.drive_batch(sid, &events, pid).await`. Mechanical migration
//! in commit 1.6 deletes `ingest_events`; this helper mirrors the
//! production actor pipeline closely enough that the assertions that
//! used to hold against `ingest_events` still hold after the cutover.
//!
//! Not a production replica: the order is persist → projections →
//! patterns → broadcast, synchronously on a single task, and the
//! `BroadcastMessage`s are returned instead of being sent over a
//! broadcast channel. Integration tests that need the NATS path
//! should use the compose harness (see `test_pi_mono_compose.rs`).
//!
//! #[allow(dead_code)] annotations absorb "not used in this test file"
//! warnings — different test binaries use different subsets of the
//! helper.

#![allow(dead_code)]

use std::sync::Arc;

use dashmap::DashMap;
use open_story::cloud_event::CloudEvent;
use open_story::server::{consumers, SharedState};
use open_story_patterns::PatternEvent;
use open_story_store::persistence::SessionStore;
use open_story_store::projection::SessionProjection;
use tempfile::TempDir;

use crate::helpers::test_state;
use consumers::broadcast::BroadcastConsumer;
use consumers::patterns::PatternsConsumer;
use consumers::persist::PersistConsumer;
use consumers::projections::ProjectionsConsumer;
use open_story::server::BroadcastMessage;

/// Bundle of the four actor-consumers backed by a shared `AppState`.
pub struct TestActors {
    pub persist: PersistConsumer,
    pub patterns: PatternsConsumer,
    pub projections: ProjectionsConsumer,
    pub broadcast: BroadcastConsumer,
    pub state: SharedState,
}

pub struct DriveResult {
    /// BroadcastMessages that would have gone to WebSocket clients.
    pub messages: Vec<BroadcastMessage>,
    /// Patterns detected in this batch (before persistence).
    pub patterns: Vec<PatternEvent>,
    /// Number of events PersistConsumer wrote.
    pub persisted: usize,
    /// Number of events PersistConsumer skipped (PK dedup).
    pub skipped: usize,
}

impl TestActors {
    pub async fn new(tmp: &TempDir) -> Self {
        let state = test_state(tmp);
        let (event_store, session_store, shared_projections, shared_parents, shared_children) = {
            let s = state.read().await;
            let ss =
                SessionStore::new(s.store.data_dir.as_path()).expect("session store for tests");
            (
                s.store.event_store.clone(),
                ss,
                s.store.projections.clone(),
                s.store.subagent_parents.clone(),
                s.store.session_children.clone(),
            )
        };
        Self {
            persist: PersistConsumer::new(event_store, session_store),
            patterns: PatternsConsumer::new(),
            projections: ProjectionsConsumer::new(
                shared_projections,
                shared_parents,
                shared_children,
            ),
            broadcast: BroadcastConsumer::new(),
            state,
        }
    }

    /// Drive a batch through the four consumers synchronously, in the
    /// same order the production dispatcher would:
    ///   1. Persist  — writes events / JSONL / FTS
    ///   2. Projections — updates shared DashMap
    ///   3. Patterns — detects turns + sentence patterns
    ///   4. Broadcast — assembles WebSocket messages
    ///
    /// Returns the messages Broadcast would have emitted + detected
    /// patterns. `project_id` / `project_name` are passed through to
    /// BroadcastConsumer unchanged.
    pub async fn drive_batch(
        &mut self,
        session_id: &str,
        events: &[CloudEvent],
        project_id: Option<&str>,
    ) -> DriveResult {
        let persist_res = self.persist.process_batch(session_id, events).await;
        let _projection_res = self.projections.process_batch(session_id, events);
        let patterns_res = self.patterns.process_batch(session_id, events);

        // Snapshot the projection *after* Actor 3 has updated it, then
        // hand that snapshot to BroadcastConsumer. Clone keeps the
        // DashMap shard lock lifetime short.
        let projection_snapshot: Option<SessionProjection> = {
            let s = self.state.read().await;
            s.store
                .projections
                .get(session_id)
                .map(|r| r.value().clone())
        };

        let project_name = project_id.map(|pid| pid.to_string());
        let messages = match projection_snapshot {
            Some(proj) => self.broadcast.process_batch(
                session_id,
                events,
                &proj,
                project_id.map(|s| s.to_string()),
                project_name,
            ),
            None => Vec::new(),
        };

        DriveResult {
            messages,
            patterns: patterns_res.patterns,
            persisted: persist_res.persisted,
            skipped: persist_res.skipped,
        }
    }

    /// Shorthand — the shared projection DashMap for assertions.
    ///
    /// Callers must `.await` on `self.state.read()` themselves and call
    /// `.store.projections.clone()` on the guard. Direct access is
    /// preferred to keep the DashMap Arc lifetime explicit in tests.
    pub async fn projections(&self) -> Arc<DashMap<String, SessionProjection>> {
        self.state.read().await.store.projections.clone()
    }
}
