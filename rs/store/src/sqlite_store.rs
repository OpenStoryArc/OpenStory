//! SqliteStore — default EventStore implementation backed by SQLite.
//!
//! Single file database at `{data_dir}/open-story.db`.
//! WAL mode for concurrent readers during writes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

use open_story_patterns::PatternEvent;

use crate::event_store::{EventStore, SessionRow};

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

    /// Open an in-memory database (for tests).
    #[cfg(test)]
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
                last_event   TEXT
            );

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

            CREATE TABLE IF NOT EXISTS plans (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                content     TEXT NOT NULL,
                created_at  TEXT NOT NULL DEFAULT ''
            );",
        )?;

        // Migration: add custom_label column for existing databases.
        let _ = conn.execute_batch("ALTER TABLE sessions ADD COLUMN custom_label TEXT");

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

impl EventStore for SqliteStore {
    fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool> {
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

    fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize> {
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

    fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
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

    fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, project_name, label, custom_label, branch, event_count, first_event, last_event
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
            })
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    fn upsert_session(&self, session: &SessionRow) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Note: custom_label is NOT included here — it's only set by
        // update_session_label(). Boot replay and live ingestion never
        // overwrite user-set custom labels.
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, branch, event_count, first_event, last_event)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                project_id = excluded.project_id,
                project_name = excluded.project_name,
                label = excluded.label,
                branch = excluded.branch,
                event_count = excluded.event_count,
                first_event = excluded.first_event,
                last_event = excluded.last_event",
            rusqlite::params![
                session.id,
                session.project_id,
                session.project_name,
                session.label,
                session.branch,
                session.event_count as i64,
                session.first_event,
                session.last_event,
            ],
        )?;
        Ok(())
    }

    fn update_session_label(&self, session_id: &str, label: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET custom_label = ?1 WHERE id = ?2",
            rusqlite::params![label, session_id],
        )?;
        Ok(())
    }

    fn insert_pattern(&self, session_id: &str, pattern: &PatternEvent) -> Result<()> {
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

    fn session_patterns(
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

    fn upsert_plan(
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

    fn full_payload(&self, event_id: &str) -> Result<Option<String>> {
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

    fn query_session_synopsis(&self, session_id: &str) -> Option<crate::queries::SessionSynopsis> {
        self.with_connection(|conn| crate::queries::session_synopsis(conn, session_id))
    }

    fn query_tool_journey(&self, session_id: &str) -> Vec<crate::queries::ToolStep> {
        self.with_connection(|conn| crate::queries::tool_journey(conn, session_id))
    }

    fn query_file_impact(&self, session_id: &str) -> Vec<crate::queries::FileImpact> {
        self.with_connection(|conn| crate::queries::file_impact(conn, session_id))
    }

    fn query_session_errors(&self, session_id: &str) -> Vec<crate::queries::SessionError> {
        self.with_connection(|conn| crate::queries::session_errors(conn, session_id))
    }

    fn query_project_pulse(&self, days: u32) -> Vec<crate::queries::ProjectPulse> {
        self.with_connection(|conn| crate::queries::project_pulse(conn, days))
    }

    fn query_tool_evolution(&self, days: u32) -> Vec<crate::queries::ToolEvolution> {
        self.with_connection(|conn| crate::queries::tool_evolution(conn, days))
    }

    fn query_session_efficiency(&self) -> Vec<crate::queries::SessionEfficiency> {
        self.with_connection(crate::queries::session_efficiency)
    }

    fn query_project_context(&self, project_id: &str, limit: usize) -> Vec<crate::queries::ProjectSession> {
        self.with_connection(|conn| crate::queries::project_context(conn, project_id, limit))
    }

    fn query_recent_files(&self, project_id: &str, session_limit: usize) -> Vec<String> {
        self.with_connection(|conn| crate::queries::recent_files(conn, project_id, session_limit))
    }

    fn query_productivity_by_hour(&self, days: u32) -> Vec<crate::queries::HourlyActivity> {
        self.with_connection(|conn| crate::queries::productivity_by_hour(conn, days))
    }

    fn query_token_usage(&self, days: Option<u32>, session_id: Option<&str>, model: &str) -> crate::queries::TokenUsageSummary {
        self.with_connection(|conn| crate::queries::token_usage(conn, days, session_id, model))
    }

    fn query_daily_token_usage(&self, days: Option<u32>) -> Vec<crate::queries::DailyTokenUsage> {
        self.with_connection(|conn| crate::queries::daily_token_usage(conn, days))
    }

    // ── Lifecycle methods ───────────────────────────────────────────

    fn delete_session(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let events_deleted: u64 = conn.execute(
            "DELETE FROM events WHERE session_id = ?1",
            [session_id],
        )? as u64;
        conn.execute("DELETE FROM patterns WHERE session_id = ?1", [session_id])?;
        conn.execute("DELETE FROM plans WHERE session_id = ?1", [session_id])?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(events_deleted)
    }

    fn cleanup_old_sessions(&self, retention_days: u32) -> Result<u64> {
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
            total += conn.execute("DELETE FROM events WHERE session_id = ?1", [sid])? as u64;
            conn.execute("DELETE FROM patterns WHERE session_id = ?1", [sid])?;
            conn.execute("DELETE FROM plans WHERE session_id = ?1", [sid])?;
            conn.execute("DELETE FROM sessions WHERE id = ?1", [sid])?;
        }
        Ok(total)
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
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "time": timestamp,
            "source": "arc://test",
            "data": { "text": "hello" }
        })
    }

    #[test]
    fn insert_event_returns_true_for_new() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        assert!(store.insert_event("sess-1", &event).unwrap());
    }

    #[test]
    fn insert_event_returns_false_for_duplicate() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        store.insert_event("sess-1", &event).unwrap();
        assert!(!store.insert_event("sess-1", &event).unwrap());
    }

    #[test]
    fn session_events_returns_ordered_by_timestamp() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-1", &test_event("evt-2", "2025-01-14T00:00:02Z")).unwrap();
        store.insert_event("sess-1", &test_event("evt-1", "2025-01-14T00:00:01Z")).unwrap();
        store.insert_event("sess-1", &test_event("evt-3", "2025-01-14T00:00:03Z")).unwrap();

        let events = store.session_events("sess-1").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["id"], "evt-1");
        assert_eq!(events[1]["id"], "evt-2");
        assert_eq!(events[2]["id"], "evt-3");
    }

    #[test]
    fn session_events_unknown_session_returns_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.session_events("nonexistent").unwrap().is_empty());
    }

    // ── Phase 1d: insert_batch ──

    #[test]
    fn insert_batch_returns_count() {
        let store = SqliteStore::in_memory().unwrap();
        let events = vec![
            test_event("evt-1", "2025-01-14T00:00:01Z"),
            test_event("evt-2", "2025-01-14T00:00:02Z"),
            test_event("evt-3", "2025-01-14T00:00:03Z"),
        ];
        assert_eq!(store.insert_batch("sess-1", &events).unwrap(), 3);
    }

    #[test]
    fn insert_batch_deduplicates() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-1", &test_event("evt-1", "2025-01-14T00:00:01Z")).unwrap();

        let events = vec![
            test_event("evt-1", "2025-01-14T00:00:01Z"), // duplicate
            test_event("evt-2", "2025-01-14T00:00:02Z"),
        ];
        assert_eq!(store.insert_batch("sess-1", &events).unwrap(), 1);
    }

    #[test]
    fn insert_batch_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.insert_batch("sess-1", &[]).unwrap(), 0);
    }

    // ── Phase 1e: list_sessions + upsert_session ──

    #[test]
    fn list_sessions_empty() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn upsert_session_then_list() {
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
        }).unwrap();

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(sessions[0].label.as_deref(), Some("fix auth bug"));
        assert_eq!(sessions[0].event_count, 42);
    }

    #[test]
    fn upsert_session_updates_existing() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-1".into(),
            project_id: None, project_name: None,
            label: Some("old label".into()),
                custom_label: None,
            branch: None,
            event_count: 10,
            first_event: None, last_event: None,
        }).unwrap();

        store.upsert_session(&SessionRow {
            id: "sess-1".into(),
            project_id: None, project_name: None,
            label: Some("new label".into()),
                custom_label: None,
            branch: Some("feature".into()),
            event_count: 20,
            first_event: None, last_event: Some("2025-01-14T02:00:00Z".into()),
        }).unwrap();

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].label.as_deref(), Some("new label"));
        assert_eq!(sessions[0].branch.as_deref(), Some("feature"));
        assert_eq!(sessions[0].event_count, 20);
    }

    #[test]
    fn list_sessions_sorted_by_last_event_desc() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_session(&SessionRow {
            id: "old".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 0,
                custom_label: None,
            first_event: None, last_event: Some("2025-01-13T00:00:00Z".into()),
        }).unwrap();
        store.upsert_session(&SessionRow {
            id: "new".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 0,
                custom_label: None,
            first_event: None, last_event: Some("2025-01-14T00:00:00Z".into()),
        }).unwrap();

        let sessions = store.list_sessions().unwrap();
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

    #[test]
    fn insert_and_query_pattern() {
        let store = SqliteStore::in_memory().unwrap();
        let pattern = test_pattern("test_cycle", "2025-01-14T00:00:00Z");
        store.insert_pattern("sess-1", &pattern).unwrap();

        let patterns = store.session_patterns("sess-1", None).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, "test_cycle");
        assert_eq!(patterns[0].event_ids, vec!["evt-1", "evt-2"]);
        assert_eq!(patterns[0].metadata["key"], "value");
    }

    #[test]
    fn session_patterns_filter_by_type() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_pattern("sess-1", &test_pattern("test_cycle", "2025-01-14T00:00:00Z")).unwrap();
        store.insert_pattern("sess-1", &test_pattern("error_recovery", "2025-01-14T00:00:01Z")).unwrap();
        store.insert_pattern("sess-1", &test_pattern("test_cycle", "2025-01-14T00:00:02Z")).unwrap();

        let filtered = store.session_patterns("sess-1", Some("test_cycle")).unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|p| p.pattern_type == "test_cycle"));

        let all = store.session_patterns("sess-1", None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn session_patterns_empty_session() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.session_patterns("nonexistent", None).unwrap().is_empty());
    }

    // ── Phase 1g: upsert_plan + full_payload ──

    #[test]
    fn upsert_and_retrieve_plan() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_plan("plan-1", "sess-1", "# My Plan\n\nStep 1...").unwrap();

        let conn = store.conn.lock().unwrap();
        let content: String = conn.query_row(
            "SELECT content FROM plans WHERE id = ?1",
            ["plan-1"],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(content, "# My Plan\n\nStep 1...");
    }

    #[test]
    fn upsert_plan_updates_content() {
        let store = SqliteStore::in_memory().unwrap();
        store.upsert_plan("plan-1", "sess-1", "v1").unwrap();
        store.upsert_plan("plan-1", "sess-1", "v2").unwrap();

        let conn = store.conn.lock().unwrap();
        let content: String = conn.query_row(
            "SELECT content FROM plans WHERE id = ?1",
            ["plan-1"],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(content, "v2");
    }

    #[test]
    fn full_payload_returns_event_json() {
        let store = SqliteStore::in_memory().unwrap();
        let event = test_event("evt-1", "2025-01-14T00:00:00Z");
        store.insert_event("sess-1", &event).unwrap();

        let payload = store.full_payload("evt-1").unwrap().unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["id"], "evt-1");
    }

    #[test]
    fn full_payload_missing_returns_none() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.full_payload("nonexistent").unwrap().is_none());
    }

    // ── Lifecycle: delete_session ────────────────────────────────────

    #[test]
    fn delete_session_removes_all_data() {
        let store = SqliteStore::in_memory().unwrap();

        // Insert events, session, pattern, plan
        store.insert_event("sess-del", &test_event("evt-d1", "2025-01-14T00:00:00Z")).unwrap();
        store.insert_event("sess-del", &test_event("evt-d2", "2025-01-14T00:00:01Z")).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-del".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 2,
                custom_label: None,
            first_event: None, last_event: None,
        }).unwrap();
        store.upsert_plan("plan-del", "sess-del", "plan content").unwrap();

        let deleted = store.delete_session("sess-del").unwrap();
        assert_eq!(deleted, 2, "should delete 2 events");
        assert!(store.session_events("sess-del").unwrap().is_empty());
        assert!(store.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn delete_session_does_not_affect_other_sessions() {
        let store = SqliteStore::in_memory().unwrap();

        store.insert_event("sess-keep", &test_event("evt-k1", "2025-01-14T00:00:00Z")).unwrap();
        store.insert_event("sess-del2", &test_event("evt-d1", "2025-01-14T00:00:00Z")).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-keep".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
        }).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-del2".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
        }).unwrap();

        store.delete_session("sess-del2").unwrap();

        assert_eq!(store.session_events("sess-keep").unwrap().len(), 1);
        assert_eq!(store.list_sessions().unwrap().len(), 1);
        assert_eq!(store.list_sessions().unwrap()[0].id, "sess-keep");
    }

    #[test]
    fn delete_nonexistent_session_returns_zero() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.delete_session("nonexistent").unwrap(), 0);
    }

    // ── Lifecycle: export_session_jsonl ──────────────────────────────

    #[test]
    fn export_session_jsonl_returns_newline_delimited() {
        let store = SqliteStore::in_memory().unwrap();
        store.insert_event("sess-exp", &test_event("evt-e1", "2025-01-14T00:00:01Z")).unwrap();
        store.insert_event("sess-exp", &test_event("evt-e2", "2025-01-14T00:00:02Z")).unwrap();

        let jsonl = store.export_session_jsonl("sess-exp").unwrap();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let val: Value = serde_json::from_str(line).unwrap();
            assert!(val.get("id").is_some());
        }
    }

    #[test]
    fn export_empty_session_returns_empty_string() {
        let store = SqliteStore::in_memory().unwrap();
        let jsonl = store.export_session_jsonl("nonexistent").unwrap();
        assert!(jsonl.is_empty());
    }

    // ── Lifecycle: cleanup_old_sessions ──────────────────────────────

    #[test]
    fn cleanup_old_sessions_removes_stale() {
        let store = SqliteStore::in_memory().unwrap();

        // Old session (90 days ago)
        store.insert_event("sess-old", &test_event("evt-old", "2025-12-01T00:00:00Z")).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-old".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some("2025-12-01T00:00:00Z".into()),
            last_event: Some("2025-12-01T00:00:00Z".into()),
        }).unwrap();

        // Recent session
        let now = chrono::Utc::now().to_rfc3339();
        store.insert_event("sess-new", &test_event("evt-new", &now)).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-new".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some(now.clone()),
            last_event: Some(now),
        }).unwrap();

        let deleted = store.cleanup_old_sessions(30).unwrap();
        assert_eq!(deleted, 1, "should delete 1 old event");
        assert_eq!(store.list_sessions().unwrap().len(), 1);
        assert_eq!(store.list_sessions().unwrap()[0].id, "sess-new");
    }

    #[test]
    fn cleanup_with_no_old_sessions_deletes_nothing() {
        let store = SqliteStore::in_memory().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        store.insert_event("sess-recent", &test_event("evt-r", &now)).unwrap();
        store.upsert_session(&SessionRow {
            id: "sess-recent".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: Some(now.clone()),
            last_event: Some(now),
        }).unwrap();

        let deleted = store.cleanup_old_sessions(7).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(store.list_sessions().unwrap().len(), 1);
    }

    // ── Encryption key API (SQLCipher when available) ──────────────

    #[test]
    fn new_with_key_accepts_key_parameter() {
        // With bundled (non-cipher) SQLite, key is accepted but ignored.
        // With bundled-sqlcipher, the DB would actually be encrypted.
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), Some("test-key-123")).unwrap();

        store.insert_event("sess-enc", &test_event("evt-enc-1", "2025-01-14T00:00:00Z")).unwrap();
        let events = store.session_events("sess-enc").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "evt-enc-1");
    }

    #[test]
    fn new_with_key_none_is_same_as_new() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), None).unwrap();
        store.insert_event("sess-nk", &test_event("evt-nk", "2025-01-14T00:00:00Z")).unwrap();

        // Readable by new() (no key)
        let store2 = SqliteStore::new(tmp.path()).unwrap();
        assert_eq!(store2.session_events("sess-nk").unwrap().len(), 1);
    }

    #[test]
    fn new_with_empty_key_is_same_as_no_key() {
        let tmp = TempDir::new().unwrap();
        let store = SqliteStore::new_with_key(tmp.path(), Some("")).unwrap();
        store.insert_event("sess-ek", &test_event("evt-ek", "2025-01-14T00:00:00Z")).unwrap();

        let store2 = SqliteStore::new(tmp.path()).unwrap();
        let events = store2.session_events("sess-ek").unwrap();
        assert_eq!(events.len(), 1);
    }
}
