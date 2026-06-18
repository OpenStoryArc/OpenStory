//! change-shape — the actual delta of every edit.
//!
//! Ported from `scripts/build_change_shapes.py`. For every Edit / Write /
//! MultiEdit, extract lines/chars added+removed and a 200-char excerpt of the
//! new text. One row per event (MultiEdit is summed into a single row).

use open_story_core::cloud_event::CloudEvent;
use serde_json::json;

use crate::{ShapeExtractor, ShapeRow};

pub const SHAPE_TYPE: &str = "change-shape";

const CHANGE_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit"];
const EXCERPT_LEN: usize = 200;

pub struct ChangeShape;

impl ShapeExtractor for ChangeShape {
    fn shape_type(&self) -> &str {
        SHAPE_TYPE
    }

    fn extract(&self, event: &CloudEvent, session_id: &str) -> Vec<ShapeRow> {
        let ap = match event.data.agent_payload.as_ref() {
            Some(ap) => ap,
            None => return vec![],
        };
        let tool = match ap.tool() {
            Some(t) if CHANGE_TOOLS.contains(&t) => t,
            _ => return vec![],
        };
        let args = match ap.args() {
            Some(a) => a,
            None => return vec![],
        };
        let path = args
            .get("file_path")
            .or_else(|| args.get("notebook_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if path.is_empty() {
            return vec![];
        }

        let (edit_count, added, removed, chars_a, chars_r, excerpt) = match tool {
            "Edit" => {
                let old = str_arg(args, "old_string");
                let new = str_arg(args, "new_string");
                (1, count_lines(new), count_lines(old), char_len(new), char_len(old), excerpt(new))
            }
            "Write" => {
                let content = str_arg(args, "content");
                (1, count_lines(content), 0, char_len(content), 0, excerpt(content))
            }
            "MultiEdit" => {
                let mut added = 0;
                let mut removed = 0;
                let mut chars_a = 0;
                let mut chars_r = 0;
                let mut first_excerpt = String::new();
                let edits = args.get("edits").and_then(|e| e.as_array());
                let edit_count = edits.map(|e| e.len()).unwrap_or(0);
                if let Some(edits) = edits {
                    for (i, e) in edits.iter().enumerate() {
                        let old = e.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                        let new = e.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                        removed += count_lines(old);
                        added += count_lines(new);
                        chars_r += char_len(old);
                        chars_a += char_len(new);
                        if i == 0 {
                            first_excerpt = excerpt(new);
                        }
                    }
                }
                (edit_count, added, removed, chars_a, chars_r, first_excerpt)
            }
            _ => return vec![],
        };

        let data = json!({
            "tool": tool,
            "path": path,
            "edit_count": edit_count,
            "lines_added": added,
            "lines_removed": removed,
            "chars_added": chars_a,
            "chars_removed": chars_r,
            "new_excerpt": excerpt,
        });
        vec![ShapeRow::new(event, session_id, SHAPE_TYPE, 0, data)]
    }
}

fn str_arg<'a>(args: &'a serde_json::Value, key: &str) -> &'a str {
    args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn excerpt(s: &str) -> String {
    s.chars().take(EXCERPT_LEN).collect()
}

/// Count lines: each `\n` separates a line; a non-empty string without a
/// trailing newline counts its final line. Mirrors the Python `count_lines()`.
pub fn count_lines(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut n = s.matches('\n').count();
    if !s.ends_with('\n') {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};

    // Fixtures lifted from `scripts/_shape_spec_fixtures.py` (the spec).
    #[test]
    fn count_lines_matches_python() {
        assert_eq!(count_lines("hello\nworld"), 2);
        assert_eq!(count_lines("a\nb\nc\n"), 3);
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("single line no newline"), 1);
    }

    fn tool_event(tool: &str, args: serde_json::Value) -> CloudEvent {
        let mut p = ClaudeCodePayload::new();
        p.tool = Some(tool.to_string());
        p.args = Some(args);
        let data = EventData::with_payload(json!({}), 5, "parent".to_string(),
                                           AgentPayload::ClaudeCode(p));
        CloudEvent::new("test".into(), "io.arc.event".into(), data,
                        Some("message.assistant.tool_use".into()), Some("evt-c".into()),
                        Some("2026-01-01T00:00:00Z".into()), None, None, Some("claude-code".into()))
    }

    #[test]
    fn edit_delta() {
        let ev = tool_event("Edit", json!({
            "file_path": "/x/a.rs", "old_string": "a\nb", "new_string": "a\nb\nc\nd"
        }));
        let rows = ChangeShape.extract(&ev, "own");
        assert_eq!(rows.len(), 1);
        let d = &rows[0].data;
        assert_eq!(d["tool"], "Edit");
        assert_eq!(d["lines_removed"], 2);
        assert_eq!(d["lines_added"], 4);
        assert_eq!(d["chars_removed"], 3);
        assert_eq!(d["edit_count"], 1);
        assert_eq!(rows[0].id, "evt-c:change-shape:0");
    }

    #[test]
    fn multiedit_sums_and_excerpts_first() {
        let ev = tool_event("MultiEdit", json!({
            "file_path": "/x/a.rs",
            "edits": [
                {"old_string": "x", "new_string": "first\nline"},
                {"old_string": "", "new_string": "y"}
            ]
        }));
        let d = &ChangeShape.extract(&ev, "own")[0].data;
        assert_eq!(d["edit_count"], 2);
        assert_eq!(d["lines_added"], 3); // 2 + 1
        assert_eq!(d["new_excerpt"], "first\nline");
    }

    #[test]
    fn write_counts_content_only() {
        let ev = tool_event("Write", json!({"file_path": "/x/n.md", "content": "one\ntwo\n"}));
        let d = &ChangeShape.extract(&ev, "own")[0].data;
        assert_eq!(d["lines_added"], 2);
        assert_eq!(d["lines_removed"], 0);
    }

    #[test]
    fn non_change_tool_and_missing_path_yield_nothing() {
        let read_ev = tool_event("Read", json!({"file_path": "/x"}));
        assert!(ChangeShape.extract(&read_ev, "s").is_empty());
        let no_path = tool_event("Write", json!({"content": "x"}));
        assert!(ChangeShape.extract(&no_path, "s").is_empty());
    }
}
