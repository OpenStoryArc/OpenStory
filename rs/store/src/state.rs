//! StoreState — owns event storage, projections, patterns, and project resolution.
//!
//! This is the store-owned subset of what was previously AppState. The server
//! composes StoreState with server-specific fields (broadcast_tx, transcript_states, bus).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;

use open_story_patterns::PatternEvent;

use crate::event_store::EventStore;
use crate::payload_cache::PayloadCache;
use crate::persistence::{EventLog, SessionStore};
use crate::plan_store::PlanStore;
use crate::projection::SessionProjection;
use crate::projection_cache::ProjectionCache;
use crate::sqlite_store::SqliteStore;

// ── default cache budgets ────────────────────────────────────────────────
// These mirror the server `Config` defaults (projection_cache_bytes = 4 GB,
// working_set_days = 7, payload_cache_bytes = 256 MB). The store crate stays
// standalone (no dependency on the server's `Config`), so the legacy/test
// constructors bake in these defaults; the server overrides them at boot via
// `set_cache_budget` from the parsed config. A 4 GB projection budget means
// effectively no eviction for the small fixtures the test suite builds — the
// old unbounded-`DashMap` behavior is preserved for those callers.
const DEFAULT_PROJECTION_CACHE_BYTES: u64 = 4_000_000_000;
const DEFAULT_WORKING_SET_DAYS: u32 = 7;
const DEFAULT_PAYLOAD_CACHE_BYTES: u64 = 256_000_000;

/// Detect a subagent → parent relationship from one event and update the maps.
///
/// The convention is that an event's `data.session_id` carries the **parent**
/// session id while the surrounding context (filename / hook envelope /
/// ingest call) carries the **subagent's own** session id. When they differ,
/// the subagent has a parent; record the relationship once per subagent.
///
/// Extracted 2026-04-15 from four byte-identical copies (audit walk):
///   - rs/server/src/ingest.rs:102 (live ingest)
///   - rs/server/src/ingest.rs:350 (replay_boot_sessions)
///   - rs/server/src/state.rs:138 (boot_from_sqlite)
///   - rs/server/src/consumers/projections.rs:78 (Actor 3, dead state)
///
/// All four had the same logic with different variable names — a refactor
/// that adds a third condition (e.g., explicit subagent flag) needs to land
/// in one place, not four.
pub fn detect_subagent_relationship(
    event: &serde_json::Value,
    own_session_id: &str,
    parents: &DashMap<String, String>,
    children: &DashMap<String, Vec<String>>,
) {
    let Some(data_sid) = event
        .get("data")
        .and_then(|d| d.get("session_id"))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    if data_sid != own_session_id && !parents.contains_key(own_session_id) {
        parents.insert(own_session_id.to_string(), data_sid.to_string());
        children
            .entry(data_sid.to_string())
            .or_default()
            .push(own_session_id.to_string());
    }
}

/// Store state — event storage, projections, patterns, and project resolution.
pub struct StoreState {
    // ── event store (SQLite default, JSONL fallback) ──
    // Arc-wrapped so multiple actor-consumers can hold a reference
    // without a shared RwLock. SQLite handles internal locking.
    pub event_store: Arc<dyn EventStore>,

    pub session_store: SessionStore,
    pub event_log: EventLog,
    pub plan_store: PlanStore,

