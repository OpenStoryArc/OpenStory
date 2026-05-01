//! SqliteStore — default EventStore implementation backed by SQLite.
//!
//! Single file database at `{data_dir}/open-story.db`.
//! WAL mode for concurrent readers during writes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};

use crate::event_store::{EventStore, SessionRow};
use crate::queries::FtsSearchResult;

/// SQLite-backed event store. Default persistence layer.
///
/// Supports SQLCipher encryption when a key is provided.
/// Empty key = unencrypted (backward compatible).
pub struct SqliteStore {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open or create a SQLite database at the given path.
    ///
    /// If `key` is provided and non-empty, the database is encrypted with SQLCipher.
    /// An empty or None key opens the database unencrypted (backward compatible).
    pub fn new(data_dir: &Path) -> Result<Self> {
        Self::new_with_key(data_dir, None)
    }

    /// Open or create a SQLite database with an optional encryption key.
    ///
    /// Encryption requires the `encryption` feature flag on open-story-store,
    /// which switches rusqlite to `bundled-sqlcipher` (needs OpenSSL).
    ///
    /// Without the feature flag, the key parameter is accepted but ignored.
    /// This allows the config/CLI to accept a key without breaking the build.
    ///
    /// To build with encryption:
    ///   cargo build --features open-story-store/encryption
    pub fn new_with_key(data_dir: &Path, key: Option<&str>) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("open-story.db");
        let conn = Connection::open(&path)?;

        // Apply encryption key when the encryption feature is enabled.
        if let Some(k) = key {
            if !k.is_empty() {
                #[cfg(feature = "encryption")]
                {
                    conn.pragma_update(None, "key", k)?;
                }
                #[cfg(not(feature = "encryption"))]
                {
                    let _ = k; // suppress unused warning
                    eprintln!("  \x1b[33mWarning: db_key set but `encryption` feature not enabled\x1b[0m");
                    eprintln!("  \x1b[33mBuild with --features open-story-store/encryption for SQLCipher\x1b[0m");
                }
            }
        }

        let store = Self {
            path,
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an in-memory database. Useful for tests and the EventStore
    /// conformance suite (`store/tests/event_store_conformance.rs`).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                subtype     TEXT NOT NULL DEFAULT '',
                timestamp   TEXT NOT NULL DEFAULT '',
                agent_id    TEXT,
                parent_uuid TEXT,
                payload     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_events_subtype ON events(subtype);

            CREATE TABLE IF NOT EXISTS sessions (
                id           TEXT PRIMARY KEY,
                project_id   TEXT,
                project_name TEXT,
                label        TEXT,
                custom_label TEXT,
                branch       TEXT,
                event_count  INTEGER DEFAULT 0,
                first_event  TEXT,
                last_event   TEXT,
                host         TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_host ON sessions(host);

            CREATE TABLE IF NOT EXISTS patterns (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                type        TEXT NOT NULL,
                start_time  TEXT NOT NULL DEFAULT '',
                end_time    TEXT NOT NULL DEFAULT '',
                summary     TEXT NOT NULL DEFAULT '',
                event_ids   TEXT NOT NULL DEFAULT '[]',
                metadata    TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_patterns_session ON patterns(session_id);
            CREATE INDEX IF NOT EXISTS idx_patterns_type ON patterns(type);

            CREATE TABLE IF NOT EXISTS turns (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                turn_number INTEGER NOT NULL,
                data        TEXT NOT NULL,
                timestamp   TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);

            CREATE TABLE IF NOT EXISTS plans (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                content     TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT ''
            );",
        )?;

        // FTS5 full-text search index.
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
                event_id UNINDEXED, session_id UNINDEXED, record_type UNINDEXED, content,
                tokenize='porter unicode61'
            );",
        )?;

        // Migration: add custom_label column for existing databases.
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN custom_label TEXT");

        // Migration: add host column + index for existing databases.
        // ALTER TABLE / CREATE INDEX are idempotent-by-error — the `let _`
        // pattern ignores the "duplicate column" / "already exists" errors
        // on already-migrated databases.
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN host TEXT");
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_sessions_host ON sessions(host)");

