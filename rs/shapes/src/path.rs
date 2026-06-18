//! path-shape — the codebase attention footprint.
//!
//! Ported from `scripts/build_path_shapes.py`. For every file-touching tool
//! call, decompose each path into directory chain / basename / stem / extension
//! / top project segment / naming tokens (camel + snake + kebab split). One row
//! per matching path key, so multi-key events emit several rows (`ord` = key
//! index).

use open_story_core::cloud_event::CloudEvent;
use serde_json::json;

use crate::{ShapeExtractor, ShapeRow};

pub const SHAPE_TYPE: &str = "path-shape";

/// Tool input keys that carry a file path, in priority order.
const PATH_KEYS: &[&str] = &["file_path", "notebook_path", "path"];

/// Anything after this anchor is the project-relative view of the path.
const PROJECT_ANCHORS: &[&str] = &["/OpenStory/", "/openstory/"];

/// Naming tokens dropped as too generic to be signal.
const NAMING_STOPWORDS: &[&str] = &["src", "lib", "js", "ts", "tsx", "rs", "py", "md", "test", "tests"];

pub struct PathShape;

impl ShapeExtractor for PathShape {
    fn shape_type(&self) -> &str {
        SHAPE_TYPE
    }

    fn extract(&self, event: &CloudEvent, session_id: &str) -> Vec<ShapeRow> {
        let ap = match event.data.agent_payload.as_ref() {
            Some(ap) => ap,
            None => return vec![],
        };
        // Only tool-call events carry a tool + args.
        let tool = match ap.tool() {
            Some(t) => t,
            None => return vec![],
        };
        let args = match ap.args() {
            Some(a) => a,
            None => return vec![],
        };

        let mut rows = Vec::new();
        for key in PATH_KEYS {
            let val = match args.get(*key).and_then(|v| v.as_str()) {
                Some(v) if !v.trim().is_empty() => v.trim(),
                _ => continue,
            };
            // skip glob patterns and shell-meta paths
            if val.contains('*') || val.contains('?') {
                continue;
            }
            let mut d = decompose(val);
            d["tool"] = json!(tool);
            d["path"] = json!(val);
            rows.push(ShapeRow::new(event, session_id, SHAPE_TYPE, rows.len(), d));
        }
        rows
    }
}

/// Decompose a path into its structural components. Mirrors the Python
/// `decompose()` (PurePosixPath semantics replicated by hand).
pub fn decompose(path: &str) -> serde_json::Value {
    let is_abs = path.starts_with('/');
    let rel = normalize_for_top_segment(path);
    let parts = posix_parts(rel);

    let basename = parts.last().cloned().unwrap_or_default();
    let dir_segments: Vec<String> = if parts.is_empty() {
        vec![]
    } else {
        parts[..parts.len() - 1].to_vec()
    };
    let directory = dir_segments.join("/");
    let depth = parts.len().saturating_sub(1);
    let top_segment = dir_segments.first().cloned().unwrap_or_default();

    let extension = suffix_of(&basename).to_lowercase();
    // recover the inner stem for compound extensions like `.spec.ts`
    let mut stem = strip_suffix(&basename).to_string();
    loop {
        let suf = suffix_of(&stem);
        if stem.contains('.') && !suf.is_empty() {
            let new_len = stem.len() - suf.len();
            stem.truncate(new_len);
        } else {
            break;
        }
    }
    let naming_tokens = tokenize_stem(&stem);

    json!({
        "directory": directory,
        "basename": basename,
        "stem": stem,
        "extension": extension,
        "depth": depth,
        "top_segment": top_segment,
        "dir_segments": dir_segments,
        "naming_tokens": naming_tokens,
        "absolute": if is_abs { 1 } else { 0 },
    })
}

fn normalize_for_top_segment(path: &str) -> &str {
    for anchor in PROJECT_ANCHORS {
        if let Some(i) = path.find(anchor) {
            return &path[i + anchor.len()..];
        }
    }
    path
}

/// Replicate `PurePosixPath(rel).parts`: leading "/" is its own part for
/// absolute paths, then the non-empty components.
fn posix_parts(rel: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if rel.starts_with('/') {
        parts.push("/".to_string());
    }
    for seg in rel.split('/') {
        if !seg.is_empty() {
            parts.push(seg.to_string());
        }
    }
    parts
}

/// `PurePosixPath(name).suffix` — the final `.ext`, or "" if none / leading dot.
fn suffix_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i..],
        _ => "",
    }
}

/// `name` without its final suffix (`PurePosixPath(name).stem`-ish).
fn strip_suffix(name: &str) -> &str {
    let suf = suffix_of(name);
    if suf.is_empty() {
        name
    } else {
        &name[..name.len() - suf.len()]
    }
}

