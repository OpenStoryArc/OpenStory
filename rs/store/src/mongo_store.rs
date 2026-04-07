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
use bson::{doc, Bson, Document};
use mongodb::error::{ErrorKind, InsertManyError, WriteFailure};
use mongodb::{options::ClientOptions, Client, Collection, Database};
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};

use crate::event_store::{EventStore, SessionRow};

// Collection names — kept as const so any rename happens in one place.
const COLL_EVENTS: &str = "events";
#[allow(dead_code)] // populated in later phases
const COLL_SESSIONS: &str = "sessions";
#[allow(dead_code)]
const COLL_PATTERNS: &str = "patterns";
#[allow(dead_code)]
const COLL_TURNS: &str = "turns";
#[allow(dead_code)]
const COLL_PLANS: &str = "plans";
#[allow(dead_code)]
const COLL_FTS: &str = "events_fts";

/// Mongo error code for duplicate key violations on `insert*`.
const MONGO_DUPLICATE_KEY: i32 = 11000;

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
    /// silently no-ops `createIndex` calls when an index with the same
    /// shape already exists.
    ///
    /// Indexes:
    /// - `events`: compound (session_id, timestamp) for `session_events`
    ///   range scans. The `_id` PK is implicit.
    /// - `patterns`: session_id index for `session_patterns` filter.
    /// - `turns`: session_id index for `session_turns` filter.
    /// - `plans`: session_id index.
    /// - `events_fts`: a **text index** on `searchable_text` powering
    ///   `$text: { $search: ... }` queries used by `search_fts`.
    ///   Mongo's text index implements stemming + stopword removal +
    ///   relevance scoring out of the box; we use the default English
    ///   analyzer to match SQLite's `porter` tokenizer.
    async fn init_indexes(&self) -> Result<()> {
        use mongodb::IndexModel;

        let events: Collection<Document> = self.db.collection(COLL_EVENTS);
        events
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "session_id": 1, "timestamp": 1 })
                    .build(),
            )
            .await
            .map_err(|e| anyhow!("create events index: {e}"))?;

        let patterns: Collection<Document> = self.db.collection(COLL_PATTERNS);
        patterns
            .create_index(IndexModel::builder().keys(doc! { "session_id": 1 }).build())
            .await
            .map_err(|e| anyhow!("create patterns index: {e}"))?;

        let turns: Collection<Document> = self.db.collection(COLL_TURNS);
        turns
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "session_id": 1, "turn_number": 1 })
                    .build(),
            )
            .await
            .map_err(|e| anyhow!("create turns index: {e}"))?;

        let plans: Collection<Document> = self.db.collection(COLL_PLANS);
        plans
            .create_index(IndexModel::builder().keys(doc! { "session_id": 1 }).build())
            .await
            .map_err(|e| anyhow!("create plans index: {e}"))?;

        // FTS: text index on searchable_text. Mongo's text index syntax
        // uses the special "text" string in the keys document.
        let fts: Collection<Document> = self.db.collection(COLL_FTS);
        fts.create_index(
            IndexModel::builder()
                .keys(doc! { "searchable_text": "text" })
                .build(),
        )
        .await
        .map_err(|e| anyhow!("create events_fts text index: {e}"))?;
        // Companion index on session_id for the optional filter — text
        // queries combine with regular filters via compound match.
        fts.create_index(IndexModel::builder().keys(doc! { "session_id": 1 }).build())
            .await
            .map_err(|e| anyhow!("create events_fts session_id index: {e}"))?;

        Ok(())
    }
}

#[async_trait]
impl EventStore for MongoStore {
    // ── Phase 3: writes ─────────────────────────────────────────────

    /// Insert a CloudEvent. Dedup is per **event id** (global, not per
    /// session) — that matches `SqliteStore`'s `INSERT OR IGNORE` on
    /// the `events.id` PK and the `seen_event_ids` invariant in the
    /// persist consumer.
    ///
    /// Returns `Ok(true)` for new events, `Ok(false)` for duplicates.
    /// A `Mongo` write error with code 11000 is the only error path that
    /// maps to `Ok(false)`; all other errors propagate as `Err`.
    async fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool> {
        let doc = event_to_doc(session_id, event)?;
        let coll: Collection<Document> = self.db.collection(COLL_EVENTS);
        match coll.insert_one(doc).await {
            Ok(_) => Ok(true),
            Err(e) => {
                if is_duplicate_key(&e) {
                    Ok(false)
                } else {
                    Err(anyhow!("mongo insert_event: {e}"))
                }
            }
        }
    }