    // ── projections + patterns ──
    // Shared across actor-consumers (Actor 3 owns writes, API + ws +
    // other actors read). DashMap gives lock-free concurrent reads
    // without forcing all call sites onto an explicit RwLock guard.
    /// Bounded read-through cache of per-session projections. Was an unbounded
    /// `Arc<DashMap<String, SessionProjection>>`; now a byte-budgeted,
    /// recency-aware cache. An evicted projection is transparently re-derived
    /// from the durable event store via [`StoreState::get_or_rebuild`] — SQLite
    /// stays the source of truth, so eviction never loses data.
    pub projections: Arc<ProjectionCache>,
    /// Cache of detected patterns keyed by session_id, populated by the
    /// patterns consumer (Actor 2). Read by `build_initial_state` for the
    /// WebSocket handshake. Pattern *detection* lives in the patterns
    /// consumer; this is just the in-memory mirror it pushes into so the
    /// API/WebSocket layers can read it without a DB roundtrip.
    pub detected_patterns: Arc<DashMap<String, Vec<PatternEvent>>>,
    /// Truncation cache: `(session_id, event_id)` → full tool output.
    /// Byte-bounded LRU (`PayloadCache`); the lazy-load endpoint falls back to
    /// the durable `EventStore::full_payload` on a miss, so eviction here is
    /// safe. Populated by replay + live ingest for truncated tool outputs.
    pub full_payloads: Arc<PayloadCache>,

    // ── subagent parent-child index ──
    /// Subagent session_id → parent session_id. Shared `Arc<DashMap>`
    /// so async `replay_boot_sessions` can populate it without blocking
    /// the main AppState RwLock.
    pub subagent_parents: Arc<DashMap<String, String>>,
    /// Parent session_id → list of subagent session_ids (shared).
    pub session_children: Arc<DashMap<String, Vec<String>>>,

    // ── project resolution ──
    // Shared `Arc<DashMap>` so `ingest_events` can take `&AppState`
    // (not `&mut`). That in turn lets Actor 4's broadcast consumer use
    // a read guard on `RwLock<AppState>` instead of write, so API
    // reads aren't blocked by the consumer's per-batch writes.
    pub session_projects: Arc<DashMap<String, String>>,
    pub session_project_names: Arc<DashMap<String, String>>,
    pub watch_dir_entries: Vec<String>,

    // ── configuration ──
    pub data_dir: PathBuf,
}

/// Persistence backend selector — independent of the server crate's
/// `Config` so the store crate stays standalone. The server's
/// `DataBackend` enum maps onto this at boot time.
#[derive(Debug, Clone)]
pub enum BackendChoice {
    /// SQLite (default, no extra deps).
    Sqlite,
    /// MongoDB. Requires `--features open-story-store/mongo` at build time.
    /// `uri` is a `mongodb://...` connection string.
    Mongo { uri: String, db_name: String },
}

