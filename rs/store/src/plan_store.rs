//! PlanStore — persist plans extracted from session events.
//!
//! Plans stored as .md files with hand-written frontmatter.
//! Port of Python plan_store.py.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PlanMeta {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub timestamp: String,
    pub content: String,
}

fn slugify(text: &str, max_len: usize) -> String {
    let slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    // Collapse runs of dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    if result.len() > max_len {
        result.truncate(max_len);
        result = result.trim_end_matches('-').to_string();
    }
    if result.is_empty() {
        "untitled".to_string()
    } else {
        result
    }
}

fn extract_title(content: &str) -> String {
    let re = Regex::new(r"(?m)^#\s+(?:Plan:\s*)?(.+)").unwrap();
    if let Some(cap) = re.captures(content) {
        cap[1].trim().to_string()
    } else {
        "Untitled Plan".to_string()
    }
}

fn write_frontmatter(session_id: &str, timestamp: &str, title: &str, content: &str) -> String {
    format!("---\nsession_id: {session_id}\ntimestamp: {timestamp}\ntitle: {title}\n---\n{content}")
}

/// Deterministic plan id from event identity: `{timestamp}-{session}-{title}`.
///
/// Keyed on the event's own identity rather than wall-clock time, so re-ingesting
/// the same plan resolves to the same filename and overwrites in place. The
/// timestamp slug leads so ids stay lexically time-sortable (ISO 8601 sorts that
/// way). `(session, timestamp)` is the dedup key — matching the event-store
/// `plan:{session}:{timestamp}` scheme — with the title slug appended only for
/// human-readable filenames.
fn plan_id(session_id: &str, timestamp: &str, title: &str) -> String {
    let ts = slugify(timestamp, 24);
    let sess = slugify(session_id, 48);
    let title_slug = slugify(title, 40);
    format!("{ts}-{sess}-{title_slug}")
}

fn parse_frontmatter(text: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut meta = std::collections::HashMap::new();
    if !text.starts_with("---\n") {
        return (meta, text.to_string());
    }
    if let Some(end_idx) = text[4..].find("\n---\n") {
        let header = &text[4..4 + end_idx];
        let body = &text[4 + end_idx + 5..];
        for line in header.lines() {
            if let Some(pos) = line.find(": ") {
                let key = line[..pos].trim().to_string();
                let val = line[pos + 2..].trim().to_string();
                meta.insert(key, val);
            }
        }
        (meta, body.to_string())
    } else {
        (meta, text.to_string())
    }
}

/// Persist plans as markdown files with frontmatter.
///
/// Cheap to `Clone` — it holds only the plans directory path; all state lives
/// on disk. Consumers that need their own handle (e.g. the persist actor) take
/// a clone rather than sharing a lock.
#[derive(Clone)]
pub struct PlanStore {
    dir: PathBuf,
}

impl PlanStore {
    pub fn new(plans_dir: &Path) -> Result<Self> {
        fs::create_dir_all(plans_dir)?;
        Ok(Self {
            dir: plans_dir.to_path_buf(),
        })
    }

    /// Save a plan and return its ID.
    ///
    /// The id is derived deterministically from the plan's event identity
    /// (timestamp + session + title), not wall-clock time, so re-ingesting the
    /// same plan overwrites in place rather than appending a duplicate. The
    /// timestamp slug leads so ids remain lexically time-sortable (ISO 8601).
    pub fn save(&self, session_id: &str, content: &str, timestamp: &str) -> Result<String> {
        let title = extract_title(content);
        let plan_id = plan_id(session_id, timestamp, &title);
        let file_text = write_frontmatter(session_id, timestamp, &title, content);
        fs::write(self.dir.join(format!("{plan_id}.md")), &file_text)?;
        Ok(plan_id)
    }