/// Split a file stem into naming tokens. Snake / kebab / dot, then camelCase.
pub fn tokenize_stem(stem: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for part in stem.split(['_', '-', '.']) {
        if part.is_empty() {
            continue;
        }
        for sub in camel_split(part) {
            let sub = sub.to_lowercase();
            let sub = sub.trim();
            if sub.is_empty() || sub.chars().all(|c| c.is_ascii_digit()) || sub.len() < 2 {
                continue;
            }
            if NAMING_STOPWORDS.contains(&sub) {
                continue;
            }
            tokens.push(sub.to_string());
        }
    }
    tokens
}

/// Split on camelCase boundaries. Hand-port of the Python regex
/// `(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])` (Rust regex has no
/// lookaround).
fn camel_split(part: &str) -> Vec<String> {
    let chars: Vec<char> = part.chars().collect();
    if chars.len() < 2 {
        return vec![part.to_string()];
    }
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];
        let boundary =
            // (?<=[a-z0-9])(?=[A-Z])
            ((prev.is_ascii_lowercase() || prev.is_ascii_digit()) && cur.is_ascii_uppercase())
            // (?<=[A-Z])(?=[A-Z][a-z])
            || (prev.is_ascii_uppercase()
                && cur.is_ascii_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()));
        if boundary {
            out.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    out.push(chars[start..].iter().collect());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};

    // Fixtures lifted from `scripts/_shape_spec_fixtures.py` (the spec).
    #[test]
    fn matches_python_decompose_fixtures() {
        let d = decompose("/Users/x/projects/OpenStory/rs/shapes/src/lib.rs");
        assert_eq!(d["directory"], "rs/shapes/src");
        assert_eq!(d["basename"], "lib.rs");
        assert_eq!(d["stem"], "lib");
        assert_eq!(d["extension"], ".rs");
        assert_eq!(d["depth"], 3);
        assert_eq!(d["top_segment"], "rs");
        assert_eq!(d["naming_tokens"], json!([])); // "lib" is a stopword
        assert_eq!(d["absolute"], 1);

        let d = decompose("rs/store/src/sqlite_store.rs");
        assert_eq!(d["naming_tokens"], json!(["sqlite", "store"]));
        assert_eq!(d["absolute"], 0);

        // compound extension: stem strips back through `.spec`
        let d = decompose("ui/tests/streams/event-transforms.spec.ts");
        assert_eq!(d["stem"], "event-transforms");
        assert_eq!(d["extension"], ".ts");
        assert_eq!(d["naming_tokens"], json!(["event", "transforms"]));

        let d = decompose("scripts/build_bash_shapes.py");
        assert_eq!(d["naming_tokens"], json!(["build", "bash", "shapes"]));

        let d = decompose("Cargo.toml");
        assert_eq!(d["directory"], "");
        assert_eq!(d["depth"], 0);
        assert_eq!(d["top_segment"], "");
        assert_eq!(d["stem"], "Cargo");
        assert_eq!(d["naming_tokens"], json!(["cargo"]));
    }

    #[test]
    fn camel_split_handles_acronym_boundaries() {
        assert_eq!(camel_split("sqliteStore"), vec!["sqlite", "Store"]);
        assert_eq!(camel_split("HTTPServer"), vec!["HTTP", "Server"]);
        assert_eq!(camel_split("plain"), vec!["plain"]);
    }

    fn tool_event(tool: &str, args: serde_json::Value) -> CloudEvent {
        let mut p = ClaudeCodePayload::new();
        p.tool = Some(tool.to_string());
        p.args = Some(args);
        let data = EventData::with_payload(json!({}), 3, "parent".to_string(),
                                           AgentPayload::ClaudeCode(p));
        CloudEvent::new("test".into(), "io.arc.event".into(), data,
                        Some("message.assistant.tool_use".into()), Some("evt-9".into()),
                        Some("2026-01-01T00:00:00Z".into()), None, None, Some("claude-code".into()))
    }

    #[test]
    fn extract_emits_one_row_per_path_key_and_skips_globs() {
        let ev = tool_event("Read", json!({"file_path": "rs/store/src/sqlite_store.rs"}));
        let rows = PathShape.extract(&ev, "own");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "own");
        assert_eq!(rows[0].id, "evt-9:path-shape:0");
        assert_eq!(rows[0].data["tool"], "Read");
        assert_eq!(rows[0].data["top_segment"], "rs");

        // glob patterns are skipped
        let ev = tool_event("Glob", json!({"path": "rs/**/*.rs"}));
        assert!(PathShape.extract(&ev, "own").is_empty());
    }
}
