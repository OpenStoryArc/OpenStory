//! Grok L2 enrichment: session sibling artifacts → CloudEvents.
//!
//! L1 (`translate_grok`) owns the ACP `updates.jsonl` stream.
//! L2 observes durable sidecars under the same session directory:
//!
//! | Artifact | subtype |
//! |----------|---------|
//! | `terminal/{toolCallId}.log` | `message.user.tool_result` (full shell I/O) |
//! | `hunk_records.jsonl` | `file.hunk` |
//! | `summary.json` | `system.grok.summary` |
//! | `signals.json` | `system.grok.signals` |
//! | `images/*`, `assets/*` | `file.attachment` |
//!
//! Large bodies: inline when ≤ [`TERMINAL_INLINE_MAX`]; otherwise preview +
//! path/sha256 in payload extra (sovereignty via open files).

use std::path::Path;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::cloud_event::CloudEvent;
use crate::event_data::{AgentPayload, EventData, GrokPayload};
use crate::translate::IO_ARC_EVENT;

const AGENT: &str = "grok";

/// Max chars of terminal log inlined into the event payload.
pub const TERMINAL_INLINE_MAX: usize = 100_000;

/// Detect a Grok hunk_records.jsonl line.
pub fn is_hunk_record(line: &Value) -> bool {
    line.get("hunkId").and_then(|v| v.as_str()).is_some()
        && line.get("filePath").and_then(|v| v.as_str()).is_some()
}

/// Translate one hunk_records.jsonl object.
pub fn translate_hunk_record(
    line: &Value,
    session_id: &str,
    seq: u64,
) -> Option<CloudEvent> {
    if !is_hunk_record(line) {
        return None;
    }
    let hunk_id = line.get("hunkId")?.as_str()?;
    let file_path = line.get("filePath")?.as_str()?.to_string();
    let event_id = stable_id(session_id, "hunk", hunk_id);

    let mut payload = GrokPayload::new();
    payload.text = Some(file_path.clone());
    payload.extra.insert("hunk_id".into(), json!(hunk_id));
    payload
        .extra
        .insert("file_path".into(), json!(file_path));
    if let Some(v) = line.get("hunkStart") {
        payload.extra.insert("hunk_start".into(), v.clone());
    }
    if let Some(v) = line.get("hunkEnd") {
        payload.extra.insert("hunk_end".into(), v.clone());
    }
    if let Some(v) = line.get("linesAdded") {
        payload.extra.insert("lines_added".into(), v.clone());
    }
    if let Some(v) = line.get("linesRemoved") {
        payload.extra.insert("lines_removed".into(), v.clone());
    }
    if let Some(v) = line.get("eventType") {
        payload.extra.insert("event_type".into(), v.clone());
    }
    if let Some(v) = line.get("timestamp") {
        payload.extra.insert("source_timestamp".into(), v.clone());
    }

    let ts = line
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Some(make_l2_event(
        line,
        session_id,
        seq,
        "file.hunk",
        payload,
        Some(event_id),
        ts,
    ))
}

/// Translate a `terminal/{toolCallId}.log` file into a full tool_result event.
///
/// `tool_call_id` is the file stem (Grok names logs `call-….log`).
/// When content exceeds [`TERMINAL_INLINE_MAX`], only a preview is inlined;
/// full path + sha256 land in `extra` so the log remains the source of truth.
pub fn translate_terminal_log(
    session_id: &str,
    tool_call_id: &str,
    log_path: &Path,
    content: &str,
    seq: u64,
) -> CloudEvent {
    let content_key = Uuid::new_v5(&Uuid::NAMESPACE_URL, content.as_bytes()).to_string();
    let event_id = stable_id(
        session_id,
        "terminal",
        &format!("{tool_call_id}:{content_key}"),
    );

    let (text, truncated) = if content.len() <= TERMINAL_INLINE_MAX {
        (content.to_string(), false)
    } else {
        let mut preview: String = content.chars().take(TERMINAL_INLINE_MAX).collect();
        preview.push_str("\n…[truncated; full log at path in payload]");
        (preview, true)
    };

    let mut payload = GrokPayload::new();
    payload.text = Some(text);
    payload.tool = Some("run_terminal_command".into());
    payload.tool_call_id = Some(tool_call_id.to_string());
    payload.extra.insert(
        "artifact".into(),
        json!({
            "kind": "terminal_log",
            "path": log_path.to_string_lossy(),
            "content_key": content_key,
            "bytes": content.len(),
            "truncated": truncated,
        }),
    );

    let raw = json!({
        "source": "grok.terminal_log",
        "tool_call_id": tool_call_id,
        "path": log_path.to_string_lossy(),
        "content_key": content_key,
        "bytes": content.len(),
    });

    make_l2_event(
        &raw,
        session_id,
        seq,
        "message.user.tool_result",
        payload,
        Some(event_id),
        None,
    )
}

