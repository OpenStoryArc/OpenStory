//! Session citizenship — Live (disk + watcher) vs Explore (store).
//!
//! Pure classification mirrors `scripts/session_citizenship.py`. The API
//! surface is `GET /api/sessions/{id}/citizenship`. See
//! `docs/research/session-citizenship-ghosts.md`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

/// Verdict: is this session a full member of the durable mirror?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Disk + store agree (events present).
    Citizen,
    /// Live stream on disk; Explore atom empty (sovereignty failure mode).
    Ghost,
    /// Store has rows; no disk transcript under known roots.
    OrphanStore,
    /// Neither disk nor store know this id.
    Absent,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Citizen => "citizen",
            Verdict::Ghost => "ghost",
            Verdict::OrphanStore => "orphan-store",
            Verdict::Absent => "absent",
        }
    }
}

/// Node-level **ghost risk**: watchers have emitted CloudEvents but the store
/// has no sessions — the classic "Live green, Explore empty" failure mode after
/// a slow-consumer drop during backfill.
///
/// Pure. Used by `GET /api/health` so operators/agents see sovereignty risk
/// without grepping NATS logs.
pub fn ghost_risk(watcher_cloud_events_emitted: u64, store_sessions: usize) -> bool {
    watcher_cloud_events_emitted > 0 && store_sessions == 0
}

/// Pure classify — same rules as the Python script.
///
/// - citizen: on disk AND in store with event_count > 0
/// - ghost: on disk AND not in store
/// - orphan-store: not on disk AND in store
/// - absent: neither
pub fn classify(on_disk: bool, in_store: bool, store_event_count: u64) -> Verdict {
    if on_disk && in_store && store_event_count > 0 {
        Verdict::Citizen
    } else if on_disk && !in_store {
        Verdict::Ghost
    } else if !on_disk && in_store {
        Verdict::OrphanStore
    } else {
        // on_disk && in_store but zero events → treat as absent/unknown edge;
        // in_store is false when count is 0 and no session row, so this is
        // typically neither-on-disk-nor-store.
        Verdict::Absent
    }
}

/// Grok layout: `~/.grok/sessions/<project-encoded>/<session-id>/updates.jsonl`
pub fn find_disk_session(session_id: &str, grok_root: &Path) -> Option<PathBuf> {
    if !grok_root.is_dir() {
        return None;
    }
    let mut stack = vec![grok_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some(session_id)
                    && path.join("updates.jsonl").is_file()
                {
                    return Some(path);
                }
                // Bound depth: project → session (2 levels under root is enough)
                // but walk a few more for nested layouts.
                if path.starts_with(grok_root) {
                    stack.push(path);
                }
            }
        }
    }
    None
}

pub fn disk_updates_stats(session_dir: &Path) -> (u64, u64) {
    let updates = session_dir.join("updates.jsonl");
    if !updates.is_file() {
        return (0, 0);
    }
    let bytes = updates.metadata().map(|m| m.len()).unwrap_or(0);
    let lines = std::fs::read_to_string(&updates)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as u64)
        .unwrap_or(0);
    (bytes, lines)
}

#[derive(Debug, Clone, Serialize)]
pub struct CitizenshipReport {
    pub session_id: String,
    pub verdict: Verdict,
    pub on_disk: bool,
    pub disk_path: Option<String>,
    pub updates_bytes: u64,
    pub updates_lines: u64,
    pub in_store: bool,
    pub store_event_count: u64,
    pub store_last_event: Option<String>,
    pub watcher_last_path: Option<String>,
    pub watcher_last_event_at: Option<String>,
    pub watcher_publish_failures: u64,
    pub watcher_cloud_events_emitted: u64,
    pub notes: Vec<String>,
}

