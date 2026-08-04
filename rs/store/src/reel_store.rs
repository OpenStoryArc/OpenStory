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
    pub fn save(&self, reel: &mut Reel) -> Result<String> {
        if reel.id.is_empty() {
            reel.id = format!("reel-{}", Uuid::new_v4());
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
        let text = fs::read_to_string(self.dir.join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn delete(&self, id: &str) -> bool {
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
}
