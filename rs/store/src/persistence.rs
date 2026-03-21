//! JSONL append-only storage — one file per session, survives restarts.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use serde_json::Value;

/// Persist CloudEvents to JSONL files in a data directory.
pub struct SessionStore {
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)?;
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn path(&self, session_id: &str) -> PathBuf {
        let safe: String = session_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.data_dir.join(format!("{safe}.jsonl"))
    }

    /// Append a single CloudEvent to the session's JSONL file.
    pub fn append(&self, session_id: &str, event: &Value) -> Result<()> {
        let path = self.path(session_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let line = serde_json::to_string(event)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Return all session IDs (from JSONL filenames).
    pub fn list_sessions(&self) -> Vec<String> {
        let mut sessions = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        sessions.push(stem.to_string());
                    }
                }
            }
        }
        sessions.sort();
        sessions
    }

    /// Return session IDs for files modified within `cutoff` duration from now.
    ///
    /// Uses file mtime to avoid reading file contents. Sessions older than `cutoff`
    /// are skipped entirely — they never enter memory.
    pub fn list_recent_sessions(&self, cutoff: Duration) -> Vec<String> {
        let now = SystemTime::now();
        let mut sessions = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                // Check mtime
                let dominated = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|mtime| {
                        now.duration_since(mtime)
                            .unwrap_or(Duration::ZERO)
                            <= cutoff
                    })
                    .unwrap_or(false);
                if !dominated {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    sessions.push(stem.to_string());
                }
            }
        }
        sessions.sort();
        sessions
    }

    /// Read all events for a session.
    pub fn load_session(&self, session_id: &str) -> Vec<Value> {
        let path = self.path(session_id);
        if !path.exists() {
            return vec![];
        }
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return vec![],
        };
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    events.push(val);
                }
            }
        }
        events
    }
}

/// Unified event log — all events in one append-only JSONL file.
///
/// Every CloudEvent that passes through `ingest_events` is appended here,
/// regardless of session. This provides a single chronological stream of
/// all activity the listener observes.
pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)?;
        Ok(Self {
            path: data_dir.join("events.jsonl"),
        })
    }

    /// Append a single CloudEvent to the unified log.
    pub fn append(&self, event: &Value) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Path to the log file (for external tools like the viewer).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_append_and_load() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        let event = json!({"type": "io.arc.session.start", "data": {}});
        store.append("sess-1", &event).unwrap();
        store.append("sess-1", &json!({"type": "io.arc.prompt.submit"})).unwrap();

        let loaded = store.load_session("sess-1");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["type"], "io.arc.session.start");
    }

    #[test]
    fn test_list_sessions() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store.append("alpha", &json!({"type": "test"})).unwrap();
        store.append("beta", &json!({"type": "test"})).unwrap();

        let sessions = store.list_sessions();
        assert_eq!(sessions, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_load_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();
        assert!(store.load_session("nope").is_empty());
    }

    #[test]
    fn test_sanitizes_session_id() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();
        store.append("bad/id:here", &json!({"type": "test"})).unwrap();
        let path = store.path("bad/id:here");
        assert!(path.file_name().unwrap().to_str().unwrap().contains("bad_id_here"));
    }

    #[test]
    fn test_list_recent_sessions_returns_recently_modified() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();
        store.append("recent-1", &json!({"type": "test"})).unwrap();
        store.append("recent-2", &json!({"type": "test"})).unwrap();

        let recent = store.list_recent_sessions(Duration::from_secs(3600));
        assert_eq!(recent, vec!["recent-1", "recent-2"]);
    }

    #[test]
    fn test_list_recent_sessions_excludes_old_files() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();
        store.append("old-session", &json!({"type": "test"})).unwrap();

        // Set mtime to 3 days ago
        let old_path = store.path("old-session");
        let three_days_ago = filetime::FileTime::from_system_time(
            SystemTime::now() - Duration::from_secs(3 * 24 * 3600),
        );
        filetime::set_file_mtime(&old_path, three_days_ago).unwrap();

        store.append("new-session", &json!({"type": "test"})).unwrap();

        // 24-hour window should only include new-session
        let recent = store.list_recent_sessions(Duration::from_secs(24 * 3600));
        assert_eq!(recent, vec!["new-session"]);
    }

    #[test]
    fn test_list_recent_sessions_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();
        let recent = store.list_recent_sessions(Duration::from_secs(3600));
        assert!(recent.is_empty());
    }

    // ── EventLog tests ──

    #[test]
    fn test_event_log_appends_to_single_file() {
        let tmp = TempDir::new().unwrap();
        let log = EventLog::new(tmp.path()).unwrap();

        log.append(&json!({"id": "e1", "type": "io.arc.event", "subtype": "message.user.prompt"})).unwrap();
        log.append(&json!({"id": "e2", "type": "io.arc.event", "subtype": "message.assistant.text"})).unwrap();

        let content = fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["id"], "e1");
        let second: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["id"], "e2");
    }

    #[test]
    fn test_event_log_creates_file_on_first_append() {
        let tmp = TempDir::new().unwrap();
        let log = EventLog::new(tmp.path()).unwrap();

        assert!(!log.path().exists());
        log.append(&json!({"id": "e1"})).unwrap();
        assert!(log.path().exists());
    }

    #[test]
    fn test_event_log_path_is_events_jsonl() {
        let tmp = TempDir::new().unwrap();
        let log = EventLog::new(tmp.path()).unwrap();
        assert_eq!(log.path().file_name().unwrap().to_str().unwrap(), "events.jsonl");
    }
}
