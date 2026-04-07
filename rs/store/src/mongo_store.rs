//! MongoStore — alternate EventStore backend backed by MongoDB.
//!
//! This is the **Phase 2 stub**: connection bootstrap + index creation +
//! a `todo!()`-only `EventStore` impl. The conformance suite at
//! `store/tests/event_store_conformance.rs` will spin up a Mongo
//! testcontainer and run every helper against this stub — every test
//! will panic at `todo!()`. That's the red wall. Phase 3 starts turning
//! the writes green, Phase 4 the reads, Phase 5 the analytics, Phase 6
//! the FTS.
//!
//! **Why Mongo at all?** Open Story's persistence layer was built around
//! the SQLite `EventStore` trait shape — but the same shape works for
//! distributed deployments where multiple consumers want to share state
//! across hosts. Mongo gives that without forcing every dev to run a
//! Postgres cluster locally. SQLite stays the default; Mongo is opt-in
//! per deployment via `data_backend = "mongo"` in `config.toml` (Phase 7).
//!
//! **Schema mirrors SQLite tables as five collections:**
//! - `events`    — `_id = event.id`, indexed on `(session_id, timestamp)`
//! - `sessions`  — `_id = session_id`, with `custom_label` preservation
//! - `patterns`  — `_id = "{type}:{started_at}:{session}"`, indexed on session_id
//! - `turns`     — `_id = "turn:{session}:{turn_number}"`, indexed on session_id
//! - `plans`     — `_id = plan_id`, indexed on session_id
//! - `events_fts` — text-indexed `searchable_text` field for `$text` search
//!
//! **Type fidelity:** the conformance test
//! `it_round_trips_an_event_payload_losslessly` is the canary. BSON's
//! int32/int64/datetime distinctions can lose data on the way back through
//! serde — when that test goes red, the fix is in this file, not the test.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use mongodb::{options::ClientOptions, Client, Database};
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};

use crate::event_store::{EventStore, SessionRow};

/// MongoDB-backed event store. Alternate to `SqliteStore` for distributed
/// deployments. Selected via `Config::data_backend = "mongo"` (Phase 7).
pub struct MongoStore {
    #[allow(dead_code)] // used by Phase 3+ method bodies
    client: Client,
    #[allow(dead_code)] // used by Phase 3+ method bodies
    db: Database,
}

impl MongoStore {
    /// Connect to a MongoDB deployment, select the named database, and
    /// create the indexes Open Story requires for query performance and
    /// dedup correctness.
    ///
    /// `uri` accepts the standard `mongodb://...` connection string —
    /// single node, replica set, sharded, or Atlas. Auth + TLS go in the
    /// URI per the driver convention.
    pub async fn connect(uri: &str, db_name: &str) -> Result<Self> {
        let mut options = ClientOptions::parse(uri)
            .await
            .map_err(|e| anyhow!("parse mongo uri: {e}"))?;
        // Tag the connection so it shows up identifiably in `db.currentOp()`.
        options.app_name = Some("open-story".to_string());
        let client = Client::with_options(options)
            .map_err(|e| anyhow!("build mongo client: {e}"))?;
        let db = client.database(db_name);

        let store = Self { client, db };
        store.init_indexes().await?;
        Ok(store)
    }

    /// Create the indexes the trait contract relies on. Idempotent — Mongo
    /// silently no-ops `createIndex` calls when the index already exists.
    ///
    /// **Phase 2 stub:** no-op. Phase 3 will populate this with the actual
    /// `IndexModel` calls (events.session_id+timestamp, sessions._id,
    /// patterns.session_id, turns.session_id+turn_number, plans.session_id,
    /// events_fts text index).
    async fn init_indexes(&self) -> Result<()> {
        // TODO Phase 3: create the five collection indexes.
        Ok(())
    }
}

#[async_trait]
impl EventStore for MongoStore {
    // ── Phase 3: writes ─────────────────────────────────────────────
    async fn insert_event(&self, _session_id: &str, _event: &Value) -> Result<bool> {
        todo!("Phase 3: insert_event")
    }

    async fn insert_batch(&self, _session_id: &str, _events: &[Value]) -> Result<usize> {
        todo!("Phase 3: insert_batch")
    }

    async fn upsert_session(&self, _session: &SessionRow) -> Result<()> {
        todo!("Phase 3: upsert_session — must preserve custom_label on update")
    }

    async fn update_session_label(&self, _session_id: &str, _label: &str) -> Result<()> {
        todo!("Phase 3: update_session_label")
    }

    async fn insert_pattern(&self, _session_id: &str, _pattern: &PatternEvent) -> Result<()> {
        todo!("Phase 3: insert_pattern")
    }

    async fn insert_turn(&self, _session_id: &str, _turn: &StructuralTurn) -> Result<()> {
        todo!("Phase 3: insert_turn")
    }

    async fn upsert_plan(&self, _plan_id: &str, _session_id: &str, _content: &str) -> Result<()> {
        todo!("Phase 3: upsert_plan")
    }

    async fn delete_session(&self, _session_id: &str) -> Result<u64> {
        todo!("Phase 3: delete_session — must remove events, patterns, plans, fts entries")
    }

    async fn cleanup_old_sessions(&self, _retention_days: u32) -> Result<u64> {
        todo!("Phase 3: cleanup_old_sessions")
    }

    // ── Phase 4: reads ──────────────────────────────────────────────
    async fn session_events(&self, _session_id: &str) -> Result<Vec<Value>> {
        todo!("Phase 4: session_events — round-trip BSON↔Value losslessly")
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        todo!("Phase 4: list_sessions")
    }

    async fn session_patterns(
        &self,
        _session_id: &str,
        _pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        todo!("Phase 4: session_patterns")
    }

    async fn session_turns(&self, _session_id: &str) -> Result<Vec<StructuralTurn>> {
        todo!("Phase 4: session_turns")
    }

    async fn full_payload(&self, _event_id: &str) -> Result<Option<String>> {
        todo!("Phase 4: full_payload")
    }

    // export_session_jsonl uses the default trait impl which calls
    // session_events — Phase 4 gets it for free.

    // ── Phase 5: analytics queries ──────────────────────────────────
    // (intentionally not stubbed — they fall back to the trait's default
    // empty impls until Phase 5 implements them as Mongo aggregations)

    // ── Phase 6: FTS ────────────────────────────────────────────────
    async fn index_fts(
        &self,
        _event_id: &str,
        _session_id: &str,
        _record_type: &str,
        _text: &str,
    ) -> Result<()> {
        todo!("Phase 6: index_fts — populate searchable_text on events_fts collection")
    }

    async fn search_fts(
        &self,
        _query: &str,
        _limit: usize,
        _session_filter: Option<&str>,
    ) -> Result<Vec<crate::queries::FtsSearchResult>> {
        todo!("Phase 6: search_fts — $text search with optional session_id filter, sort by textScore")
    }

    async fn fts_count(&self) -> Result<u64> {
        todo!("Phase 6: fts_count")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check: MongoStore implements EventStore as a trait object.
    #[test]
    fn mongo_store_is_object_safe() {
        fn _assert_object_safe(_: &dyn EventStore) {}
        // No actual instance needed — this only verifies the trait bounds.
    }
}
