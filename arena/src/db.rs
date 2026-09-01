use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

use crate::manifest::EventManifest;

pub struct Db(Mutex<Connection>);

/// Typed failure signal for `create_user`, so callers (the HTTP layer) can
/// distinguish "username already taken" (409) from any other storage
/// failure (500) without string-matching an error message.
#[derive(Debug)]
pub enum DbError {
    /// A UNIQUE/PRIMARY KEY constraint was violated — the username exists.
    Duplicate,
    /// Any other storage failure.
    Other(anyhow::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Duplicate => write!(f, "username already exists"),
            DbError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DbError {}

fn classify_create_user_error(e: rusqlite::Error, username: &str) -> DbError {
    if let rusqlite::Error::SqliteFailure(ref sqlite_err, _) = e {
        if sqlite_err.code == ErrorCode::ConstraintViolation {
            return DbError::Duplicate;
        }
    }
    DbError::Other(anyhow!("create_user failed for {username:?}: {e}"))
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub username: String,
    pub event: String,
    pub pass_hash: String,
}

#[derive(Debug, Clone)]
pub struct SandboxRow {
    pub username: String,
    pub container_id: String,
    pub litellm_key: String,
    pub expires_at: DateTime<Utc>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events(
  name TEXT PRIMARY KEY,
  manifest_json TEXT NOT NULL,
  join_code TEXT UNIQUE,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS users(
  username TEXT PRIMARY KEY,
  event TEXT NOT NULL REFERENCES events(name),
  pass_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sandboxes(
  username TEXT PRIMARY KEY REFERENCES users(username),
  container_id TEXT NOT NULL,
  litellm_key TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
";

/// RFC3339 UTC with a Zulu suffix and second precision, so that plain string
/// comparison (used by `list_expired`'s `WHERE expires_at <= ?1`) matches
/// chronological order.
fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

fn row_to_sandbox(username: String, container_id: String, litellm_key: String, expires_at: String) -> Result<SandboxRow> {
    Ok(SandboxRow {
        username,
        container_id,
        litellm_key,
        expires_at: parse_ts(&expires_at)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Db> {
        // `serve` (reaper writing every 60s + register/launch writes) and
        // the separate `arena up/down/users` invocations (each `docker exec
        // arena-cp arena …` opens its own connection) all hit the same
        // /data/arena.db file concurrently. rusqlite's default busy_timeout
        // is 0, so any write overlap between those independent connections
        // is an immediate SQLITE_BUSY instead of a short wait. Set a real
        // timeout before anything else touches the connection.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // WAL lets readers and a writer proceed concurrently instead of
        // serializing on a single file lock, which is what actually makes
        // the busy_timeout above rare to hit in practice rather than just a
        // longer wait before the same failure. `PRAGMA journal_mode=WAL` is
        // not a plain setter — it returns a row naming the resulting mode —
        // so it must go through `pragma_update`, not `execute`. It's also
        // meaningless for `:memory:` connections (there's no file to
        // journal); SQLite itself reports that by leaving the mode as
        // "memory" rather than erroring, but tolerate an error here too so
        // `open_in_memory` can never fail because of this pragma.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");

        conn.execute_batch(SCHEMA)?;
        Ok(Db(Mutex::new(conn)))
    }

    pub fn insert_event(&self, m: &EventManifest) -> Result<()> {
        let conn = self.0.lock().unwrap();
        let manifest_json = serde_json::to_string(m)?;
        conn.execute(
            "INSERT INTO events (name, manifest_json, join_code, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![m.name, manifest_json, m.join_code, fmt_ts(Utc::now())],
        )?;
        Ok(())
    }

    pub fn get_event(&self, name: &str) -> Result<Option<EventManifest>> {
        let conn = self.0.lock().unwrap();
        let manifest_json: Option<String> = conn
            .query_row(
                "SELECT manifest_json FROM events WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        manifest_json
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }

    pub fn event_by_join_code(&self, code: &str) -> Result<Option<String>> {
        let conn = self.0.lock().unwrap();
        let name = conn
            .query_row(
                "SELECT name FROM events WHERE join_code = ?1",
                params![code],
                |row| row.get(0),
            )
            .optional()?;
        Ok(name)
    }

    pub fn create_user(&self, username: &str, event: &str, pass_hash: &str) -> Result<(), DbError> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, event, pass_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![username, event, pass_hash, fmt_ts(Utc::now())],
        )
        .map(|_| ())
        .map_err(|e| classify_create_user_error(e, username))
    }

    pub fn get_user(&self, username: &str) -> Result<Option<UserRow>> {
        let conn = self.0.lock().unwrap();
        conn.query_row(
            "SELECT username, event, pass_hash FROM users WHERE username = ?1",
            params![username],
            |row| {
                Ok(UserRow {
                    username: row.get(0)?,
                    event: row.get(1)?,
                    pass_hash: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_users(&self, event: &str) -> Result<Vec<UserRow>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, event, pass_hash FROM users WHERE event = ?1 ORDER BY username",
        )?;
        let rows = stmt
            .query_map(params![event], |row| {
                Ok(UserRow {
                    username: row.get(0)?,
                    event: row.get(1)?,
                    pass_hash: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_sandbox(&self, s: &SandboxRow) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute(
            "INSERT INTO sandboxes (username, container_id, litellm_key, expires_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(username) DO UPDATE SET
               container_id = excluded.container_id,
               litellm_key = excluded.litellm_key,
               expires_at = excluded.expires_at",
            params![s.username, s.container_id, s.litellm_key, fmt_ts(s.expires_at)],
        )?;
        Ok(())
    }

    pub fn get_sandbox(&self, username: &str) -> Result<Option<SandboxRow>> {
        let conn = self.0.lock().unwrap();
        let row: Option<(String, String, String, String)> = conn
            .query_row(
                "SELECT username, container_id, litellm_key, expires_at FROM sandboxes WHERE username = ?1",
                params![username],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        row.map(|(username, container_id, litellm_key, expires_at)| {
            row_to_sandbox(username, container_id, litellm_key, expires_at)
        })
        .transpose()
    }

    pub fn delete_sandbox(&self, username: &str) -> Result<()> {
        let conn = self.0.lock().unwrap();
        conn.execute("DELETE FROM sandboxes WHERE username = ?1", params![username])?;
        Ok(())
    }

    pub fn list_expired(&self, now: DateTime<Utc>) -> Result<Vec<SandboxRow>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, container_id, litellm_key, expires_at FROM sandboxes WHERE expires_at <= ?1",
        )?;
        let rows = stmt
            .query_map(params![fmt_ts(now)], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(username, container_id, litellm_key, expires_at)| {
                row_to_sandbox(username, container_id, litellm_key, expires_at)
            })
            .collect()
    }

    pub fn list_sandboxes_for_event(&self, event: &str) -> Result<Vec<SandboxRow>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.username, s.container_id, s.litellm_key, s.expires_at
             FROM sandboxes s JOIN users u ON u.username = s.username
             WHERE u.event = ?1
             ORDER BY s.username",
        )?;
        let rows = stmt
            .query_map(params![event], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(username, container_id, litellm_key, expires_at)| {
                row_to_sandbox(username, container_id, litellm_key, expires_at)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::EventManifest;
    use chrono::{Duration, Utc};

    fn event(name: &str, code: &str) -> EventManifest {
        EventManifest::from_toml(&format!(
            "name = \"{name}\"\nimage = \"img:1\"\njoin_code = \"{code}\"\nttl_hours = 6\nbudget_usd = 5.0"
        )).unwrap()
    }

    #[test]
    fn file_backed_db_sets_a_nonzero_busy_timeout() {
        // `Db::open` (the file-path constructor) had no direct test before
        // this — every other test exercises `open_in_memory`. Concurrent
        // writers (the reaper vs. an `arena up/down/users` invocation) rely
        // on a real busy_timeout, not rusqlite's default of 0.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arena.db");
        let db = Db::open(&path).unwrap();
        let conn = db.0.lock().unwrap();
        let busy_timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout_ms, 5000);
    }

    #[test]
    fn event_roundtrips_and_resolves_by_join_code() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("uva-fall", "uva-2026")).unwrap();
        let got = db.get_event("uva-fall").unwrap().unwrap();
        assert_eq!(got.image, "img:1");
        assert_eq!(db.event_by_join_code("uva-2026").unwrap().as_deref(), Some("uva-fall"));
        assert!(db.event_by_join_code("nope").unwrap().is_none());
    }

    #[test]
    fn duplicate_username_is_rejected_with_a_typed_duplicate_signal() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("e", "c")).unwrap();
        db.create_user("katie", "e", "h1").unwrap();
        assert!(
            matches!(db.create_user("katie", "e", "h2"), Err(DbError::Duplicate)),
            "a PRIMARY KEY collision on username must classify as DbError::Duplicate"
        );
        assert_eq!(db.get_user("katie").unwrap().unwrap().pass_hash, "h1");
    }

    #[test]
    fn sandbox_upsert_get_delete_and_expiry_listing() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("e", "c")).unwrap();
        db.create_user("katie", "e", "h").unwrap();
        db.create_user("bob", "e", "h").unwrap();
        let now = Utc::now();
        db.upsert_sandbox(&SandboxRow { username: "katie".into(), container_id: "c1".into(), litellm_key: "k1".into(), expires_at: now - Duration::hours(1) }).unwrap();
        db.upsert_sandbox(&SandboxRow { username: "bob".into(), container_id: "c2".into(), litellm_key: "k2".into(), expires_at: now + Duration::hours(1) }).unwrap();
        let expired = db.list_expired(now).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].username, "katie");
        db.delete_sandbox("katie").unwrap();
        assert!(db.get_sandbox("katie").unwrap().is_none());
        assert_eq!(db.list_sandboxes_for_event("e").unwrap().len(), 1);
    }
}