/// Translate `summary.json` → session metadata event.
pub fn translate_summary_json(session_id: &str, value: &Value, seq: u64) -> CloudEvent {
    let hash = content_hash(value);
    let event_id = stable_id(session_id, "summary", &hash);

    let mut payload = GrokPayload::new();
    payload.text = value
        .get("session_summary")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if let Some(info) = value.get("info") {
        payload.extra.insert("info".into(), info.clone());
    }
    for key in [
        "num_messages",
        "num_chat_messages",
        "current_model_id",
        "head_branch",
        "head_commit",
        "git_root_dir",
        "updated_at",
        "created_at",
    ] {
        if let Some(v) = value.get(key) {
            payload.extra.insert(key.to_string(), v.clone());
        }
    }

    make_l2_event(
        value,
        session_id,
        seq,
        "system.grok.summary",
        payload,
        Some(event_id),
        value
            .get("updated_at")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    )
}

/// Translate `signals.json` → session signals event.
pub fn translate_signals_json(session_id: &str, value: &Value, seq: u64) -> CloudEvent {
    let hash = content_hash(value);
    let event_id = stable_id(session_id, "signals", &hash);

    let mut payload = GrokPayload::new();
    payload.text = Some("session signals".into());
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            payload.extra.insert(k.clone(), v.clone());
        }
    }

    make_l2_event(
        value,
        session_id,
        seq,
        "system.grok.signals",
        payload,
        Some(event_id),
        None,
    )
}

/// Translate an image/asset path under the session dir.
pub fn translate_attachment(
    session_id: &str,
    rel_path: &str,
    abs_path: &Path,
    bytes: u64,
    seq: u64,
) -> CloudEvent {
    let event_id = stable_id(session_id, "attachment", rel_path);

    let mut payload = GrokPayload::new();
    payload.text = Some(rel_path.to_string());
    payload.extra.insert(
        "attachment".into(),
        json!({
            "path": abs_path.to_string_lossy(),
            "rel_path": rel_path,
            "bytes": bytes,
        }),
    );

    let raw = json!({
        "source": "grok.attachment",
        "path": abs_path.to_string_lossy(),
        "rel_path": rel_path,
        "bytes": bytes,
    });

    make_l2_event(
        &raw,
        session_id,
        seq,
        "file.attachment",
        payload,
        Some(event_id),
        None,
    )
}

/// Stable UUIDv5 for L2 events so boot re-scan is idempotent at ingest.
pub fn stable_id(session_id: &str, kind: &str, key: &str) -> String {
    let seed = format!("grok:{session_id}:{kind}:{key}");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

fn content_hash(value: &Value) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, value.to_string().as_bytes()).to_string()
}

fn make_l2_event(
    raw: &Value,
    session_id: &str,
    seq: u64,
    subtype: &str,
    payload: GrokPayload,
    event_id: Option<String>,
    timestamp: Option<String>,
) -> CloudEvent {
    let data = EventData::with_payload(
        raw.clone(),
        seq,
        session_id.to_string(),
        AgentPayload::Grok(payload),
    );
    let source = format!("grok://session/{session_id}");
    CloudEvent::new(
        source,
        IO_ARC_EVENT.to_string(),
        data,
        Some(subtype.to_string()),
        event_id,
        timestamp,
        None,
        None,
        Some(AGENT.to_string()),
    )
    .with_host(crate::host::host())
    .with_user(crate::user::user())
}