impl CitizenshipReport {
    pub fn to_json(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "verdict": self.verdict.as_str(),
            "disk": {
                "on_disk": self.on_disk,
                "path": self.disk_path,
                "updates_bytes": self.updates_bytes,
                "updates_lines": self.updates_lines,
            },
            "store": {
                "in_store": self.in_store,
                "event_count": self.store_event_count,
                "last_event": self.store_last_event,
            },
            "watcher": {
                "last_path": self.watcher_last_path,
                "last_event_at": self.watcher_last_event_at,
                "publish_failures": self.watcher_publish_failures,
                "cloud_events_emitted": self.watcher_cloud_events_emitted,
            },
            "notes": self.notes,
        })
    }

    pub fn with_notes(mut self) -> Self {
        self.notes = match self.verdict {
            Verdict::Citizen => vec![
                "Disk and Explore atom agree — sovereignty path is intact.".into(),
            ],
            Verdict::Ghost => {
                let mut n = vec![
                    "Live stream exists on disk but Explore atom is empty. \
                     Often: NATS slow-consumer killed persist subscription during backfill; \
                     watcher still publishes (green diagnostics) while store stays silent."
                        .into(),
                ];
                if self
                    .watcher_last_path
                    .as_ref()
                    .is_some_and(|p| p.contains(&self.session_id))
                {
                    n.push(
                        "Watcher last_processed_path matches this session — publish path is live."
                            .into(),
                    );
                }
                if self.watcher_publish_failures > 0 {
                    n.push(format!(
                        "Watcher recorded {} publish_failures.",
                        self.watcher_publish_failures
                    ));
                }
                n
            }
            Verdict::OrphanStore => {
                vec!["Store has rows but no disk updates.jsonl under grok root.".into()]
            }
            Verdict::Absent => {
                vec!["Neither disk nor store know this session id.".into()]
            }
        };
        self
    }
}

/// Build a report from pre-gathered facts (pure).
pub fn build_report(
    session_id: &str,
    on_disk: bool,
    disk_path: Option<String>,
    updates_bytes: u64,
    updates_lines: u64,
    store_event_count: u64,
    store_last_event: Option<String>,
    watcher_last_path: Option<String>,
    watcher_last_event_at: Option<String>,
    watcher_publish_failures: u64,
    watcher_cloud_events_emitted: u64,
) -> CitizenshipReport {
    let in_store = store_event_count > 0 || store_last_event.is_some();
    let verdict = classify(on_disk, in_store, store_event_count);
    CitizenshipReport {
        session_id: session_id.to_string(),
        verdict,
        on_disk,
        disk_path,
        updates_bytes,
        updates_lines,
        in_store,
        store_event_count,
        store_last_event,
        watcher_last_path,
        watcher_last_event_at,
        watcher_publish_failures,
        watcher_cloud_events_emitted,
        notes: vec![],
    }
    .with_notes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn when_watchers_emit_but_store_is_empty_it_should_flag_ghost_risk() {
        assert!(ghost_risk(100, 0));
        assert!(!ghost_risk(0, 0));
        assert!(!ghost_risk(100, 3));
        assert!(!ghost_risk(0, 3));
    }

    #[test]
    fn when_disk_and_store_agree_it_should_be_citizen() {
        assert_eq!(classify(true, true, 10), Verdict::Citizen);
    }

    #[test]
    fn when_disk_only_it_should_be_ghost() {
        assert_eq!(classify(true, false, 0), Verdict::Ghost);
    }

    #[test]
    fn when_store_only_it_should_be_orphan_store() {
        assert_eq!(classify(false, true, 5), Verdict::OrphanStore);
    }

    #[test]
    fn when_neither_it_should_be_absent() {
        assert_eq!(classify(false, false, 0), Verdict::Absent);
    }

    #[test]
    fn when_disk_has_session_dir_it_should_find_updates() {
        let tmp = TempDir::new().unwrap();
        let sid = "019f71cd-ghost-session";
        let sess = tmp
            .path()
            .join("%2FUsers%2Fme%2Fproj")
            .join(sid);
        fs::create_dir_all(&sess).unwrap();
        fs::write(sess.join("updates.jsonl"), "{\"method\":\"session/update\"}\n").unwrap();

        let found = find_disk_session(sid, tmp.path()).expect("disk session");
        assert_eq!(found, sess);
        let (bytes, lines) = disk_updates_stats(&found);
        assert!(bytes > 0);
        assert_eq!(lines, 1);
    }

    #[test]
    fn when_ghost_report_it_should_note_sovereignty_failure() {
        let r = build_report(
            "sid-1",
            true,
            Some("/tmp/sid-1".into()),
            100,
            3,
            0,
            None,
            Some("/tmp/sid-1/updates.jsonl".into()),
            None,
            0,
            42,
        );
        assert_eq!(r.verdict, Verdict::Ghost);
        assert!(r.notes.iter().any(|n| n.contains("Explore atom is empty")));
        let j = r.to_json();
        assert_eq!(j["verdict"], "ghost");
        assert_eq!(j["disk"]["on_disk"], true);
        assert_eq!(j["store"]["in_store"], false);
    }
}