        Ok(())
    }

    /// Path to the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Execute a read-only closure against the database connection.
    ///
    /// Used by the query module to run analytical queries without exposing
    /// the raw connection. The closure receives a `&Connection` reference.
    pub fn with_connection<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&Connection) -> T,
    {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    /// Index a record in FTS5 for full-text search.
    fn index_fts_inner(
        &self,
        event_id: &str,
        session_id: &str,
        record_type: &str,
        text: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO events_fts(event_id, session_id, record_type, content) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![event_id, session_id, record_type, text],
        )?;
        Ok(())
    }

    /// Full-text search across indexed events.
    fn search_fts_inner(
        &self,
        query: &str,
        limit: usize,
        session_filter: Option<&str>,
    ) -> Result<Vec<FtsSearchResult>> {
        if query.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().unwrap();
        if let Some(sid) = session_filter {
            let mut stmt = conn.prepare(
                "SELECT event_id, session_id, record_type,
                        snippet(events_fts, 3, '<b>', '</b>', '...', 32),
                        rank
                 FROM events_fts
                 WHERE events_fts MATCH ?1 AND session_id = ?2
                 ORDER BY rank
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(rusqlite::params![query, sid, limit as i64], |row| {
                Ok(FtsSearchResult {
                    event_id: row.get(0)?,
                    session_id: row.get(1)?,
                    record_type: row.get(2)?,
                    snippet: row.get(3)?,
                    rank: row.get(4)?,
                })
            })?;
            let results: Vec<_> = rows.filter_map(|r| r.ok()).collect();
            Ok(results)
        } else {
            let mut stmt = conn.prepare(
                "SELECT event_id, session_id, record_type,
                        snippet(events_fts, 3, '<b>', '</b>', '...', 32),
                        rank
                 FROM events_fts
                 WHERE events_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
                Ok(FtsSearchResult {
                    event_id: row.get(0)?,
                    session_id: row.get(1)?,
                    record_type: row.get(2)?,
                    snippet: row.get(3)?,
                    rank: row.get(4)?,
                })
            })?;
            let results: Vec<_> = rows.filter_map(|r| r.ok()).collect();
            Ok(results)
        }
    }

    /// Count of records in the FTS5 index (used for backfill check).
    fn fts_count_inner(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM events_fts",
            [],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Check if a table exists (for tests).
    #[cfg(test)]
    fn table_exists(&self, name: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false)
    }
}

#[async_trait]
impl EventStore for SqliteStore {
    async fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let id = event.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or_default();
        let timestamp = event.get("time").and_then(|v| v.as_str()).unwrap_or_default();
        let agent_id = event.get("data").and_then(|d| d.get("agent_id")).and_then(|v| v.as_str());
        let parent_uuid = event.get("data").and_then(|d| d.get("parent_uuid")).and_then(|v| v.as_str());
        let payload = serde_json::to_string(event)?;

        let rows = conn.execute(
            "INSERT OR IGNORE INTO events (id, session_id, subtype, timestamp, agent_id, parent_uuid, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, session_id, subtype, timestamp, agent_id, parent_uuid, payload],
        )?;