/// Extract tool_call_id from a terminal log path (`…/terminal/call-….log`).
pub fn tool_call_id_from_terminal_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".log") {
        return None;
    }
    let parent = path.parent()?.file_name()?.to_str()?;
    if parent != "terminal" {
        return None;
    }
    Some(name.trim_end_matches(".log").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_data::AgentPayload;
    use serde_json::json;

    fn grok_payload(ev: &CloudEvent) -> &crate::event_data::GrokPayload {
        match ev.data.agent_payload.as_ref() {
            Some(AgentPayload::Grok(g)) => g,
            _ => panic!("expected Grok payload"),
        }
    }

    // describe("when a hunk_records.jsonl line is translated")
    mod when_hunk_record_is_translated {
        use super::*;

        fn sample_hunk() -> Value {
            json!({
                "hunkId": "78b844d9-d0be-4e16-bc0e-ffc22996cbd3",
                "filePath": "/Users/me/proj/file.rs",
                "hunkStart": 1,
                "hunkEnd": 10,
                "linesAdded": 5,
                "linesRemoved": 2,
                "timestamp": "2026-07-17T00:00:00Z"
            })
        }

        #[test]
        fn it_should_emit_file_hunk_with_agent_grok() {
            let ev = translate_hunk_record(&sample_hunk(), "sess-1", 1).unwrap();
            assert_eq!(ev.subtype.as_deref(), Some("file.hunk"));
            assert_eq!(ev.agent.as_deref(), Some("grok"));
            assert_eq!(ev.data.session_id, "sess-1");
        }

        #[test]
        fn it_should_carry_the_edited_path_as_text() {
            let ev = translate_hunk_record(&sample_hunk(), "sess-1", 1).unwrap();
            assert_eq!(
                grok_payload(&ev).text.as_deref(),
                Some("/Users/me/proj/file.rs")
            );
            assert_eq!(
                grok_payload(&ev)
                    .extra
                    .get("lines_added")
                    .and_then(|v| v.as_i64()),
                Some(5)
            );
        }

        #[test]
        fn it_should_use_stable_ids_so_boot_replay_dedups() {
            let a = translate_hunk_record(&sample_hunk(), "sess-1", 1).unwrap();
            let b = translate_hunk_record(&sample_hunk(), "sess-1", 99).unwrap();
            assert_eq!(a.id, b.id);
        }

        #[test]
        fn it_should_reject_non_hunk_objects() {
            assert!(translate_hunk_record(&json!({"type": "nope"}), "s", 1).is_none());
        }
    }

    // describe("when a terminal/{toolCallId}.log is translated")
    mod when_terminal_log_is_translated {
        use super::*;

        #[test]
        fn it_should_emit_tool_result_whose_text_equals_the_log_body() {
            let path = Path::new(
                "/Users/me/.grok/sessions/p/sess/terminal/call-abc-1.log",
            );
            let body = "exit: 0\nhello from shell\n";
            let ev = translate_terminal_log("sess", "call-abc-1", path, body, 3);
            assert_eq!(ev.subtype.as_deref(), Some("message.user.tool_result"));
            assert_eq!(ev.agent.as_deref(), Some("grok"));
            let g = grok_payload(&ev);
            assert_eq!(g.text.as_deref(), Some(body));
            assert_eq!(g.tool_call_id.as_deref(), Some("call-abc-1"));
            assert_eq!(g.tool.as_deref(), Some("run_terminal_command"));
        }

        #[test]
        fn it_should_truncate_large_bodies_but_preserve_byte_count_and_path() {
            let path = Path::new("/tmp/sess/terminal/call-big.log");
            let big = "x".repeat(TERMINAL_INLINE_MAX + 50);
            let ev = translate_terminal_log("sess", "call-big", path, &big, 1);
            let g = grok_payload(&ev);
            let text = g.text.as_deref().unwrap();
            assert!(text.len() < big.len());
            assert!(text.contains("truncated"));
            let art = g.extra.get("artifact").expect("artifact ref");
            assert_eq!(art.get("truncated").and_then(|v| v.as_bool()), Some(true));
            assert_eq!(
                art.get("bytes").and_then(|v| v.as_u64()),
                Some(big.len() as u64)
            );
            assert!(
                art.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .ends_with("call-big.log")
            );
        }
    }

    // describe("when summary.json / signals.json are translated")
    mod when_session_meta_json_is_translated {
        use super::*;

        #[test]
        fn it_should_surface_session_summary_text() {
            let summary = json!({
                "session_summary": "Working on parity",
                "current_model_id": "grok-4.5",
                "num_messages": 12,
                "updated_at": "2026-07-17T12:00:00Z"
            });
            let s = translate_summary_json("sess", &summary, 1);
            assert_eq!(s.subtype.as_deref(), Some("system.grok.summary"));
            assert_eq!(
                s.data.agent_payload.as_ref().unwrap().text(),
                Some("Working on parity")
            );
            assert_eq!(
                grok_payload(&s)
                    .extra
                    .get("current_model_id")
                    .and_then(|v| v.as_str()),
                Some("grok-4.5")
            );
        }

        #[test]
        fn it_should_surface_signal_counters() {
            let signals = json!({"turnCount": 5, "errorCount": 0});
            let sig = translate_signals_json("sess", &signals, 2);
            assert_eq!(sig.subtype.as_deref(), Some("system.grok.signals"));
            assert_eq!(
                grok_payload(&sig)
                    .extra
                    .get("turnCount")
                    .and_then(|v| v.as_i64()),
                Some(5)
            );
        }
    }

    // describe("when parsing a terminal log path")
    mod when_parsing_terminal_log_path {
        use super::*;

        #[test]
        fn it_should_extract_tool_call_id_from_stem() {
            let p = Path::new(
                "/Users/me/.grok/sessions/x/019f6d6e-ada9-7240-b26c-036ec49af2a7/terminal/call-xyz-9.log",
            );
            assert_eq!(
                tool_call_id_from_terminal_path(p).as_deref(),
                Some("call-xyz-9")
            );
        }

        #[test]
        fn it_should_reject_non_terminal_paths() {
            assert!(tool_call_id_from_terminal_path(Path::new("/tmp/foo.txt")).is_none());
            assert!(tool_call_id_from_terminal_path(Path::new(
                "/tmp/not-terminal/call-1.log"
            ))
            .is_none());
        }
    }
}