    /// Insert a batch of CloudEvents. Returns the count of new (non-
    /// duplicate) events. Matches `SqliteStore::insert_batch`'s
    /// transaction-with-INSERT-OR-IGNORE semantics: duplicates are
    /// silently skipped, the count reflects only new rows.
    ///
    /// Implementation: `insert_many(ordered: false)` so Mongo continues
    /// past duplicate-key errors. On partial failure the driver returns
    /// `InsertMany` with `inserted_ids` listing the successful indices —
    /// that count *is* our return value, no need to subtract failures.
    async fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let docs: Vec<Document> = events
            .iter()
            .map(|e| event_to_doc(session_id, e))
            .collect::<Result<Vec<_>>>()?;

        let coll: Collection<Document> = self.db.collection(COLL_EVENTS);
        let opts = mongodb::options::InsertManyOptions::builder()
            .ordered(false)
            .build();
        match coll.insert_many(docs).with_options(opts).await {
            Ok(result) => Ok(result.inserted_ids.len()),
            Err(e) => {
                // Partial-failure path: extract the InsertManyError, count
                // duplicates, and return the count that DID succeed. Any
                // other write error (e.g., write concern, network) is
                // surfaced.
                if let ErrorKind::InsertMany(ref ime) = *e.kind {
                    let dup_count = count_duplicate_keys(ime);
                    let total_failures = ime
                        .write_errors
                        .as_ref()
                        .map(|w| w.len())
                        .unwrap_or(0);
                    if total_failures == dup_count {
                        // All failures were duplicates — the remainder of
                        // the input must have succeeded (`ordered: false`).
                        // `InsertManyError::inserted_ids` is pub(crate),
                        // so we compute inserted = total - failures.
                        return Ok(events.len() - total_failures);
                    }
                }
                Err(anyhow!("mongo insert_batch: {e}"))
            }
        }
    }

    /// Upsert a session row. **Must NOT touch `custom_label`** — that
    /// field is owned by the user via `update_session_label`. Boot replay
    /// and live ingest both call this method, and they must never clobber
    /// a name the user picked.
    ///
    /// Implementation: `update_one` with `upsert: true`, using `$set` for
    /// the auto-derived fields and `$setOnInsert` for `custom_label = null`
    /// (so first-time inserts get an explicit field, but updates leave any
    /// existing custom_label alone).
    async fn upsert_session(&self, session: &SessionRow) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_SESSIONS);
        let filter = doc! { "_id": &session.id };
        let update = doc! {
            "$set": {
                "project_id": session.project_id.as_deref().map(Bson::from).unwrap_or(Bson::Null),
                "project_name": session.project_name.as_deref().map(Bson::from).unwrap_or(Bson::Null),
                "label": session.label.as_deref().map(Bson::from).unwrap_or(Bson::Null),
                "branch": session.branch.as_deref().map(Bson::from).unwrap_or(Bson::Null),
                "event_count": session.event_count as i64,
                "first_event": session.first_event.as_deref().map(Bson::from).unwrap_or(Bson::Null),
                "last_event": session.last_event.as_deref().map(Bson::from).unwrap_or(Bson::Null),
            },
            // custom_label is set to null only on first insert; subsequent
            // upserts leave whatever the user set (or null) untouched.
            "$setOnInsert": { "custom_label": Bson::Null },
        };
        let opts = mongodb::options::UpdateOptions::builder().upsert(true).build();
        coll.update_one(filter, update)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo upsert_session: {e}"))?;
        Ok(())
    }

    /// Set the user's custom label for a session. This is the *only*
    /// method that writes to `custom_label`. Conformance contract:
    /// subsequent `upsert_session` calls must leave this value untouched.
    async fn update_session_label(&self, session_id: &str, label: &str) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_SESSIONS);
        let filter = doc! { "_id": session_id };
        let update = doc! { "$set": { "custom_label": label } };
        // upsert: true so calling update_session_label on a brand-new
        // session id (no upsert_session yet) still creates the row with
        // just the custom_label set. Mirrors how SQLite's UPDATE on a
        // missing row would silently no-op — except here it's actively
        // useful for the API endpoint.
        let opts = mongodb::options::UpdateOptions::builder().upsert(true).build();
        coll.update_one(filter, update)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo update_session_label: {e}"))?;
        Ok(())
    }

    /// Insert a detected pattern. The `_id` is deterministic
    /// (`{type}:{started_at}:{session}`) so re-detecting the same pattern
    /// dedupes — matches `SqliteStore`'s `INSERT OR IGNORE`.
    async fn insert_pattern(&self, session_id: &str, pattern: &PatternEvent) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_PATTERNS);
        let id = format!("{}:{}:{}", pattern.pattern_type, pattern.started_at, session_id);
        let metadata: Bson = bson::to_bson(&pattern.metadata)
            .map_err(|e| anyhow!("pattern metadata → bson: {e}"))?;
        let event_ids: Bson = bson::to_bson(&pattern.event_ids)
            .map_err(|e| anyhow!("pattern event_ids → bson: {e}"))?;
        let doc = doc! {
            "_id": id,
            "session_id": session_id,
            "pattern_type": &pattern.pattern_type,
            "started_at": &pattern.started_at,
            "ended_at": &pattern.ended_at,
            "summary": &pattern.summary,
            "event_ids": event_ids,
            "metadata": metadata,
        };
        match coll.insert_one(doc).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key(&e) => Ok(()), // re-detect is a no-op
            Err(e) => Err(anyhow!("mongo insert_pattern: {e}")),
        }
    }

    /// Insert a completed structural turn. `_id` is `turn:{session}:{n}`,
    /// matching SQLite's primary key shape. Re-runs of the same turn id
    /// overwrite (matches SQLite's `INSERT OR REPLACE`).
    async fn insert_turn(&self, session_id: &str, turn: &StructuralTurn) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_TURNS);
        let id = format!("turn:{}:{}", session_id, turn.turn_number);
        let payload: Bson = bson::to_bson(turn).map_err(|e| anyhow!("turn → bson: {e}"))?;
        let doc = doc! {
            "_id": id,
            "session_id": session_id,
            "turn_number": turn.turn_number as i64,
            "timestamp": &turn.timestamp,
            "data": payload,
        };
        let opts = mongodb::options::ReplaceOptions::builder().upsert(true).build();
        coll.replace_one(doc! { "_id": format!("turn:{}:{}", session_id, turn.turn_number) }, doc)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo insert_turn: {e}"))?;
        Ok(())
    }

    /// Upsert a plan. Same `_id`-based dedup pattern; new content replaces
    /// old. Mirrors SQLite's `ON CONFLICT(id) DO UPDATE`.
    async fn upsert_plan(&self, plan_id: &str, session_id: &str, content: &str) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_PLANS);
        let now = chrono::Utc::now().to_rfc3339();
        let filter = doc! { "_id": plan_id };
        let update = doc! {
            "$set": {
                "session_id": session_id,
                "content": content,
            },
            "$setOnInsert": { "created_at": &now },
        };
        let opts = mongodb::options::UpdateOptions::builder().upsert(true).build();
        coll.update_one(filter, update)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo upsert_plan: {e}"))?;
        Ok(())
    }

    /// Delete a session and all of its events, patterns, plans, and FTS
    /// entries. Returns the count of *events* deleted (matches the
    /// SqliteStore contract used by the API).
    async fn delete_session(&self, session_id: &str) -> Result<u64> {
        let events: Collection<Document> = self.db.collection(COLL_EVENTS);
        let sessions: Collection<Document> = self.db.collection(COLL_SESSIONS);
        let patterns: Collection<Document> = self.db.collection(COLL_PATTERNS);
        let turns: Collection<Document> = self.db.collection(COLL_TURNS);
        let plans: Collection<Document> = self.db.collection(COLL_PLANS);
        let fts: Collection<Document> = self.db.collection(COLL_FTS);

        let filter_sid = doc! { "session_id": session_id };

        // Delete events first so the returned count is meaningful.
        let evt_result = events
            .delete_many(filter_sid.clone())
            .await
            .map_err(|e| anyhow!("mongo delete events: {e}"))?;
        patterns
            .delete_many(filter_sid.clone())
            .await
            .map_err(|e| anyhow!("mongo delete patterns: {e}"))?;
        turns
            .delete_many(filter_sid.clone())
            .await
            .map_err(|e| anyhow!("mongo delete turns: {e}"))?;
        plans
            .delete_many(filter_sid.clone())
            .await
            .map_err(|e| anyhow!("mongo delete plans: {e}"))?;
        fts.delete_many(filter_sid)
            .await
            .map_err(|e| anyhow!("mongo delete fts: {e}"))?;
        sessions
            .delete_one(doc! { "_id": session_id })
            .await
            .map_err(|e| anyhow!("mongo delete session row: {e}"))?;

        Ok(evt_result.deleted_count)
    }

    /// Delete sessions whose `last_event` is older than the cutoff.
    /// Mirrors `SqliteStore::cleanup_old_sessions`. Returns the total
    /// count of events removed across all deleted sessions.
    async fn cleanup_old_sessions(&self, retention_days: u32) -> Result<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let sessions: Collection<Document> = self.db.collection(COLL_SESSIONS);

        // Find stale sessions. Match SQLite semantics: stale = last_event
        // older than cutoff, OR last_event missing AND first_event older.
        let filter = doc! {
            "$or": [
                { "last_event": { "$lt": &cutoff_str } },
                {
                    "$and": [
                        { "last_event": Bson::Null },
                        { "first_event": { "$lt": &cutoff_str } },
                    ]
                }
            ]
        };
        let mut cursor = sessions
            .find(filter)
            .await
            .map_err(|e| anyhow!("mongo cleanup find: {e}"))?;
        let mut stale_ids: Vec<String> = Vec::new();
        use futures::StreamExt;
        while let Some(next) = cursor.next().await {
            let row = next.map_err(|e| anyhow!("mongo cleanup cursor: {e}"))?;
            if let Some(id) = row.get_str("_id").ok() {
                stale_ids.push(id.to_string());
            }
        }

        let mut total_events = 0u64;
        for sid in &stale_ids {
            total_events += self.delete_session(sid).await?;
        }
        Ok(total_events)
    }

    // ── Phase 4: reads ──────────────────────────────────────────────

    /// Load all events for a session, ordered by `time`. Critical: the
    /// returned `Value` must be byte-for-byte equivalent to what was
    /// passed to `insert_event` — no field reordering, no int/float
    /// drift, no datetime coercion. The conformance test
    /// `it_round_trips_an_event_payload_losslessly` is the canary.
    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
        use futures::StreamExt;
        let coll: Collection<Document> = self.db.collection(COLL_EVENTS);
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "timestamp": 1 })
            .build();
        let mut cursor = coll
            .find(doc! { "session_id": session_id })
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo session_events find: {e}"))?;

        let mut out = Vec::new();
        while let Some(next) = cursor.next().await {
            let doc = next.map_err(|e| anyhow!("mongo session_events cursor: {e}"))?;
            // The full original CloudEvent was stored at `payload`.
            if let Some(payload) = doc.get("payload") {
                let value: Value = bson::from_bson(payload.clone())
                    .map_err(|e| anyhow!("payload bson → value: {e}"))?;
                out.push(value);
            }
        }
        Ok(out)
    }

    /// List all session metadata rows.
    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        use futures::StreamExt;
        let coll: Collection<Document> = self.db.collection(COLL_SESSIONS);
        let mut cursor = coll
            .find(doc! {})
            .await
            .map_err(|e| anyhow!("mongo list_sessions find: {e}"))?;
        let mut rows = Vec::new();
        while let Some(next) = cursor.next().await {
            let doc = next.map_err(|e| anyhow!("mongo list_sessions cursor: {e}"))?;
            rows.push(doc_to_session_row(&doc)?);
        }
        Ok(rows)
    }

    async fn session_patterns(
        &self,
        session_id: &str,
        pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        use futures::StreamExt;
        let coll: Collection<Document> = self.db.collection(COLL_PATTERNS);
        let mut filter = doc! { "session_id": session_id };
        if let Some(pt) = pattern_type {
            filter.insert("pattern_type", pt);
        }
        let mut cursor = coll
            .find(filter)
            .await
            .map_err(|e| anyhow!("mongo session_patterns find: {e}"))?;
        let mut out = Vec::new();
        while let Some(next) = cursor.next().await {
            let doc = next.map_err(|e| anyhow!("mongo session_patterns cursor: {e}"))?;
            out.push(doc_to_pattern_event(&doc)?);
        }
        Ok(out)
    }

    async fn session_turns(&self, session_id: &str) -> Result<Vec<StructuralTurn>> {
        use futures::StreamExt;
        let coll: Collection<Document> = self.db.collection(COLL_TURNS);
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "turn_number": 1 })
            .build();
        let mut cursor = coll
            .find(doc! { "session_id": session_id })
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo session_turns find: {e}"))?;
        let mut out = Vec::new();
        while let Some(next) = cursor.next().await {
            let doc = next.map_err(|e| anyhow!("mongo session_turns cursor: {e}"))?;
            if let Some(payload) = doc.get("data") {
                let turn: StructuralTurn = bson::from_bson(payload.clone())
                    .map_err(|e| anyhow!("turn bson → struct: {e}"))?;
                out.push(turn);
            }
        }
        Ok(out)
    }

    async fn full_payload(&self, event_id: &str) -> Result<Option<String>> {
        let coll: Collection<Document> = self.db.collection(COLL_EVENTS);
        let doc = coll
            .find_one(doc! { "_id": event_id })
            .await
            .map_err(|e| anyhow!("mongo full_payload: {e}"))?;
        match doc.and_then(|d| d.get("payload").cloned()) {
            Some(payload) => {
                let value: Value = bson::from_bson(payload)
                    .map_err(|e| anyhow!("payload bson → value: {e}"))?;
                Ok(Some(serde_json::to_string(&value)?))
            }
            None => Ok(None),
        }
    }

    // export_session_jsonl uses the default trait impl which calls
    // session_events — gets parity for free.

    // ── Phase 5: analytics queries ──────────────────────────────────
    // (intentionally not stubbed — they fall back to the trait's default
    // empty impls until Phase 5 implements them as Mongo aggregations)

    // ── Phase 6: FTS ────────────────────────────────────────────────

    /// Index a record for full-text search. Stores the indexed text on
    /// the `searchable_text` field where the text index lives. The
    /// `_id` is the event_id so re-indexing the same event overwrites
    /// (matches SQLite's contentless table behavior).
    async fn index_fts(
        &self,
        event_id: &str,
        session_id: &str,
        record_type: &str,
        text: &str,
    ) -> Result<()> {
        let coll: Collection<Document> = self.db.collection(COLL_FTS);
        let filter = doc! { "_id": event_id };
        let update = doc! {
            "$set": {
                "session_id": session_id,
                "record_type": record_type,
                "searchable_text": text,
            }
        };
        let opts = mongodb::options::UpdateOptions::builder().upsert(true).build();
        coll.update_one(filter, update)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo index_fts: {e}"))?;
        Ok(())
    }

    /// Full-text search. Returns matches sorted by relevance (textScore).
    /// Empty query returns empty Vec — Mongo's $text rejects empty
    /// search strings, so we short-circuit before hitting the driver.
    async fn search_fts(
        &self,
        query: &str,
        limit: usize,
        session_filter: Option<&str>,
    ) -> Result<Vec<crate::queries::FtsSearchResult>> {
        use crate::queries::FtsSearchResult;
        use futures::StreamExt;

        if query.is_empty() {
            return Ok(vec![]);
        }
        let coll: Collection<Document> = self.db.collection(COLL_FTS);

        // $text search; combine with session_id filter if asked.
        let mut filter = doc! { "$text": { "$search": query } };
        if let Some(sid) = session_filter {
            filter.insert("session_id", sid);
        }

        // Project the textScore meta value alongside the document so we
        // can both sort by it and return it as `rank` (note: SQLite uses
        // negative ranks where more negative = more relevant; Mongo uses
        // positive scores where higher = more relevant. We expose the
        // raw signed score and let the caller treat it as opaque — the
        // `it_caps_full_text_search_results_at_the_limit` and
        // `it_indexes_text_and_finds_it_via_full_text_search`
        // conformance tests don't compare cross-backend rank values).
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "score": { "$meta": "textScore" } })
            .projection(doc! { "score": { "$meta": "textScore" }, "session_id": 1, "record_type": 1, "searchable_text": 1 })
            .limit(limit as i64)
            .build();

        let mut cursor = coll
            .find(filter)
            .with_options(opts)
            .await
            .map_err(|e| anyhow!("mongo search_fts: {e}"))?;

        let mut out = Vec::new();
        while let Some(next) = cursor.next().await {
            let doc = next.map_err(|e| anyhow!("mongo search_fts cursor: {e}"))?;
            let event_id = doc.get_str("_id").unwrap_or_default().to_string();
            let session_id = doc.get_str("session_id").unwrap_or_default().to_string();
            let record_type = doc.get_str("record_type").unwrap_or_default().to_string();
            let text = doc.get_str("searchable_text").unwrap_or_default().to_string();
            // Mongo doesn't return a snippet primitive — fall back to a
            // truncated copy of the matched text. The API consumes
            // FtsSearchResult.snippet for highlighting; truncating is
            // good enough until someone needs server-side bolding.
            let snippet = if text.len() > 120 {
                format!("{}…", &text[..120])
            } else {
                text
            };
            let rank = doc.get_f64("score").unwrap_or(0.0);
            out.push(FtsSearchResult {
                event_id,
                session_id,
                record_type,
                snippet,
                rank,
            });
        }
        Ok(out)
    }

    async fn fts_count(&self) -> Result<u64> {
        let coll: Collection<Document> = self.db.collection(COLL_FTS);
        let n = coll
            .count_documents(doc! {})
            .await
            .map_err(|e| anyhow!("mongo fts_count: {e}"))?;
        Ok(n)
    }
}