    /// All plans, sorted by timestamp desc.
    pub fn list_plans(&self) -> Vec<PlanMeta> {
        let mut plans = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Ok(text) = fs::read_to_string(&path) {
                        let (meta, _) = parse_frontmatter(&text);
                        let id = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        plans.push(PlanMeta {
                            id,
                            session_id: meta.get("session_id").cloned().unwrap_or_default(),
                            title: meta
                                .get("title")
                                .cloned()
                                .unwrap_or_else(|| "Untitled Plan".to_string()),
                            timestamp: meta.get("timestamp").cloned().unwrap_or_default(),
                        });
                    }
                }
            }
        }
        plans.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        plans
    }

    /// Plans for a specific session.
    pub fn list_for_session(&self, session_id: &str) -> Vec<PlanMeta> {
        self.list_plans()
            .into_iter()
            .filter(|p| p.session_id == session_id)
            .collect()
    }

    /// Load a plan by ID. Returns None if not found.
    pub fn load(&self, plan_id: &str) -> Option<Plan> {
        let path = self.dir.join(format!("{plan_id}.md"));
        if !path.is_file() {
            return None;
        }
        let text = fs::read_to_string(&path).ok()?;
        let (meta, content) = parse_frontmatter(&text);
        Some(Plan {
            id: plan_id.to_string(),
            session_id: meta.get("session_id").cloned().unwrap_or_default(),
            title: meta
                .get("title")
                .cloned()
                .unwrap_or_else(|| "Untitled Plan".to_string()),
            timestamp: meta.get("timestamp").cloned().unwrap_or_default(),
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World!", 40), "hello-world");
        assert_eq!(slugify("Add Axum Server", 40), "add-axum-server");
        assert_eq!(slugify("", 40), "untitled");
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(extract_title("# Plan: My Plan\nContent"), "My Plan");
        assert_eq!(extract_title("# Simple Title\nBody"), "Simple Title");
        assert_eq!(extract_title("No heading here"), "Untitled Plan");
    }

    #[test]
    fn test_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        let content = "# Plan: Test Plan\n\nSome content here.";
        let plan_id = store
            .save("sess-1", content, "2026-01-01T00:00:00Z")
            .unwrap();

        let plan = store.load(&plan_id).unwrap();
        assert_eq!(plan.session_id, "sess-1");
        assert_eq!(plan.title, "Test Plan");
        assert!(plan.content.contains("Some content here."));
    }

    #[test]
    fn test_list_plans() {
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        store
            .save("sess-1", "# Plan A", "2026-01-01T00:00:00Z")
            .unwrap();
        store
            .save("sess-2", "# Plan B", "2026-01-02T00:00:00Z")
            .unwrap();

        let plans = store.list_plans();
        assert_eq!(plans.len(), 2);
        // Sorted by timestamp desc
        assert_eq!(plans[0].title, "Plan B");
    }

    #[test]
    fn test_list_for_session() {
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        store
            .save("sess-1", "# Plan A", "2026-01-01T00:00:00Z")
            .unwrap();
        store
            .save("sess-2", "# Plan B", "2026-01-02T00:00:00Z")
            .unwrap();
        store
            .save("sess-1", "# Plan C", "2026-01-03T00:00:00Z")
            .unwrap();

        let plans = store.list_for_session("sess-1");
        assert_eq!(plans.len(), 2);
    }

    #[test]
    fn test_save_is_idempotent_for_same_plan() {
        // Re-ingesting the same plan (same session, timestamp, content) must not
        // pile up duplicate files. Keyed on the event identity, not wall-clock.
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        let content = "# Plan: Idempotent Plan\n\nBody.";
        let id1 = store.save("sess-1", content, "2026-01-01T00:00:00Z").unwrap();
        let id2 = store.save("sess-1", content, "2026-01-01T00:00:00Z").unwrap();

        assert_eq!(id1, id2, "same plan must map to the same id");
        assert_eq!(store.list_plans().len(), 1, "re-saving must overwrite, not append");
    }

    #[test]
    fn test_save_distinct_plans_do_not_collide() {
        // Different events (distinct timestamps) stay distinct, even same session/title.
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        store.save("sess-1", "# Plan: Same Title", "2026-01-01T00:00:00Z").unwrap();
        store.save("sess-1", "# Plan: Same Title", "2026-01-02T00:00:00Z").unwrap();

        assert_eq!(store.list_plans().len(), 2);
    }

    #[test]
    fn test_save_same_timestamp_different_session_no_collision() {
        // Two sessions emitting a plan at the same instant must not clobber each other.
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();

        store.save("sess-a", "# Plan: Untitled", "2026-01-01T00:00:00Z").unwrap();
        store.save("sess-b", "# Plan: Untitled", "2026-01-01T00:00:00Z").unwrap();

        assert_eq!(store.list_plans().len(), 2);
    }

    #[test]
    fn test_load_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let store = PlanStore::new(tmp.path()).unwrap();
        assert!(store.load("nope").is_none());
    }

    #[test]
    fn test_parse_frontmatter() {
        let text = "---\nsession_id: abc\ntitle: My Plan\n---\nBody here";
        let (meta, body) = parse_frontmatter(text);
        assert_eq!(meta.get("session_id").unwrap(), "abc");
        assert_eq!(meta.get("title").unwrap(), "My Plan");
        assert_eq!(body, "Body here");
    }
}