impl StoreState {
    /// Create a new empty StoreState backed by SQLite at `data_dir`.
    ///
    /// Tries SQLite first. Falls back to JSONL if SQLite fails. This is
    /// the legacy entry point — kept for backward compatibility with the
    /// integration test suite. New callers should prefer `with_backend`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        Self::new_with_key(data_dir, None)
    }

    /// Create a new StoreState with an optional SQLCipher encryption key.
    ///
    /// If `key` is Some and non-empty, the SQLite database is encrypted.
    /// Empty or None key = unencrypted (backward compatible).
    pub fn new_with_key(data_dir: &Path, key: Option<&str>) -> Result<Self> {
        let (event_store, session_store, event_log, plan_store) =
            init_sidecar_stores(data_dir, key)?;
        Ok(Self::assemble(
            event_store,
            session_store,
            event_log,
            plan_store,
            data_dir.to_path_buf(),
        ))
    }

    /// Create a StoreState with the chosen backend. This is the entry
    /// point used by `server::create_state` once `data_backend` is read
    /// from `Config`.
    ///
    /// SQLite path is sync-friendly (rusqlite is sync), but MongoStore
    /// boot is async (`MongoStore::connect` opens a TCP connection and
    /// runs index creation), so this constructor as a whole is async.
    /// `data_dir` is still required even when using Mongo because the
    /// JSONL backup, plans dir, and session store all live on disk
    /// regardless of which event store is durable.
    pub async fn with_backend(
        data_dir: &Path,
        key: Option<&str>,
        backend: BackendChoice,
    ) -> Result<Self> {
        // Plans, JSONL backup, and SessionStore live on disk for all
        // backends — they're the sovereignty escape hatch that survives
        // any database choice.
        let plans_dir = data_dir.join("plans");
        std::fs::create_dir_all(&plans_dir)?;
        let session_store = SessionStore::new(data_dir)?;
        let event_log = EventLog::new(data_dir)?;
        let plan_store = PlanStore::new(&plans_dir)?;

        let event_store: Arc<dyn EventStore> = match backend {
            BackendChoice::Sqlite => match SqliteStore::new_with_key(data_dir, key) {
                Ok(store) => Arc::new(store),
                Err(e) => {
                    eprintln!("SQLite unavailable ({}), falling back to JSONL store", e);
                    let fallback_session_store = SessionStore::new(data_dir)?;
                    let fallback_event_log = EventLog::new(data_dir)?;
                    Arc::new(crate::jsonl_store::JsonlStore::new(
                        fallback_session_store,
                        fallback_event_log,
                    ))
                }
            },
            #[cfg(feature = "mongo")]
            BackendChoice::Mongo { uri, db_name } => {
                use crate::mongo_store::MongoStore;
                let store = MongoStore::connect(&uri, &db_name)
                    .await
                    .map_err(|e| anyhow::anyhow!("connect MongoStore at {uri}/{db_name}: {e}"))?;
                eprintln!("  \x1b[32mEvent store: MongoDB ({db_name})\x1b[0m");
                Arc::new(store)
            }
            #[cfg(not(feature = "mongo"))]
            BackendChoice::Mongo { .. } => {
                return Err(anyhow::anyhow!(
                    "data_backend = \"mongo\" requires building with `--features open-story-store/mongo`"
                ));
            }
        };

        Ok(Self::assemble(
            event_store,
            session_store,
            event_log,
            plan_store,
            data_dir.to_path_buf(),
        ))
    }

    /// Internal: assemble a StoreState from its parts.
    fn assemble(
        event_store: Arc<dyn EventStore>,
        session_store: SessionStore,
        event_log: EventLog,
        plan_store: PlanStore,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            event_store,
            session_store,
            event_log,
            plan_store,
            projections: Arc::new(ProjectionCache::new(
                DEFAULT_PROJECTION_CACHE_BYTES,
                DEFAULT_WORKING_SET_DAYS,
            )),
            detected_patterns: Arc::new(DashMap::new()),
            full_payloads: Arc::new(PayloadCache::new(DEFAULT_PAYLOAD_CACHE_BYTES)),
            subagent_parents: Arc::new(DashMap::new()),
            session_children: Arc::new(DashMap::new()),
            session_projects: Arc::new(DashMap::new()),
            session_project_names: Arc::new(DashMap::new()),
            watch_dir_entries: Vec::new(),
            data_dir,
        }
    }

    /// Replace the projection + payload caches with ones sized from the parsed
    /// server config. The constructors bake in [`DEFAULT_PROJECTION_CACHE_BYTES`]
    /// etc. because the store crate can't see the server's `Config`; the server
    /// calls this immediately after construction (before reconcile/reproject
    /// populate the caches) to apply the operator-configured budgets. Cheap:
    /// the caches are empty at this point, so nothing is copied.
    pub fn set_cache_budget(
        &mut self,
        projection_cache_bytes: u64,
        working_set_days: u32,
        payload_cache_bytes: u64,
    ) {
        self.projections = Arc::new(ProjectionCache::new(
            projection_cache_bytes,
            working_set_days,
        ));
        self.full_payloads = Arc::new(PayloadCache::new(payload_cache_bytes));
    }

    /// Read-through accessor for a session's projection.
    ///
    /// Cache hit → return it (marking recency). Miss → re-derive from the
    /// durable event store via [`rebuild_session`](crate::rebuild::rebuild_session),
    /// insert into the cache, and return. Returns `None` only when the session
    /// has no events in the store (nothing to project). Eviction is therefore
    /// transparent to callers: a cold session reloads its full projection on
    /// demand, and SQLite remains the source of truth.
    ///
    /// Deadlock-safety: the miss path never holds a `get()` `Ref` across the
    /// `insert()`. The first `get` returns `None` (guard already dropped) before
    /// `rebuild_session`/`insert`, and only the *final* `get` produces the
    /// returned `Ref`. Holding a `Ref` across a same-cache `insert` on one
    /// thread self-deadlocks DashMap (eviction's `map.remove` needs the shard
    /// write-lock the `Ref` read-locks), so this ordering is load-bearing.
    pub async fn get_or_rebuild(
        &self,
        session_id: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, String, SessionProjection>> {
        if let Some(r) = self.projections.get(session_id) {
            return Some(r); // hit — recency marked, no Ref held past here
        }
        // Miss: no Ref is held here. Re-derive from the durable store.
        let p = crate::rebuild::rebuild_session(self.event_store.as_ref(), session_id).await?;
        // Transiently pin BEFORE inserting so the just-rebuilt entry can't
        // self-evict during its own `insert` when the projection alone exceeds
        // `projection_cache_bytes` and nothing shields it (working_set_days==0,
        // not otherwise pinned). Ref-counted pins mean this composes safely with
        // a genuine live pin: our `unpin_live` only releases OUR increment. The
        // returned `Ref` keeps the entry readable; eviction is deferred, not
        // denied — the next `insert` reclaims the bytes once it's unpinned.
        // No `Ref` is held across `pin_live`/`insert`/`unpin_live` — same
        // deadlock rule as the hit path.
        self.projections.pin_live(session_id);
        self.projections.insert(session_id.to_string(), p);
        let r = self.projections.get(session_id);
        self.projections.unpin_live(session_id);
        r
    }
}