        Ok(rows > 0)
    }

    async fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut count = 0;
        for event in events {
            let id = event.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or_default();
            let timestamp = event.get("time").and_then(|v| v.as_str()).unwrap_or_default();
            let agent_id = event.get("data").and_then(|d| d.get("agent_id")).and_then(|v| v.as_str());
            let parent_uuid = event.get("data").and_then(|d| d.get("parent_uuid")).and_then(|v| v.as_str());
            let payload = serde_json::to_string(event)?;

            let rows = tx.execute(
                "INSERT OR IGNORE INTO events (id, session_id, subtype, timestamp, agent_id, parent_uuid, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, session_id, subtype, timestamp, agent_id, parent_uuid, payload],
            )?;
            if rows > 0 {
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT payload FROM events WHERE session_id = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([session_id], |row| {
            let payload: String = row.get(0)?;
            Ok(payload)
        })?;
        let mut events = Vec::new();
        for row in rows {
            let payload = row?;
            if let Ok(val) = serde_json::from_str::<Value>(&payload) {
                events.push(val);
            }
        }
        Ok(events)
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, project_name, label, custom_label, branch, event_count, first_event, last_event, host
             FROM sessions ORDER BY last_event DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                project_name: row.get(2)?,
                label: row.get(3)?,
                custom_label: row.get(4)?,
                branch: row.get(5)?,
                event_count: row.get::<_, i64>(6)? as u64,
                first_event: row.get(7)?,
                last_event: row.get(8)?,
                host: row.get(9)?,
            })
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    async fn upsert_session(&self, session: &SessionRow) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Note: custom_label is NOT included here — it's only set by
        // update_session_label(). Boot replay and live ingestion never
        // overwrite user-set custom labels.
        //
        // `host` uses COALESCE on update: once a row has a non-null host,
        // subsequent upserts without a host (e.g. a pre-migration batch
        // that lacks host on its events) must not blank it out.
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, branch, event_count, first_event, last_event, host)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                project_name = excluded.project_name,
                label = excluded.label,
                branch = excluded.branch,
                event_count = excluded.event_count,
                first_event = excluded.first_event,
                last_event = excluded.last_event,
                host = COALESCE(excluded.host, sessions.host)",
            rusqlite::params![
                session.id,
                session.project_id,
                session.project_name,
                session.label,
                session.branch,
                session.event_count as i64,
                session.first_event,
                session.last_event,
                session.host,
            ],
        )?;
        Ok(())
    }

    async fn update_session_label(&self, session_id: &str, label: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET custom_label = ?1 WHERE id = ?2",
            rusqlite::params![label, session_id],
        )?;
        Ok(())
    }

    async fn insert_pattern(&self, session_id: &str, pattern: &PatternEvent) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let metadata = serde_json::to_string(&pattern.metadata)?;
        let event_ids = serde_json::to_string(&pattern.event_ids)?;
        // Generate a deterministic ID from pattern type + start time + session
        let id = format!("{}:{}:{}", pattern.pattern_type, pattern.started_at, session_id);
        conn.execute(
            "INSERT OR IGNORE INTO patterns (id, session_id, type, start_time, end_time, metadata, summary, event_ids)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                session_id,
                pattern.pattern_type,
                pattern.started_at,
                pattern.ended_at,
                metadata,
                pattern.summary,
                event_ids,
            ],
        )?;
        Ok(())
    }

    async fn session_patterns(
        &self,
        session_id: &str,
        pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut patterns = Vec::new();

        let query = if pattern_type.is_some() {
            "SELECT session_id, type, start_time, end_time, summary, event_ids, metadata
             FROM patterns WHERE session_id = ?1 AND type = ?2"
        } else {
            "SELECT session_id, type, start_time, end_time, summary, event_ids, metadata
             FROM patterns WHERE session_id = ?1"
        };

        let mut stmt = conn.prepare(query)?;
        let rows = if let Some(ptype) = pattern_type {
            stmt.query_map(rusqlite::params![session_id, ptype], PatternRow::from_row)?
                .collect::<Vec<_>>()
        } else {
            stmt.query_map([session_id], PatternRow::from_row)?
                .collect::<Vec<_>>()
        };
        for row in rows {
            patterns.push(row?.into_pattern_event());
        }

        Ok(patterns)
    }

    async fn insert_turn(&self, session_id: &str, turn: &StructuralTurn) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let id = format!("turn:{}:{}", session_id, turn.turn_number);
        let data = serde_json::to_string(turn)?;
        conn.execute(
            "INSERT OR REPLACE INTO turns (id, session_id, turn_number, data, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, session_id, turn.turn_number, data, turn.timestamp],
        )?;
        Ok(())
    }

    async fn session_turns(&self, session_id: &str) -> Result<Vec<StructuralTurn>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT data FROM turns WHERE session_id = ?1 ORDER BY turn_number",
        )?;
        let turns = stmt
            .query_map([session_id], |row| {
                let data: String = row.get(0)?;
                Ok(data)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|data| serde_json::from_str::<StructuralTurn>(&data).ok())
            .collect();
        Ok(turns)
    }

    async fn upsert_plan(
        &self,
        plan_id: &str,
        session_id: &str,
        content: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO plans (id, session_id, content, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET content = excluded.content",
            rusqlite::params![plan_id, session_id, content, now],
        )?;
        Ok(())
    }

    async fn full_payload(&self, event_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT payload FROM events WHERE id = ?1",
            [event_id],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(payload) => Ok(Some(payload)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Query method overrides ──────────────────────────────────────

    async fn query_session_synopsis(&self, session_id: &str) -> Option<crate::queries::SessionSynopsis> {
        self.with_connection(|conn| crate::queries::session_synopsis(conn, session_id))
    }

    async fn query_tool_journey(&self, session_id: &str) -> Vec<crate::queries::ToolStep> {
        self.with_connection(|conn| crate::queries::tool_journey(conn, session_id))
    }

    async fn query_file_impact(&self, session_id: &str) -> Vec<crate::queries::FileImpact> {
        self.with_connection(|conn| crate::queries::file_impact(conn, session_id))
    }

    async fn query_session_errors(&self, session_id: &str) -> Vec<crate::queries::SessionError> {
        self.with_connection(|conn| crate::queries::session_errors(conn, session_id))
    }

    async fn query_project_pulse(&self, days: u32) -> Vec<crate::queries::ProjectPulse> {
        self.with_connection(|conn| crate::queries::project_pulse(conn, days))
    }

    async fn query_tool_evolution(&self, days: u32) -> Vec<crate::queries::ToolEvolution> {
        self.with_connection(|conn| crate::queries::tool_evolution(conn, days))
    }

    async fn query_session_efficiency(&self) -> Vec<crate::queries::SessionEfficiency> {
        self.with_connection(crate::queries::session_efficiency)
    }

    async fn query_project_context(&self, project_id: &str, limit: usize) -> Vec<crate::queries::ProjectSession> {
        self.with_connection(|conn| crate::queries::project_context(conn, project_id, limit))
    }

    async fn query_recent_files(&self, project_id: &str, session_limit: usize) -> Vec<String> {
        self.with_connection(|conn| crate::queries::recent_files(conn, project_id, session_limit))
    }

    async fn query_productivity_by_hour(&self, days: u32) -> Vec<crate::queries::HourlyActivity> {
        self.with_connection(|conn| crate::queries::productivity_by_hour(conn, days))
    }

    async fn query_token_usage(&self, days: Option<u32>, session_id: Option<&str>, model: &str) -> crate::queries::TokenUsageSummary {
        self.with_connection(|conn| crate::queries::token_usage(conn, days, session_id, model))
    }

    async fn query_daily_token_usage(&self, days: Option<u32>) -> Vec<crate::queries::DailyTokenUsage> {
        self.with_connection(|conn| crate::queries::daily_token_usage(conn, days))
    }

    // ── Lifecycle methods ───────────────────────────────────────────

    async fn delete_session(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        // Delete FTS5 entries before events (contentless table has no cascade triggers)
        conn.execute("DELETE FROM events_fts WHERE session_id = ?1", [session_id])?;
        let events_deleted: u64 = conn.execute(
            "DELETE FROM events WHERE session_id = ?1",
            [session_id],
        )? as u64;
        conn.execute("DELETE FROM patterns WHERE session_id = ?1", [session_id])?;
        conn.execute("DELETE FROM plans WHERE session_id = ?1", [session_id])?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(events_deleted)
    }

    async fn cleanup_old_sessions(&self, retention_days: u32) -> Result<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let conn = self.conn.lock().unwrap();

        // Find sessions older than cutoff
        let mut stmt = conn.prepare(
            "SELECT id FROM sessions WHERE last_event < ?1 OR (last_event IS NULL AND first_event < ?1)",
        )?;
        let session_ids: Vec<String> = stmt
            .query_map([&cutoff_str], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut total = 0u64;
        for sid in &session_ids {
            conn.execute("DELETE FROM events_fts WHERE session_id = ?1", [sid])?;
            total += conn.execute("DELETE FROM events WHERE session_id = ?1", [sid])? as u64;
            conn.execute("DELETE FROM patterns WHERE session_id = ?1", [sid])?;
            conn.execute("DELETE FROM plans WHERE session_id = ?1", [sid])?;
            conn.execute("DELETE FROM sessions WHERE id = ?1", [sid])?;
        }
        Ok(total)
    }

    async fn index_fts(&self, event_id: &str, session_id: &str, record_type: &str, text: &str) -> Result<()> {
        self.index_fts_inner(event_id, session_id, record_type, text)
    }

    async fn search_fts(&self, query: &str, limit: usize, session_filter: Option<&str>) -> Result<Vec<crate::queries::FtsSearchResult>> {
        self.search_fts_inner(query, limit, session_filter)
    }

    async fn fts_count(&self) -> Result<u64> {
        self.fts_count_inner()
    }
}

/// Helper for reading pattern rows from SQLite.
struct PatternRow {
    session_id: String,
    pattern_type: String,
    started_at: String,
    ended_at: String,
    summary: String,
    event_ids: String,
    metadata: Option<String>,
}

impl PatternRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            session_id: row.get(0)?,
            pattern_type: row.get(1)?,
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            summary: row.get(4)?,
            event_ids: row.get(5)?,
            metadata: row.get(6)?,
        })
    }

    fn into_pattern_event(self) -> PatternEvent {
        let metadata: Value = self
            .metadata
            .as_deref()
            .and_then(|m| serde_json::from_str(m).ok())
            .unwrap_or(Value::Null);
        let event_ids: Vec<String> = serde_json::from_str(&self.event_ids)
            .unwrap_or_default();

        PatternEvent {
            pattern_type: self.pattern_type,
            session_id: self.session_id,
            event_ids,
            started_at: self.started_at,
            ended_at: self.ended_at,
            summary: self.summary,
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    // ── Phase 1b: Schema creation ──

    #[test]
    fn creates_db_file() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new(tmp.path()).unwrap();
        assert!(store.path().exists());
    }

    #[test]
    fn creates_all_tables() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.table_exists("events"));
        assert!(store.table_exists("sessions"));
        assert!(store.table_exists("patterns"));
        assert!(store.table_exists("plans"));
    }

    #[test]
    fn second_open_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let _store1 = SqliteStore::new(tmp.path()).unwrap();
        let store2 = SqliteStore::new(tmp.path()).unwrap();
        assert!(store2.table_exists("events"));
    }

    // ── Phase 1c: insert_event + session_events ──

    fn test_event(id: &str, timestamp: &str) -> Value {
        json!({
            "id": id,
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "time": timestamp,
            "source": "arc://test",
            "data": { "text": "hello" }
        })
    }

    #[tokio::test]
    async fn insert_event_returns_true_for_new() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        assert!(store.insert_event("sess-1", &event).await.unwrap());
    }

    #[tokio::test]
    async fn insert_event_returns_false_for_duplicate() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        store.insert_event("sess-1", &event).await.unwrap();
        assert!(!store.insert_event("sess-1", &event).await.unwrap());
    }

    #[tokio::test]
    async fn session_events_returns_ordered_by_timestamp() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-1", &test_event("evt-2", "2025-01-14T00:00:02Z")).await.unwrap();
        store.insert_event("sess-1", &test_event("evt-1", "2025-01-14T00:00:01Z")).await.unwrap();
        store.insert_event("sess-1", &test_event("evt-3", "2025-01-14T00:00:03Z")).await.unwrap();

        let events = store.session_events("sess-1").await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["id"], "evt-1");
        assert_eq!(events[1]["id"], "evt-2");
        assert_eq!(events[2]["id"], "evt-3");
    }

    #[tokio::test]
    async fn session_events_unknown_session_returns_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.session_events("nonexistent").await.unwrap().is_empty());
    }

    // ── Phase 1d: insert_batch ──

    #[tokio::test]
    async fn insert_batch_returns_count() {
        let store = SqliteStore::in_memory().unwrap();
        let events = vec![
            test_event("evt-1", "2025-01-14T00:00:01Z"),
            test_event("evt-2", "2025-01-14T00:00:02Z"),
            test_event("evt-3", "2025-01-14T00:00:03Z"),
        ];
        assert_eq!(store.insert_batch("sess-1", &events).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn insert_batch_deduplicates() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-1", &test_event("evt-1", "2025-01-14T00:00:01Z")).await.unwrap();

        let events = vec![
            test_event("evt-1", "2025-01-14T00:00:01Z"), // duplicate
            test_event("evt-2", "2025-01-14T00:00:02Z"),
        ];
        assert_eq!(store.insert_batch("sess-1", &events).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn insert_batch_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.insert_batch("sess-1", &[]).await.unwrap(), 0);
    }

    // ── Phase 1e: list_sessions + upsert_session ──

    #[tokio::test]
    async fn list_sessions_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upsert_session_then_list() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-1".into(),
            project_id: Some("proj-1".into()),
            project_name: Some("My Project".into()),
            label: Some("fix auth bug".into()),
                custom_label: None,
            branch: Some("main".into()),
            event_count: 42,
            first_event: Some("2025-01-14T00:00:00Z".into()),
            last_event: Some("2025-01-14T01:00:00Z".into()),
            host: None,
        }).await.unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(sessions[0].label.as_deref(), Some("fix auth bug"));
        assert_eq!(sessions[0].event_count, 42);
    }

    #[tokio::test]
    async fn upsert_session_updates_existing() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-1".into(),
            project_id: None, project_name: None,
            label: Some("old label".into()),
                custom_label: None,
            branch: None,
            event_count: 10,
            first_event: None, last_event: None,
            host: None,
        }).await.unwrap();

        store.upsert_session(&SessionRow {
            id: "sess-1".into(),
            project_id: None, project_name: None,
            label: Some("new label".into()),
                custom_label: None,
            branch: Some("feature".into()),
            event_count: 20,
            first_event: None, last_event: Some("2025-01-14T02:00:00Z".into()),
            host: None,
        }).await.unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].label.as_deref(), Some("new label"));
        assert_eq!(sessions[0].branch.as_deref(), Some("feature"));
        assert_eq!(sessions[0].event_count, 20);
    }

    #[tokio::test]
    async fn upsert_session_round_trips_host() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .upsert_session(&SessionRow {
                id: "sess-host-rt".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: Some("2026-04-21T00:00:00Z".into()),
                host: Some("Maxs-Air".into()),
            })
            .await
            .unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].host.as_deref(), Some("Maxs-Air"));
    }

    #[tokio::test]
    async fn upsert_session_coalesces_host_on_update() {
        // Contract: once a row has a host, subsequent upserts without host
        // must not blank it. This protects against a pre-migration batch
        // (no host on events) clobbering a later batch's host stamp.
        let store = SqliteStore::in_memory().unwrap();
        store
            .upsert_session(&SessionRow {
                id: "sess-coalesce".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: Some("2026-04-21T00:00:00Z".into()),
                host: Some("debian-16gb-ash-1".into()),
            })
            .await
            .unwrap();

        // Second upsert with host=None — must NOT overwrite stored host.
        store
            .upsert_session(&SessionRow {
                id: "sess-coalesce".into(),
                project_id: None,
                project_name: None,
                label: Some("updated".into()),
                custom_label: None,
                branch: None,
                event_count: 5,
                first_event: None,
                last_event: Some("2026-04-21T00:01:00Z".into()),
                host: None,
            })
            .await
            .unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions[0].host.as_deref(), Some("debian-16gb-ash-1"));
        assert_eq!(sessions[0].label.as_deref(), Some("updated"));
    }

    #[tokio::test]
    async fn upsert_session_handles_none_host() {
        let store = SqliteStore::in_memory().unwrap();
        store
            .upsert_session(&SessionRow {
                id: "sess-no-host".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: Some("2026-04-21T00:00:00Z".into()),
                host: None,
            })
            .await
            .unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].host.is_none());
    }

    #[tokio::test]
    async fn list_sessions_sorted_by_last_event_desc() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_session(&SessionRow {
            id: "old".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 0,
                custom_label: None,
            first_event: None, last_event: Some("2025-01-13T00:00:00Z".into()),
            host: None,
        }).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "new".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 0,
                custom_label: None,
            first_event: None, last_event: Some("2025-01-14T00:00:00Z".into()),
            host: None,
        }).await.unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions[0].id, "new");
        assert_eq!(sessions[1].id, "old");
    }

    // ── Phase 1f: insert_pattern + session_patterns ──

    fn test_pattern(ptype: &str, started_at: &str) -> PatternEvent {
        PatternEvent {
            pattern_type: ptype.to_string(),
            session_id: "sess-1".to_string(),
            event_ids: vec!["evt-1".into(), "evt-2".into()],
            started_at: started_at.to_string(),
            ended_at: "2025-01-14T00:01:00Z".to_string(),
            summary: format!("{} pattern", ptype),
            metadata: json!({"key": "value"}),
        }
    }

    #[tokio::test]
    async fn insert_and_query_pattern() {
        let store = SqliteStore::in_memory().unwrap();
        let pattern = test_pattern("test_cycle", "2025-01-14T00:00:00Z");
        store.insert_pattern("sess-1", &pattern).await.unwrap();

        let patterns = store.session_patterns("sess-1", None).await.unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, "test_cycle");
        assert_eq!(patterns[0].event_ids, vec!["evt-1", "evt-2"]);
        assert_eq!(patterns[0].metadata["key"], "value");
    }

    #[tokio::test]
    async fn session_patterns_filter_by_type() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_pattern("sess-1", &test_pattern("test_cycle", "2025-01-14T00:00:00Z")).await.unwrap();
        store.insert_pattern("sess-1", &test_pattern("error_recovery", "2025-01-14T00:00:01Z")).await.unwrap();
        store.insert_pattern("sess-1", &test_pattern("test_cycle", "2025-01-14T00:00:02Z")).await.unwrap();

        let filtered = store.session_patterns("sess-1", Some("test_cycle")).await.unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|p| p.pattern_type == "test_cycle"));

        let all = store.session_patterns("sess-1", None).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn session_patterns_empty_session() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.session_patterns("nonexistent", None).await.unwrap().is_empty());
    }

    // ── Phase 1g: upsert_plan + full_payload ──

    #[tokio::test]
    async fn upsert_and_retrieve_plan() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_plan("plan-1", "sess-1", "# My Plan\n\nStep 1...").await.unwrap();

        let conn = store.conn.lock().unwrap();
        let content: String = conn.query_row(
            "SELECT content FROM plans WHERE id = ?1",
            ["plan-1"],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(content, "# My Plan\n\nStep 1...");
    }

    #[tokio::test]
    async fn upsert_plan_updates_content() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_plan("plan-1", "sess-1", "v1").await.unwrap();
        store.upsert_plan("plan-1", "sess-1", "v2").await.unwrap();

        let conn = store.conn.lock().unwrap();
        let content: String = conn.query_row(
            "SELECT content FROM plans WHERE id = ?1",
            ["plan-1"],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(content, "v2");
    }

    #[tokio::test]
    async fn full_payload_returns_event_json() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        store.insert_event("sess-1", &event).await.unwrap();

        let payload = store.full_payload("evt-1").await.unwrap().unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["id"], "evt-1");
    }

    #[tokio::test]
    async fn full_payload_missing_returns_none() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.full_payload("nonexistent").await.unwrap().is_none());
    }

    // ── Lifecycle: delete_session ────────────────────────────────────

    #[tokio::test]
    async fn delete_session_removes_all_data() {
        let store = SqliteStore::in_memory().unwrap();

        // Insert events, session, pattern, plan
        store.insert_event("sess-del", &test_event("evt-d1", "2025-01-14T00:00:00Z")).await.unwrap();
        store.insert_event("sess-del", &test_event("evt-d2", "2025-01-14T00:00:01Z")).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-del".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 2,
                custom_label: None,
            first_event: None, last_event: None,
            host: None,
        }).await.unwrap();
        store.upsert_plan("plan-del", "sess-del", "plan content").await.unwrap();

        let deleted = store.delete_session("sess-del").await.unwrap();
        assert_eq!(deleted, 2, "should delete 2 events");
        assert!(store.session_events("sess-del").await.unwrap().is_empty());
        assert!(store.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_session_does_not_affect_other_sessions() {
        let store = SqliteStore::in_memory().unwrap();

        store.insert_event("sess-keep", &test_event("evt-k1", "2025-01-14T00:00:00Z")).await.unwrap();
        store.insert_event("sess-del2", &test_event("evt-d1", "2025-01-14T00:00:00Z")).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-keep".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
            host: None,
        }).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-del2".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
            host: None,
        }).await.unwrap();

        store.delete_session("sess-del2").await.unwrap();

        assert_eq!(store.session_events("sess-keep").await.unwrap().len(), 1);
        assert_eq!(store.list_sessions().await.unwrap().len(), 1);
        assert_eq!(store.list_sessions().await.unwrap()[0].id, "sess-keep");
    }

    #[tokio::test]
    async fn delete_nonexistent_session_returns_zero() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.delete_session("nonexistent").await.unwrap(), 0);
    }

    // ── Lifecycle: export_session_jsonl ──────────────────────────────

    #[tokio::test]
    async fn export_session_jsonl_returns_newline_delimited() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-exp", &test_event("evt-e1", "2025-01-14T00:00:01Z")).await.unwrap();
        store.insert_event("sess-exp", &test_event("evt-e2", "2025-01-14T00:00:02Z")).await.unwrap();

        let jsonl = store.export_session_jsonl("sess-exp").await.unwrap();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let val: Value = serde_json::from_str(line).unwrap();
            assert!(val.get("id").is_some());
        }
    }

    #[tokio::test]
    async fn export_empty_session_returns_empty_string() {
        let store = SqliteStore::in_memory().unwrap();
        let jsonl = store.export_session_jsonl("nonexistent").await.unwrap();
        assert!(jsonl.is_empty());
    }

    // ── Lifecycle: cleanup_old_sessions ──────────────────────────────

    #[tokio::test]
    async fn cleanup_old_sessions_removes_stale() {
        let store = SqliteStore::in_memory().unwrap();

        // Old session (90 days ago)
        store.insert_event("sess-old", &test_event("evt-old", "2025-12-01T00:00:00Z")).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-old".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some("2025-12-01T00:00:00Z".into()),
            last_event: Some("2025-12-01T00:00:00Z".into()),
            host: None,
        }).await.unwrap();

        // Recent session
        let now = chrono::Utc::now().to_rfc3339();
        store.insert_event("sess-new", &test_event("evt-new", &now)).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-new".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some(now.clone()),
            last_event: Some(now),
            host: None,
        }).await.unwrap();

        let deleted = store.cleanup_old_sessions(30).await.unwrap();
        assert_eq!(deleted, 1, "should delete 1 old event");
        assert_eq!(store.list_sessions().await.unwrap().len(), 1);
        assert_eq!(store.list_sessions().await.unwrap()[0].id, "sess-new");
    }

    #[tokio::test]
    async fn cleanup_with_no_old_sessions_deletes_nothing() {
        let store = SqliteStore::in_memory().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        store.insert_event("sess-recent", &test_event("evt-r", &now)).await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-recent".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some(now.clone()),
            last_event: Some(now),
            host: None,
        }).await.unwrap();

        let deleted = store.cleanup_old_sessions(7).await.unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(store.list_sessions().await.unwrap().len(), 1);
    }

    // ── Encryption key API (SQLCipher when available) ──────────────

    #[tokio::test]
    async fn new_with_key_accepts_key_parameter() {
        // With bundled (non-cipher) SQLite, key is accepted but ignored.
        // With bundled-sqlcipher, the DB would actually be encrypted.
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), Some("test-key-123")).unwrap();

        store.insert_event("sess-enc", &test_event("evt-enc-1", "2025-01-14T00:00:00Z")).await.unwrap();
        let events = store.session_events("sess-enc").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "evt-enc-1");
    }

    #[tokio::test]
    async fn new_with_key_none_is_same_as_new() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), None).unwrap();
        store.insert_event("sess-nk", &test_event("evt-nk", "2025-01-14T00:00:00Z")).await.unwrap();

        // Readable by new() (no key)
        let store2 = SqliteStore::new(tmp.path()).unwrap();
        assert_eq!(store2.session_events("sess-nk").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn new_with_empty_key_is_same_as_no_key() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), Some("")).unwrap();
        store.insert_event("sess-ek", &test_event("evt-ek", "2025-01-14T00:00:00Z")).await.unwrap();

        let store2 = SqliteStore::new(tmp.path()).unwrap();
        let events = store2.session_events("sess-ek").await.unwrap();
        assert_eq!(events.len(), 1);
    }

    // ── FTS5 full-text search tests ──

    #[tokio::test]
    async fn fts5_index_and_search() {
        let store = SqliteStore::in_memory().unwrap();
        store.index_fts("evt-1", "sess-1", "user_message", "fix the authentication bug").await.unwrap();
        store.index_fts("evt-2", "sess-1", "assistant_message", "I will fix the login flow").await.unwrap();

        let results = store.search_fts("authentication", 10, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_id, "evt-1");
        assert_eq!(results[0].session_id, "sess-1");
        assert_eq!(results[0].record_type, "user_message");
    }

    #[tokio::test]
    async fn fts5_porter_stemming() {
        let store = SqliteStore::in_memory().unwrap();
        store.index_fts("evt-1", "sess-1", "user_message", "debugging the test failures").await.unwrap();

        // "debug" should match "debugging" via porter stemmer
        let results = store.search_fts("debug", 10, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_id, "evt-1");
    }

    #[tokio::test]
    async fn fts5_session_filter() {
        let store = SqliteStore::in_memory().unwrap();
        store.index_fts("evt-1", "sess-1", "user_message", "deploy the application").await.unwrap();
        store.index_fts("evt-2", "sess-2", "user_message", "deploy the database").await.unwrap();

        let results = store.search_fts("deploy", 10, Some("sess-1")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "sess-1");
    }

    #[tokio::test]
    async fn fts5_empty_query_returns_empty() {
        let store = SqliteStore::in_memory().unwrap();
        store.index_fts("evt-1", "sess-1", "user_message", "hello world").await.unwrap();

        let results = store.search_fts("", 10, None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fts5_no_match_returns_empty() {
        let store = SqliteStore::in_memory().unwrap();
        store.index_fts("evt-1", "sess-1", "user_message", "hello world").await.unwrap();

        let results = store.search_fts("nonexistent", 10, None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fts5_delete_session_removes_entries() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-fts", &test_event("evt-fts1", "2025-01-14T00:00:00Z")).await.unwrap();
        store.index_fts("evt-fts1", "sess-fts", "user_message", "search test content").await.unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-fts".into(), project_id: None, project_name: None,
            label: None, custom_label: None, branch: None, event_count: 1,
            first_event: None, last_event: None,
            host: None,
        }).await.unwrap();

        // Verify it's searchable
        assert_eq!(store.search_fts("search", 10, None).await.unwrap().len(), 1);

        // Delete session
        store.delete_session("sess-fts").await.unwrap();

        // Verify FTS entries are gone
        assert!(store.search_fts("search", 10, None).await.unwrap().is_empty());
        assert_eq!(store.fts_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn fts5_rank_ordering() {
        let store = SqliteStore::in_memory().unwrap();
        // evt-2 has "rust" twice, should rank higher
        store.index_fts("evt-1", "sess-1", "user_message", "learn rust programming").await.unwrap();
        store.index_fts("evt-2", "sess-1", "assistant_message", "rust is great for rust projects").await.unwrap();

        let results = store.search_fts("rust", 10, None).await.unwrap();
        assert_eq!(results.len(), 2);
        // FTS5 rank is negative (more negative = more relevant), so first result should be more relevant
        assert!(results[0].rank <= results[1].rank);
    }

    #[tokio::test]
    async fn fts5_count() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.fts_count().await.unwrap(), 0);

        store.index_fts("evt-1", "sess-1", "user_message", "hello").await.unwrap();
        store.index_fts("evt-2", "sess-1", "user_message", "world").await.unwrap();
        assert_eq!(store.fts_count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn fts5_limit_respected() {
        let store = SqliteStore::in_memory().unwrap();
        for i in 0..10 {
            store.index_fts(&format!("evt-{i}"), "sess-1", "user_message", "common search term").await.unwrap();
        }

        let results = store.search_fts("common", 3, None).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    // ── T6: agent payload round-trip (architecture audit) ──────────────
    // The existing `it_round_trips_an_event_payload_losslessly` in the
    // conformance suite uses a generic event. These tests round-trip a
    // fully-populated AgentPayload per variant, catching regressions in
    // serde tag/rename attrs, missing #[serde(default)], and silent
    // field loss. See docs/research/architecture-audit/T6_SQLITE_ROUND_TRIP.md

    async fn round_trip_event(event: Value) -> Value {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("t6-sess", &event).await.unwrap();
        let events = store.session_events("t6-sess").await.unwrap();
        assert_eq!(events.len(), 1);
        events.into_iter().next().unwrap()
    }

    #[tokio::test]
    async fn t6_pi_mono_agent_payload_round_trips() {
        let event = json!({
            "id": "t6-pi-1",
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "time": "2026-04-14T12:00:00Z",
            "source": "arc://test",
            "agent": "pi-mono",
            "data": {
                "seq": 1,
                "session_id": "t6-sess",
                "raw": {"type": "message", "message": {"role": "assistant"}},
                "agent_payload": {
                    "_variant": "pi-mono",
                    "meta": {"agent": "pi-mono"},
                    "uuid": "a1f500c7",
                    "parent_uuid": "775d79e9",
                    "model": "claude-opus-4-6",
                    "tool": "read",
                    "tool_call_id": "toolu_01XoH5S",
                    "args": {"path": "/tmp/config.toml"},
                    "stop_reason": "toolUse"
                }
            }
        });
        let back = round_trip_event(event.clone()).await;
        let ap = &back["data"]["agent_payload"];
        assert_eq!(ap["_variant"], "pi-mono", "variant tag must survive");
        assert_eq!(ap["tool"], "read", "tool field must survive");
        assert_eq!(ap["tool_call_id"], "toolu_01XoH5S");
        assert_eq!(ap["args"]["path"], "/tmp/config.toml");
        assert_eq!(ap["parent_uuid"], "775d79e9");

        // Deserialize back into a typed CloudEvent to prove the enum
        // variant resolves correctly.
        let ce: open_story_core::cloud_event::CloudEvent =
            serde_json::from_value(back).expect("must deserialize as CloudEvent");
        match ce.data.agent_payload {
            Some(open_story_core::event_data::AgentPayload::PiMono(p)) => {
                assert_eq!(p.tool.as_deref(), Some("read"));
                assert_eq!(p.tool_call_id.as_deref(), Some("toolu_01XoH5S"));
            }
            other => panic!("expected PiMono variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn t6_claude_code_agent_payload_round_trips() {
        let event = json!({
            "id": "t6-cc-1",
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "time": "2026-04-14T12:00:01Z",
            "source": "arc://test",
            "agent": "claude-code",
            "data": {
                "seq": 1,
                "session_id": "t6-sess",
                "raw": {},
                "agent_payload": {
                    "_variant": "claude-code",
                    "meta": {"agent": "claude-code"},
                    "uuid": "cc-uuid-1",
                    "parent_uuid": "cc-parent",
                    "tool": "Bash",
                    "args": {"command": "ls"}
                }
            }
        });
        let back = round_trip_event(event).await;
        let ce: open_story_core::cloud_event::CloudEvent =
            serde_json::from_value(back).expect("must deserialize as CloudEvent");
        match ce.data.agent_payload {
            Some(open_story_core::event_data::AgentPayload::ClaudeCode(p)) => {
                assert_eq!(p.tool.as_deref(), Some("Bash"));
                assert_eq!(p.args.as_ref().and_then(|a| a.get("command")).and_then(|v| v.as_str()), Some("ls"));
            }
            other => panic!("expected ClaudeCode variant, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn t6_hermes_agent_payload_round_trips() {
        let event = json!({
            "id": "t6-hm-1",
            "specversion": "1.0",
            "datacontenttype": "application/json",
            "type": "io.arc.event",
            "subtype": "message.assistant.tool_use",
            "time": "2026-04-14T12:00:02Z",
            "source": "arc://test",
            "agent": "hermes",
            "data": {
                "seq": 1,
                "session_id": "t6-sess",
                "raw": {},
                "agent_payload": {
                    "_variant": "hermes",
                    "meta": {"agent": "hermes"},
                    "tool": "read_file",
                    "tool_use_id": "hm-call-1",
                    "args": {"path": "foo.rs"}
                }
            }
        });
        let back = round_trip_event(event).await;
        let ce: open_story_core::cloud_event::CloudEvent =
            serde_json::from_value(back).expect("must deserialize as CloudEvent");
        match ce.data.agent_payload {
            Some(open_story_core::event_data::AgentPayload::Hermes(p)) => {
                assert_eq!(p.tool.as_deref(), Some("read_file"));
                assert_eq!(p.tool_use_id.as_deref(), Some("hm-call-1"));
            }
            other => panic!("expected Hermes variant, got {:?}", other),
        }
    }
}