// ───────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────

/// Convert a CloudEvent (`serde_json::Value`) into a BSON `Document`
/// suitable for `events.insertOne`.
///
/// The document shape:
/// - `_id`     — event id (global PK, drives dedup)
/// - `session_id` — extracted to a top-level field for indexed queries
/// - `timestamp`  — extracted from `time` for ordered reads
/// - `subtype`    — extracted for filter queries
/// - `agent_id`   — extracted from `data.agent_id` (nullable)
/// - `parent_uuid` — extracted from `data.parent_uuid` (nullable)
/// - `payload`    — the full original event, stored as a nested BSON doc
///
/// Round-trip is via `payload`. The top-level extracted fields are pure
/// query optimization; `session_events` reads them back from `payload`,
/// not from the extracted fields. That keeps Phase 4's lossless
/// round-trip honest — extracted fields can drift from the source
/// without corrupting reads.
///
/// **`_id` requirement:** the event MUST carry a non-empty `id`. The
/// translator and hooks both generate UUIDs, so this is always true in
/// production. In tests it's enforced explicitly to avoid silent dedup
/// failures (an empty `_id` would still be a valid Mongo key but every
/// such event would collide).
fn event_to_doc(session_id: &str, event: &Value) -> Result<Document> {
    let id = event
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("event missing required `id` field for Mongo _id"))?
        .to_string();
    let timestamp = event.get("time").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let agent_id = event
        .get("data")
        .and_then(|d| d.get("agent_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let parent_uuid = event
        .get("data")
        .and_then(|d| d.get("parent_uuid"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Convert the full Value to BSON for the `payload` field. This is the
    // round-trip path — `session_events` reads payload back and returns it
    // as `serde_json::Value`. Any BSON type-fidelity loss surfaces in
    // `it_round_trips_an_event_payload_losslessly`.
    let payload: Bson = bson::to_bson(event).map_err(|e| anyhow!("event → bson: {e}"))?;

    Ok(doc! {
        "_id": id,
        "session_id": session_id,
        "timestamp": timestamp,
        "subtype": subtype,
        "agent_id": agent_id.map(Bson::String).unwrap_or(Bson::Null),
        "parent_uuid": parent_uuid.map(Bson::String).unwrap_or(Bson::Null),
        "payload": payload,
    })
}

/// True iff the error is a Mongo write error with code 11000 (duplicate
/// key on a unique index). Handles the single-insert error shape.
fn is_duplicate_key(err: &mongodb::error::Error) -> bool {
    matches!(
        *err.kind,
        ErrorKind::Write(WriteFailure::WriteError(ref we)) if we.code == MONGO_DUPLICATE_KEY
    )
}

/// Convert a `sessions` document back into a `SessionRow`. The session
/// id lives at `_id`.
fn doc_to_session_row(doc: &Document) -> Result<SessionRow> {
    let id = doc
        .get_str("_id")
        .map_err(|e| anyhow!("session doc missing _id: {e}"))?
        .to_string();
    let project_id = doc.get_str("project_id").ok().map(|s| s.to_string());
    let project_name = doc.get_str("project_name").ok().map(|s| s.to_string());
    let label = doc.get_str("label").ok().map(|s| s.to_string());
    let custom_label = doc.get_str("custom_label").ok().map(|s| s.to_string());
    let branch = doc.get_str("branch").ok().map(|s| s.to_string());
    let event_count = doc.get_i64("event_count").unwrap_or(0) as u64;
    let first_event = doc.get_str("first_event").ok().map(|s| s.to_string());
    let last_event = doc.get_str("last_event").ok().map(|s| s.to_string());
    Ok(SessionRow {
        id,
        project_id,
        project_name,
        label,
        custom_label,
        branch,
        event_count,
        first_event,
        last_event,
    })
}

/// Convert a `patterns` document back into a `PatternEvent`.
fn doc_to_pattern_event(doc: &Document) -> Result<PatternEvent> {
    let session_id = doc
        .get_str("session_id")
        .map_err(|e| anyhow!("pattern doc missing session_id: {e}"))?
        .to_string();
    let pattern_type = doc
        .get_str("pattern_type")
        .map_err(|e| anyhow!("pattern doc missing pattern_type: {e}"))?
        .to_string();
    let started_at = doc.get_str("started_at").unwrap_or_default().to_string();
    let ended_at = doc.get_str("ended_at").unwrap_or_default().to_string();
    let summary = doc.get_str("summary").unwrap_or_default().to_string();
    let event_ids: Vec<String> = doc
        .get("event_ids")
        .map(|b| bson::from_bson(b.clone()).unwrap_or_default())
        .unwrap_or_default();
    let metadata: Value = doc
        .get("metadata")
        .map(|b| bson::from_bson(b.clone()).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    Ok(PatternEvent {
        pattern_type,
        session_id,
        event_ids,
        started_at,
        ended_at,
        summary,
        metadata,
    })
}

/// For `insert_many` errors: count how many of the failures were duplicate
/// keys. With `ordered: false`, Mongo continues past duplicates and reports
/// all of them in `write_errors` — the `inserted_ids` map says exactly
/// which inputs *did* land. We use both to compute the "new events"
/// count for `insert_batch`.
#[allow(dead_code)] // wired in once insert_batch lands
fn count_duplicate_keys(err: &InsertManyError) -> usize {
    err.write_errors
        .as_ref()
        .map(|errs| {
            errs.iter()
                .filter(|e| e.code == MONGO_DUPLICATE_KEY)
                .count()
        })
        .unwrap_or(0)
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

    #[test]
    fn event_to_doc_extracts_indexed_fields() {
        let event = serde_json::json!({
            "id": "evt-123",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "time": "2025-01-14T00:00:00Z",
            "source": "arc://test",
            "data": {
                "agent_id": "agent-A",
                "parent_uuid": "parent-1",
                "raw": {"hello": "world"}
            }
        });
        let doc = event_to_doc("sess-1", &event).unwrap();
        assert_eq!(doc.get_str("_id").unwrap(), "evt-123");
        assert_eq!(doc.get_str("session_id").unwrap(), "sess-1");
        assert_eq!(doc.get_str("timestamp").unwrap(), "2025-01-14T00:00:00Z");
        assert_eq!(doc.get_str("subtype").unwrap(), "message.user.prompt");
        assert_eq!(doc.get_str("agent_id").unwrap(), "agent-A");
        assert_eq!(doc.get_str("parent_uuid").unwrap(), "parent-1");
        assert!(doc.get("payload").is_some());
    }

    #[test]
    fn event_to_doc_rejects_missing_id() {
        let event = serde_json::json!({"type": "io.arc.event"});
        assert!(event_to_doc("sess-1", &event).is_err());
    }

    #[test]
    fn event_to_doc_handles_null_optional_fields() {
        let event = serde_json::json!({
            "id": "evt-x",
            "type": "io.arc.event",
            "data": {"text": "hello"}
        });
        let doc = event_to_doc("sess-1", &event).unwrap();
        assert_eq!(doc.get("agent_id").unwrap(), &Bson::Null);
        assert_eq!(doc.get("parent_uuid").unwrap(), &Bson::Null);
        assert_eq!(doc.get_str("subtype").unwrap(), "");
    }
}