/// Internal helper used by the legacy sync constructors. Creates the
/// SQLite-backed event store and the disk-backed sidecars in one shot.
fn init_sidecar_stores(
    data_dir: &Path,
    key: Option<&str>,
) -> Result<(Arc<dyn EventStore>, SessionStore, EventLog, PlanStore)> {
    let plans_dir = data_dir.join("plans");
    std::fs::create_dir_all(&plans_dir)?;
    let session_store = SessionStore::new(data_dir)?;
    let event_log = EventLog::new(data_dir)?;
    let plan_store = PlanStore::new(&plans_dir)?;

    let event_store: Arc<dyn EventStore> = match SqliteStore::new_with_key(data_dir, key) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            eprintln!("SQLite unavailable ({}), falling back to JSONL store", e);
            let fallback_session_store = SessionStore::new(data_dir)?;
            let fallback_event_log = EventLog::new(data_dir)?;
            Arc::new(crate::jsonl_store::JsonlStore::new(
                fallback_session_store,
                fallback_event_log,
            ))
        }
    };
    Ok((event_store, session_store, event_log, plan_store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Without the `mongo` feature, asking for the Mongo backend must
    /// fail at boot with a clear, actionable error message — never
    /// silently fall back to SQLite. The Phase 7 contract.
    #[cfg(not(feature = "mongo"))]
    #[tokio::test]
    async fn with_backend_mongo_without_feature_errors_clearly() {
        let tmp = TempDir::new().unwrap();
        let result = StoreState::with_backend(
            tmp.path(),
            None,
            BackendChoice::Mongo {
                uri: "mongodb://localhost:27017".to_string(),
                db_name: "openstory".to_string(),
            },
        )
        .await;
        // Can't use `expect_err` because StoreState doesn't impl Debug.
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("must error without the mongo feature"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("mongo") && msg.contains("feature"),
            "error message must mention the feature flag, got: {msg}"
        );
    }

    /// With or without the feature, SQLite is always selectable and
    /// always works.
    #[tokio::test]
    async fn with_backend_sqlite_works() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::with_backend(
            tmp.path(),
            None,
            BackendChoice::Sqlite,
        )
        .await
        .expect("sqlite backend must always boot");
        assert!(state.event_store.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn new_creates_empty_store() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        assert!(state.event_store.list_sessions().await.unwrap().is_empty());
        assert!(state.projections.is_empty());
        assert!(state.detected_patterns.is_empty());
        assert!(state.full_payloads.is_empty());
        assert!(state.subagent_parents.is_empty());
        assert!(state.session_children.is_empty());
        assert!(state.session_projects.is_empty());
        assert!(state.session_project_names.is_empty());
        assert!(state.watch_dir_entries.is_empty());
        assert_eq!(state.data_dir, tmp.path());
    }

    /// A well-formed CloudEvent envelope (same shape `rebuild.rs`'s own tests
    /// use — the fields `CloudEvent` requires to deserialize). Kept minimal;
    /// `event_count()` only needs a parseable event per id.
    fn test_event(id: &str, subtype: &str, time: &str, data: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": subtype,
            "time": time,
            "source": "arc://test",
            "data": data,
        })
    }

    /// Build a `StoreState` whose durable event store holds `events` under
    /// `session_id`, but whose projection cache is left cold (nothing inserted).
    async fn test_state_with_events(session_id: &str, events: &[serde_json::Value]) -> StoreState {
        let tmp = TempDir::new().unwrap();
        // Persist the TempDir so the on-disk SQLite outlives the returned state
        // for the duration of the test (the state owns no handle to keep it
        // alive). `keep` returns the path and disarms the auto-delete.
        let path = tmp.keep();
        let state = StoreState::new(&path).unwrap();
        for event in events {
            state
                .event_store
                .insert_event(session_id, event)
                .await
                .expect("insert_event");
        }
        state
    }

    /// Read-through contract: a session whose events are durably present but
    /// whose projection was never loaded (evicted / never inserted) misses on a
    /// plain `get`, is transparently rebuilt by `get_or_rebuild`, and is then
    /// resident. This is the eviction-transparency guarantee — SQLite is the
    /// source of truth and the projection re-derives on demand.
    #[tokio::test]
    async fn get_or_rebuild_reloads_evicted_session() {
        let state = test_state_with_events(
            "s1",
            &[test_event(
                "e1",
                "message.user.prompt",
                "2026-01-01T00:00:00Z",
                serde_json::json!({"text": "hi"}),
            )],
        )
        .await;

        assert!(state.projections.get("s1").is_none(), "starts cold");
        let p = state.get_or_rebuild("s1").await.expect("rebuilt from store");
        assert_eq!(p.event_count(), 1);
        drop(p);
        assert!(state.projections.contains("s1"), "now resident");

        // A session with no durable events has nothing to project → None.
        assert!(
            state.get_or_rebuild("no-such-session").await.is_none(),
            "missing session yields None, not an empty projection"
        );
    }

    /// Under real byte-budget pressure (`working_set_days == 0`, tiny budget),
    /// two things must hold:
    ///   (a) a cold, non-oversized session genuinely evicts when a second
    ///       session pushes the cache over budget; and
    ///   (b) `get_or_rebuild` STILL returns the full projection — both for the
    ///       evicted session AND for a session whose projection alone exceeds
    ///       the entire budget. The latter is the Important edge: without the
    ///       transient pin in `get_or_rebuild`, the just-rebuilt oversized entry
    ///       self-evicts during its own `insert` and the final `get` misses,
    ///       returning `None` for a session that has events.
    #[tokio::test]
    async fn get_or_rebuild_survives_eviction_and_oversized_projection() {
        // Well-formed assistant-text events (the shape that actually populates
        // `records`, so `heap_bytes()` is non-zero) — chunky text so a 3-event
        // projection has a meaningful footprint. Two same-shape sessions with
        // different ids ⇒ equal-ish projection sizes.
        let assistant_text = |id: &str, sess: &str, seq: u64, text: &str| {
            serde_json::json!({
                "specversion": "1.0",
                "id": id,
                "source": "arc://test",
                "type": "io.arc.event",
                "datacontenttype": "application/json",
                "subtype": "message.assistant.text",
                "time": format!("2026-01-01T00:00:{:02}Z", seq % 60),
                "data": {
                    "raw": {"type": "assistant", "message": {"model": "m",
                            "content": [{"type": "text", "text": text}]}},
                    "seq": seq,
                    "session_id": sess,
                    "agent_payload": {
                        "_variant": "claude-code",
                        "meta": {"agent": "claude-code"},
                        "text": text,
                    },
                },
            })
        };
        let events_for = |sess: &str| {
            let body = "x".repeat(300);
            vec![
                assistant_text(&format!("{sess}-e1"), sess, 1, &body),
                assistant_text(&format!("{sess}-e2"), sess, 2, &body),
                assistant_text(&format!("{sess}-e3"), sess, 3, &body),
            ]
        };

        let tmp = TempDir::new().unwrap();
        let path = tmp.keep();
        let mut state = StoreState::new(&path).unwrap();
        for e in events_for("a") {
            state.event_store.insert_event("a", &e).await.unwrap();
        }
        for e in events_for("b") {
            state.event_store.insert_event("b", &e).await.unwrap();
        }

        // Measure one projection's real heap size, then size the budget to hold
        // ~one but not two → a second resident session forces eviction.
        let one = crate::rebuild::rebuild_session(state.event_store.as_ref(), "a")
            .await
            .unwrap()
            .heap_bytes();
        state.set_cache_budget(one + one / 2, 0, 256_000);

        // (a) Load `a`; it fits. Loading `b` pushes over budget → the LRU cold
        //     `a` is the (unpinned, no-window) victim.
        let ra = state.get_or_rebuild("a").await.expect("a rebuilt");
        let a_count = ra.event_count();
        assert_eq!(a_count, 3);
        drop(ra); // must not hold a Ref across the next insert (deadlock rule)
        assert!(state.projections.contains("a"), "a resident after load");

        let rb = state.get_or_rebuild("b").await.expect("b rebuilt");
        assert_eq!(rb.event_count(), 3);
        drop(rb);
        assert!(state.projections.contains("b"), "b resident");
        assert!(
            !state.projections.contains("a"),
            "a evicted under budget pressure (cold, non-oversized)"
        );
        assert!(state.projections.evictions() >= 1, "a real eviction happened");

        // …and the evicted session transparently reloads with its full count.
        let ra2 = state
            .get_or_rebuild("a")
            .await
            .expect("a transparently reloaded after eviction");
        assert_eq!(ra2.event_count(), a_count, "reloaded projection is complete");
        drop(ra2);

        // (b) The Important edge: a projection larger than the ENTIRE budget.
        //     Budget of 1 byte makes every projection oversized. Without the
        //     transient pin this returns None; with it, the full projection
        //     round-trips.
        state.set_cache_budget(1, 0, 256_000);
        let rb2 = state
            .get_or_rebuild("b")
            .await
            .expect("oversized projection must still return the full projection, not None");
        assert_eq!(
            rb2.event_count(),
            3,
            "oversized session returns the FULL projection through get_or_rebuild"
        );
    }

    #[test]
    fn new_creates_plans_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let _state = StoreState::new(tmp.path()).unwrap();

        assert!(
            tmp.path().join("plans").exists(),
            "StoreState::new should create plans/ subdirectory"
        );
    }

    #[tokio::test]
    async fn ingest_event_into_store_state() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        // Simulate what ingest_events does: dedup, persist, project.
        // Event shape mirrors the typed EventData → AgentPayload model the
        // production code now expects (post-refactor): seq + session_id at
        // the data level, text inside the agent_payload.
        let event = serde_json::json!({
            "specversion": "1.0",
            "id": "evt-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-14T00:00:00Z",
            "datacontenttype": "application/json",
            "data": {
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}},
                "seq": 1,
                "session_id": "sess-1",
                "agent_payload": {
                    "_variant": "claude-code",
                    "meta": {"agent": "claude-code"},
                    "text": "hello"
                }
            }
        });

        // Dedup is now solely the EventStore PRIMARY KEY's job — the legacy
        // in-memory `seen_event_ids` HashSet was retired alongside the
        // /hooks endpoint that needed it.
        assert!(state
            .event_store
            .insert_event("sess-1", &event)
            .await
            .unwrap());
        assert!(
            !state
                .event_store
                .insert_event("sess-1", &event)
                .await
                .unwrap(),
            "dedup via PK"
        );

        let result = state
            .projections
            .append_or_insert("sess-1", |proj| proj.append(&event));

        let stored = state.event_store.session_events("sess-1").await.unwrap();
        assert_eq!(stored.len(), 1);
        let proj = state.projections.get("sess-1").unwrap();
        assert_eq!(proj.event_count(), 1);
        assert!(!result.is_empty());

        // Verify round-trip
        assert_eq!(stored[0]["id"].as_str(), Some("evt-1"));
    }

    #[test]
    fn new_persistence_is_functional() {
        let tmp = TempDir::new().unwrap();
        let state = StoreState::new(tmp.path()).unwrap();

        // SessionStore should be able to list sessions (empty)
        assert!(state.session_store.list_sessions().is_empty());

        // PlanStore should be able to list plans (empty)
        assert!(state.plan_store.list_plans().is_empty());
    }

    // ── detect_subagent_relationship — extracted from 4 call sites ────

    #[test]
    fn subagent_relationship_records_when_data_session_differs() {
        let event = serde_json::json!({"data": {"session_id": "parent-123"}});
        let parents: DashMap<String, String> = DashMap::new();
        let children: DashMap<String, Vec<String>> = DashMap::new();
        detect_subagent_relationship(&event, "agent-456", &parents, &children);

        assert_eq!(
            parents.get("agent-456").map(|r| r.value().clone()),
            Some("parent-123".to_string())
        );
        assert_eq!(
            children.get("parent-123").map(|r| r.value().clone()),
            Some(vec!["agent-456".to_string()])
        );
    }

    #[test]
    fn subagent_relationship_skips_when_data_session_matches() {
        // Normal session — its own session_id equals data.session_id.
        // No subagent relationship to record.
        let event = serde_json::json!({"data": {"session_id": "sess-1"}});
        let parents: DashMap<String, String> = DashMap::new();
        let children: DashMap<String, Vec<String>> = DashMap::new();
        detect_subagent_relationship(&event, "sess-1", &parents, &children);

        assert!(parents.is_empty());
        assert!(children.is_empty());
    }

    #[test]
    fn subagent_relationship_only_records_first_event_per_subagent() {
        // The condition `!parents.contains_key(own)` means we only record
        // once per subagent. Subsequent events from the same subagent
        // don't add duplicate child entries.
        let e1 = serde_json::json!({"data": {"session_id": "p"}});
        let e2 = serde_json::json!({"data": {"session_id": "p"}});
        let parents: DashMap<String, String> = DashMap::new();
        let children: DashMap<String, Vec<String>> = DashMap::new();

        detect_subagent_relationship(&e1, "a", &parents, &children);
        detect_subagent_relationship(&e2, "a", &parents, &children);

        assert_eq!(children.get("p").map(|r| r.value().len()), Some(1));
    }

    #[test]
    fn subagent_relationship_handles_missing_data_field() {
        // Event without data.session_id — should be a no-op, not panic.
        let event = serde_json::json!({"id": "evt-1"});
        let parents: DashMap<String, String> = DashMap::new();
        let children: DashMap<String, Vec<String>> = DashMap::new();
        detect_subagent_relationship(&event, "a", &parents, &children);

        assert!(parents.is_empty());
        assert!(children.is_empty());
    }
}
