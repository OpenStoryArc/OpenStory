//! Pure-function reader for Claude Code transcript JSONL files.
//!
//! Port of Python transcript.py. Reads the full session transcript and extracts
//! a timeline of conversation entries.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptEntry {
    pub timestamp: Option<String>,
    pub role: String,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Read a Claude Code transcript JSONL and return a timeline of entries.
pub fn read_transcript(path: &Path) -> Vec<TranscriptEntry> {
    if !path.exists() {
        return vec![];
    }

    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let obj: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let obj_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if obj_type != "user" && obj_type != "assistant" {
            continue;
        }

        let timestamp = obj.get("timestamp").and_then(|v| v.as_str()).map(|s| s.to_string());
        let message = obj.get("message").cloned().unwrap_or(Value::Object(Default::default()));
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or(obj_type)
            .to_string();

        let model = if obj_type == "assistant" {
            message.get("model").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        };

        let content = message.get("content").cloned().unwrap_or(Value::Array(vec![]));

        // Content can be a plain string
        if let Some(text) = content.as_str() {
            entries.push(TranscriptEntry {
                timestamp: timestamp.clone(),
                role: role.clone(),
                kind: "text".to_string(),
                text: text.to_string(),
                tool_name: None,
                tool_use_id: None,
                model: model.clone(),
            });
            continue;
        }

        let blocks = match content.as_array() {
            Some(arr) => arr,
            None => continue,
        };

        for block in blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    entries.push(TranscriptEntry {
                        timestamp: timestamp.clone(),
                        role: role.clone(),
                        kind: "text".to_string(),
                        text: block.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        tool_name: None,
                        tool_use_id: None,
                        model: model.clone(),
                    });
                }
                "thinking" => {
                    entries.push(TranscriptEntry {
                        timestamp: timestamp.clone(),
                        role: role.clone(),
                        kind: "thinking".to_string(),
                        text: block
                            .get("thinking")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        tool_name: None,
                        tool_use_id: None,
                        model: model.clone(),
                    });
                }
                "tool_use" => {
                    let input = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                    entries.push(TranscriptEntry {
                        timestamp: timestamp.clone(),
                        role: role.clone(),
                        kind: "tool_use".to_string(),
                        text: serde_json::to_string(&input).unwrap_or_default(),
                        tool_name: block.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        tool_use_id: block.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        model: model.clone(),
                    });
                }
                "tool_result" => {
                    let result_content = block.get("content").cloned().unwrap_or(Value::String(String::new()));
                    let text = if let Some(arr) = result_content.as_array() {
                        let parts: Vec<String> = arr
                            .iter()
                            .map(|rc| {
                                if let Some(obj) = rc.as_object() {
                                    obj.get("text")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| serde_json::to_string(rc).unwrap_or_default())
                                } else {
                                    rc.to_string()
                                }
                            })
                            .collect();
                        parts.join("\n")
                    } else {
                        result_content.as_str().unwrap_or("").to_string()
                    };
                    entries.push(TranscriptEntry {
                        timestamp: timestamp.clone(),
                        role: role.clone(),
                        kind: "tool_result".to_string(),
                        text,
                        tool_name: None,
                        tool_use_id: block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        model: None,
                    });
                }
                _ => {}
            }
        }
    }

    entries
}

/// Find transcript JSONL file path from session events.
///
/// Tries three strategies:
/// 1. Look for explicit transcript_path in event metadata (hook format)
/// 2. Extract session_id from source URI and discover on disk
/// 3. Fall back to first event's source URI for session_id
pub fn find_transcript_path(events: &[Value]) -> Option<String> {
    // Strategy 1+2: Look for session.start events (legacy or unified)
    for e in events {
        let etype = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let subtype = e.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

        let is_start = etype.ends_with(".session.start")
            || (etype == "io.arc.event" && subtype == "system.session.start");

        if is_start {
            // Try explicit transcript_path in metadata
            let meta = e
                .get("data")
                .and_then(|d| d.get("meta"))
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            if let Some(path) = meta.get("transcript_path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }

        // Try extracting session_id from source URI
        let source = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(session_id) = extract_session_id_from_source(source) {
            if let Some(path) = discover_transcript(&session_id) {
                return Some(path);
            }
        }
    }

    // Strategy 3: Try the first event's source URI
    if let Some(first) = events.first() {
        let source = first.get("source").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(session_id) = extract_session_id_from_source(source) {
            return discover_transcript(&session_id);
        }
    }

    None
}

/// Extract session_id from source URIs like "arc://transcript/{id}" or "arc://hooks/{id}"
fn extract_session_id_from_source(source: &str) -> Option<String> {
    if source.starts_with("arc://") {
        let id = source.rsplit('/').next().unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

/// Auto-discover transcript file in ~/.claude/projects/**/<session_id>.jsonl.
pub fn discover_transcript(session_id: &str) -> Option<String> {
    let home = if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE").ok()?
    } else {
        std::env::var("HOME").ok()?
    };
    let claude_dir = Path::new(&home).join(".claude").join("projects");
    if !claude_dir.is_dir() {
        return None;
    }

    // Search one level of subdirectories
    if let Ok(entries) = std::fs::read_dir(&claude_dir) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(format!("{session_id}.jsonl"));
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_transcript_user_and_assistant() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            "{}",
            json!({
                "type": "user",
                "timestamp": "2026-01-01T00:00:00Z",
                "message": {"role": "user", "content": "Hello"}
            })
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({
                "type": "assistant",
                "timestamp": "2026-01-01T00:00:01Z",
                "message": {
                    "role": "assistant",
                    "model": "claude-sonnet-4-20250514",
                    "content": [{"type": "text", "text": "Hi there!"}]
                }
            })
        )
        .unwrap();

        let entries = read_transcript(f.path());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].role, "user");
        assert_eq!(entries[0].text, "Hello");
        assert_eq!(entries[1].role, "assistant");
        assert_eq!(entries[1].text, "Hi there!");
        assert_eq!(entries[1].model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_read_transcript_tool_use() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            "{}",
            json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "name": "Read", "id": "tu_1", "input": {"file_path": "/tmp/test"}}
                    ]
                }
            })
        )
        .unwrap();

        let entries = read_transcript(f.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, "tool_use");
        assert_eq!(entries[0].tool_name.as_deref(), Some("Read"));
    }

    #[test]
    fn test_skips_non_message_types() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", json!({"type": "progress", "data": {}})).unwrap();
        writeln!(f, "{}", json!({"type": "file-history-snapshot"})).unwrap();
        let entries = read_transcript(f.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_find_transcript_path_from_meta() {
        let events = vec![json!({
            "type": "io.arc.session.start",
            "source": "arc://hooks/test-sess",
            "data": {"meta": {"transcript_path": "/tmp/test.jsonl"}}
        })];
        assert_eq!(
            find_transcript_path(&events),
            Some("/tmp/test.jsonl".to_string())
        );
    }
}
