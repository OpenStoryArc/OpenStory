//! ReelStore — persist reels: saved, replayable story sequences.
//!
//! A reel is curation ABOUT history (ordered pointers into the immutable
//! record + narration text). JSON files in `{data_dir}/reels/` — the wire
//! format IS the file format (camelCase), so the files are portable and
//! useful without the tool. Mirrors PlanStore: Clone-cheap, all state on disk.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReelStop {
    pub session_id: String,
    pub event_id: String,
    pub line: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reel {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub author: String,
    /// BLUF title card shown (and narrated) before stop 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opener: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closer: Option<String>,
    pub stops: Vec<ReelStop>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReelMeta {
    pub id: String,
    pub title: String,
    pub created: String,
    pub author: String,
    pub stop_count: usize,
}

/// A client-supplied (non-empty) reel id is interpolated straight into a
/// filesystem path (`{dir}/{id}.json`). Without validation, `"../../x"`
/// escapes the reels dir and an absolute path replaces the base entirely —
/// path traversal on save (write) and load/delete (read/unlink). Restrict
/// non-empty ids to the shape we ourselves generate: `reel-` followed by
/// hex digits and hyphens (a UUID), nothing else — no `/`, no `.`, no `\`.
fn valid_reel_id(id: &str) -> bool {
    match id.strip_prefix("reel-") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        None => false,
    }
}

#[derive(Clone)]
pub struct ReelStore {
    dir: PathBuf,
}

impl ReelStore {
    pub fn new(reels_dir: &Path) -> Result<Self> {
        fs::create_dir_all(reels_dir)?;
        Ok(Self { dir: reels_dir.to_path_buf() })
    }

    /// Save a reel; assigns a `reel-<uuid>` id when empty. Same id = overwrite.
    /// A non-empty, client-supplied id must match `valid_reel_id` — anything
    /// else (path traversal, absolute paths) is rejected before it ever
    /// reaches the filesystem.
    pub fn save(&self, reel: &mut Reel) -> Result<String> {
        if reel.id.is_empty() {
            reel.id = format!("reel-{}", Uuid::new_v4());
        } else if !valid_reel_id(&reel.id) {
            anyhow::bail!("invalid reel id: {}", reel.id);
        }
        let text = serde_json::to_string_pretty(reel)?;
        fs::write(self.dir.join(format!("{}.json", reel.id)), text)?;
        Ok(reel.id.clone())
    }

    /// All reels, newest `created` first.
    pub fn list(&self) -> Vec<ReelMeta> {
        let mut metas = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(&path) {
                    if let Ok(reel) = serde_json::from_str::<Reel>(&text) {
                        metas.push(ReelMeta {
                            id: reel.id,
                            title: reel.title,
                            created: reel.created,
                            author: reel.author,
                            stop_count: reel.stops.len(),
                        });
                    }
                }
            }
        }
        metas.sort_by(|a, b| b.created.cmp(&a.created));
        metas
    }

    pub fn load(&self, id: &str) -> Option<Reel> {
        if !valid_reel_id(id) {
            return None;
        }
        let text = fs::read_to_string(self.dir.join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn delete(&self, id: &str) -> bool {
        if !valid_reel_id(id) {
            return false;
        }
        fs::remove_file(self.dir.join(format!("{id}.json"))).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample(id: &str) -> Reel {
        Reel {
            id: id.to_string(),
            title: "The Launch pitch".to_string(),
            created: "2026-08-04T18:00:00Z".to_string(),
            author: "test-issuer".to_string(),
            opener: Some("The point up front.".to_string()),
            closer: Some("Three weeks. One pitch.".to_string()),
            stops: vec![ReelStop {
                session_id: "sess-1".to_string(),
                event_id: "evt-1".to_string(),
                line: "It starts with one typed line.".to_string(),
                clip_at: None,
            }],
        }
    }

    #[test]
    fn save_assigns_reel_prefixed_id_when_empty() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut reel = sample("");
        let id = store.save(&mut reel).unwrap();
        assert!(id.starts_with("reel-"), "got {id}");
        assert_eq!(reel.id, id);
    }

    #[test]
    fn save_then_load_round_trips_camel_case() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut reel = sample("");
        let id = store.save(&mut reel).unwrap();
        // On-disk JSON is camelCase (wire format = file format).
        let raw = std::fs::read_to_string(tmp.path().join(format!("{id}.json"))).unwrap();
        assert!(raw.contains("\"sessionId\""), "camelCase on disk: {raw}");
        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.title, "The Launch pitch");
        assert_eq!(loaded.stops.len(), 1);
        assert_eq!(loaded.stops[0].event_id, "evt-1");
        assert_eq!(loaded.opener.as_deref(), Some("The point up front."));
        assert_eq!(loaded.closer.as_deref(), Some("Three weeks. One pitch."));
    }

    #[test]
    fn save_with_existing_id_overwrites_in_place() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut reel = sample("");
        let id = store.save(&mut reel).unwrap();
        reel.title = "Retitled".to_string();
        let id2 = store.save(&mut reel).unwrap();
        assert_eq!(id, id2);
        assert_eq!(store.list().len(), 1, "re-save must overwrite, not append");
        assert_eq!(store.load(&id).unwrap().title, "Retitled");
    }

    #[test]
    fn list_sorts_by_created_desc_and_counts_stops() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut a = sample("");
        a.created = "2026-08-01T00:00:00Z".to_string();
        let mut b = sample("");
        b.created = "2026-08-03T00:00:00Z".to_string();
        b.title = "Newer".to_string();
        store.save(&mut a).unwrap();
        store.save(&mut b).unwrap();
        let metas = store.list();
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].title, "Newer");
        assert_eq!(metas[0].stop_count, 1);
    }

    #[test]
    fn delete_removes_and_reports() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut reel = sample("");
        let id = store.save(&mut reel).unwrap();
        assert!(store.delete(&id));
        assert!(store.load(&id).is_none());
        assert!(!store.delete(&id), "second delete is a no-op false");
    }

    #[test]
    fn load_nonexistent_and_ignores_non_json_files() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "not a reel").unwrap();
        assert!(store.load("nope").is_none());
        assert!(store.list().is_empty());
    }

    // ── Path traversal via client-supplied id (FINDING 1) ──────────────

    #[test]
    fn save_rejects_path_traversal_id_and_writes_no_file_outside_dir() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let mut reel = sample("../escape");
        let result = store.save(&mut reel);
        assert!(result.is_err(), "path-traversal id must be rejected, got {result:?}");

        // The dir passed to ReelStore::new is itself inside `tmp`, so
        // "../escape" would resolve to a sibling of the reels dir — list
        // the tempdir's parent to prove nothing landed there.
        let parent = tmp.path().parent().expect("tempdir has a parent");
        let escaped = parent.join("escape.json");
        assert!(!escaped.exists(), "must not write outside the reels dir: {escaped:?}");
    }

    #[test]
    fn save_rejects_absolute_path_id() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        let evil = tmp.path().parent().unwrap().join("absolute-escape.json");
        let mut reel = sample(evil.to_str().unwrap());
        let result = store.save(&mut reel);
        assert!(result.is_err(), "absolute-path id must be rejected, got {result:?}");
        assert!(!evil.exists(), "must not write to an absolute path: {evil:?}");
    }

    #[test]
    fn load_rejects_path_traversal_id() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        assert!(store.load("../x").is_none());
    }

    #[test]
    fn delete_rejects_path_traversal_id() {
        let tmp = TempDir::new().unwrap();
        let store = ReelStore::new(tmp.path()).unwrap();
        assert!(!store.delete("../x"));
    }
}
