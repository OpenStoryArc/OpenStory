//! Snapshot-diff file watcher for Hermes Agent sessions.
//!
//! Hermes Agent writes session state as a JSON snapshot that gets atomically
//! overwritten after every assistant response via `atomic_json_write()`
//! (temp file → fsync → os.replace). This is fundamentally different from
//! the append-only JSONL model used by Claude Code and pi-mono.
//!
//! This module is a **separate actor** from `watcher.rs`:
//!
//! | Concern          | watcher.rs (JSONL)        | snapshot_watcher.rs       |
//! |------------------|---------------------------|---------------------------|
//! | Model            | Stream processor          | State machine             |
//! | Input            | Append-only byte stream   | Whole-file snapshots      |
//! | State            | Byte offset (monotonic)   | Message count + session_id|
//! | Read             | Incremental (from offset) | Full file every time      |
//! | Edge cases       | Partial lines             | Compression, undo, splits |
//!
//! Both actors produce the same output: `Vec<CloudEvent>` via a callback
//! with signature `FnMut(&str, Option<&str>, &str, Vec<CloudEvent>)`.
//!
//! Performance characteristics (from stress testing):
//! - Parse + diff + translate: p50 = 286µs at 60KB file sizes
//! - Zero parse failures under adversarial load (16K writes, 58K reads)
//! - 100% message detection at 12 writes/sec (fast model simulation)
//! - Linear scaling: 10MB parses in ~32ms

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;
use walkdir::WalkDir;

use open_story_core::cloud_event::CloudEvent;
use open_story_core::translate::TranscriptState;
use open_story_core::translate_hermes::{is_hermes_format, translate_hermes_line};

use crate::paths::session_id_from_path;

/// Per-file state for the snapshot diff.
///
/// Tracks the previously-seen message count and session_id so we can
/// emit only NEW messages on each file change.
pub struct SnapshotState {
    /// The session_id from the last read. Changes on compression.
    pub session_id: String,
    /// Number of messages seen in the last read.
    pub message_count: usize,
    /// Translation state for CloudEvent generation.
    pub transcript: TranscriptState,
}

impl SnapshotState {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id: session_id.clone(),
            message_count: 0,
            transcript: TranscriptState::new(session_id),
        }
    }
}

/// Diff a snapshot file against previously-seen state.
///
/// Returns new messages (as Hermes-native dicts from the `messages` array)
/// and updates `state` in place.
///
/// Handles three cases:
/// 1. **Append** (normal): message_count grew → emit messages[prev_count..]
/// 2. **Compression**: session_id changed → emit ALL messages (new session)
/// 3. **Undo/retry**: message_count shrank → reset and emit ALL messages
pub fn diff_snapshot(
    state: &mut SnapshotState,
    snapshot: &Value,
) -> Vec<Value> {
    let curr_sid = snapshot
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let messages = match snapshot.get("messages").and_then(|v| v.as_array()) {
        Some(m) => m,
        None => return vec![],
    };
    let curr_count = messages.len();

    if curr_sid != state.session_id || curr_count < state.message_count {
        // Case 2 or 3: session_id changed (compression) or messages shrank (undo).
        // Reset state and emit all messages.
        state.session_id = curr_sid.to_string();
        state.message_count = curr_count;
        state.transcript = TranscriptState::new(curr_sid.to_string());
        return messages.clone();
    }

    if curr_count == state.message_count {
        // No new messages.
        return vec![];
    }

    // Case 1: normal append. Emit only new messages.
    let new_messages = messages[state.message_count..].to_vec();
    state.message_count = curr_count;
    new_messages
}

/// Wrap a raw Hermes message dict in the plugin envelope format
/// that `translate_hermes_line()` expects.
///
/// The snapshot contains bare messages (`{role, content, tool_calls, ...}`).
/// The translator expects the plugin envelope:
/// `{envelope: {session_id, event_seq, source: "hermes"}, event_type: "message", data: <msg>}`
fn wrap_message(session_id: &str, seq: usize, msg: &Value, timestamp: &str) -> Value {
    serde_json::json!({
        "envelope": {
            "session_id": session_id,
            "event_seq": seq,
            "timestamp": timestamp,
            "source": "hermes",
        },
        "event_type": "message",
        "data": msg,
    })
}

/// Wrap session metadata as a session_start event.
fn wrap_session_start(snapshot: &Value) -> Value {
    let sid = snapshot.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
    let model = snapshot.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let platform = snapshot.get("platform").and_then(|v| v.as_str()).unwrap_or("cli");
    let system_prompt = snapshot.get("system_prompt").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = snapshot.get("session_start").and_then(|v| v.as_str()).unwrap_or("");

    let tools: Vec<String> = snapshot
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    if let Some(s) = t.as_str() {
                        Some(s.to_string())
                    } else {
                        t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    serde_json::json!({
        "envelope": {
            "session_id": sid,
            "event_seq": 0,
            "timestamp": timestamp,
            "source": "hermes",
            "model": model,
            "platform": platform,
        },
        "event_type": "session_start",
        "data": {
            "system_prompt_preview": &system_prompt[..system_prompt.len().min(500)],
            "tools": tools.iter().take(15).collect::<Vec<_>>(),
        },
    })
}

/// Process a snapshot file: read, diff, translate new messages to CloudEvents.
///
/// Returns the CloudEvents for any new messages found since the last read.
pub fn process_snapshot(
    path: &Path,
    states: &mut HashMap<PathBuf, SnapshotState>,
) -> Result<Vec<CloudEvent>> {
    // Only process session_*.json files
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !fname.starts_with("session_") || !fname.ends_with(".json") {
        return Ok(vec![]);
    }
    // Skip temp files from atomic_json_write
    if fname.starts_with(".") {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(path)?;
    let snapshot: Value = serde_json::from_str(&content)?;

    let sid = snapshot
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if sid.is_empty() {
        return Ok(vec![]);
    }

    let canonical = path.to_path_buf();
    let is_new = !states.contains_key(&canonical);
    let prev_sid = states
        .get(&canonical)
        .map(|s| s.session_id.clone())
        .unwrap_or_default();

    let state = states
        .entry(canonical)
        .or_insert_with(|| SnapshotState::new(sid.clone()));

    let new_messages = diff_snapshot(state, &snapshot);
    if new_messages.is_empty() && !is_new {
        return Ok(vec![]);
    }

    // Detect session_id change (compression) — need to emit session_start
    let session_changed = !is_new && prev_sid != sid;

    let timestamp = snapshot
        .get("last_updated")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut events: Vec<CloudEvent> = Vec::new();

    // Emit session_start on first encounter OR session_id change (compression)
    if is_new || session_changed {
        let start_line = wrap_session_start(&snapshot);
        if is_hermes_format(&start_line) {
            events.extend(translate_hermes_line(&start_line, &mut state.transcript));
        }
    }

    // Translate each new message
    for (i, msg) in new_messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "system" {
            continue; // System prompt is in session_start, not individual events
        }
        let seq = state.message_count - new_messages.len() + i + 1;
        let wrapped = wrap_message(&sid, seq, msg, timestamp);
        if is_hermes_format(&wrapped) {
            events.extend(translate_hermes_line(&wrapped, &mut state.transcript));
        }
    }

    Ok(events)
}

/// Watch a directory of Hermes session snapshots with a callback.
///
/// This blocks the current thread. The callback receives
/// `(session_id, project_id, subject, events)` — same signature as
/// `watcher::watch_with_callback`.
///
/// Unlike the JSONL watcher, this handles atomic-replace file writes
/// (temp file → rename) which Hermes uses for session persistence.
pub fn watch_snapshots<F>(
    watch_dir: &Path,
    backfill_window_hours: Option<u64>,
    mut on_events: F,
) -> Result<()>
where
    F: FnMut(&str, Option<&str>, &str, Vec<CloudEvent>),
{
    let mut states: HashMap<PathBuf, SnapshotState> = HashMap::new();

    // Backfill: process existing session files
    if let Some(window_hours) = backfill_window_hours {
        let now = SystemTime::now();
        let window = if window_hours == 0 {
            None
        } else {
            Some(Duration::from_secs(window_hours * 3600))
        };

        let mut total = 0u64;
        for entry in WalkDir::new(watch_dir)
            .follow_links(true)
            .max_depth(1) // session files are in the top-level directory
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !fname.starts_with("session_") || !fname.ends_with(".json") {
                continue;
            }

            let in_window = match window {
                None => true,
                Some(w) => entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|mtime| now.duration_since(mtime).unwrap_or(Duration::ZERO) <= w)
                    .unwrap_or(false),
            };
            if !in_window {
                continue;
            }

            match process_snapshot(path, &mut states) {
                Ok(events) if !events.is_empty() => {
                    total += events.len() as u64;
                    // Use the session_id from the snapshot JSON, not the filename.
                    // Hermes filenames are `session_<id>.json` but the JSON session_id
                    // is just `<id>` (without the `session_` prefix).
                    let sid = states.get(path)
                        .map(|s| s.session_id.clone())
                        .unwrap_or_else(|| session_id_from_path(path));
                    let subject = format!("events.hermes.{}.main", sid);
                    on_events(&sid, None, &subject, events);
                }
                Ok(_) => {}
                Err(e) => eprintln!("Error processing snapshot {}: {}", path.display(), e),
            }
        }
        eprintln!("Backfilled {} events from Hermes snapshots", total);
    }

    // Watch for changes
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    eprintln!(
        "Watching {} for Hermes session snapshots...",
        watch_dir.display()
    );

    for res in rx {
        match res {
            Ok(event) => {
                // os.replace generates Create or Modify events depending on platform
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    for path in &event.paths {
                        match process_snapshot(path, &mut states) {
                            Ok(events) if !events.is_empty() => {
                                let sid = states.get(path.as_path())
                                    .map(|s| s.session_id.clone())
                                    .unwrap_or_else(|| session_id_from_path(path));
                                let subject = format!("events.hermes.{}.main", sid);
                                on_events(&sid, None, &subject, events);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!(
                                    "Error processing snapshot {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => eprintln!("Snapshot watch error: {}", e),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    // ── Helpers ──────────────────────────────────────────────────

    fn make_snapshot(session_id: &str, messages: Vec<Value>) -> Value {
        json!({
            "session_id": session_id,
            "model": "mock-model",
            "base_url": "http://mock:8000/v1",
            "platform": "cli",
            "session_start": "2026-04-10T10:00:00.000000",
            "last_updated": "2026-04-10T10:01:00.000000",
            "system_prompt": "You are Hermes Agent.",
            "tools": ["read_file", "write_file", "terminal"],
            "message_count": messages.len(),
            "messages": messages,
        })
    }

    fn user_msg(content: &str) -> Value {
        json!({"role": "user", "content": content})
    }

    fn assistant_tool(tool: &str, args: &str) -> Value {
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": format!("tc_{}", tool), "function": {"name": tool, "arguments": args}}],
            "finish_reason": "tool_calls",
        })
    }

    fn tool_result(call_id: &str, content: &str) -> Value {
        json!({"role": "tool", "tool_call_id": call_id, "content": content})
    }

    fn assistant_text(content: &str) -> Value {
        json!({"role": "assistant", "content": content, "finish_reason": "stop"})
    }

    fn atomic_replace(path: &Path, content: &str) {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content).unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }

    // ── BDD: describe diff_snapshot ──────────────────────────────

    mod describe_diff_snapshot {
        use super::*;

        #[test]
        fn it_should_return_empty_when_no_new_messages() {
            let mut state = SnapshotState::new("sess-1".to_string());
            let snapshot = make_snapshot("sess-1", vec![user_msg("hello")]);
            state.message_count = 1;

            let new = diff_snapshot(&mut state, &snapshot);
            assert!(new.is_empty(), "no new messages should return empty");
        }

        #[test]
        fn it_should_return_new_messages_when_count_grows() {
            let mut state = SnapshotState::new("sess-1".to_string());
            state.message_count = 1; // previously saw 1 message

            let snapshot = make_snapshot(
                "sess-1",
                vec![
                    user_msg("hello"),
                    assistant_text("hi there"),
                    user_msg("thanks"),
                ],
            );

            let new = diff_snapshot(&mut state, &snapshot);
            assert_eq!(new.len(), 2, "should return 2 new messages");
            assert_eq!(new[0]["content"], "hi there");
            assert_eq!(new[1]["content"], "thanks");
            assert_eq!(state.message_count, 3);
        }

        #[test]
        fn it_should_emit_all_messages_on_first_read() {
            let mut state = SnapshotState::new("sess-1".to_string());
            // message_count starts at 0

            let snapshot = make_snapshot(
                "sess-1",
                vec![user_msg("hello"), assistant_text("hi")],
            );

            let new = diff_snapshot(&mut state, &snapshot);
            assert_eq!(new.len(), 2, "first read emits all messages");
        }

        #[test]
        fn it_should_reset_on_session_id_change_compression() {
            let mut state = SnapshotState::new("sess-1".to_string());
            state.message_count = 10; // had 10 messages

            // Compression: new session_id, fewer messages
            let snapshot = make_snapshot(
                "sess-2",
                vec![
                    json!({"role": "system", "content": "[Compressed history]"}),
                    user_msg("continue"),
                    assistant_text("ok"),
                ],
            );

            let new = diff_snapshot(&mut state, &snapshot);
            assert_eq!(new.len(), 3, "compression emits ALL messages from new session");
            assert_eq!(state.session_id, "sess-2");
            assert_eq!(state.message_count, 3);
        }

        #[test]
        fn it_should_reset_on_message_shrinkage_undo() {
            let mut state = SnapshotState::new("sess-1".to_string());
            state.message_count = 10;

            // /undo: same session_id, fewer messages
            let snapshot = make_snapshot(
                "sess-1",
                vec![
                    user_msg("hello"),
                    assistant_text("hi"),
                    user_msg("actually..."),
                ],
            );

            let new = diff_snapshot(&mut state, &snapshot);
            assert_eq!(new.len(), 3, "undo emits ALL messages (full reset)");
            assert_eq!(state.message_count, 3);
        }

        #[test]
        fn it_should_handle_empty_messages_array() {
            let mut state = SnapshotState::new("sess-1".to_string());
            let snapshot = make_snapshot("sess-1", vec![]);

            let new = diff_snapshot(&mut state, &snapshot);
            assert!(new.is_empty());
        }

        #[test]
        fn it_should_handle_missing_messages_field() {
            let mut state = SnapshotState::new("sess-1".to_string());
            let snapshot = json!({"session_id": "sess-1"});

            let new = diff_snapshot(&mut state, &snapshot);
            assert!(new.is_empty());
        }
    }

    // ── BDD: describe process_snapshot ───────────────────────────

    mod describe_process_snapshot {
        use super::*;

        #[test]
        fn it_should_produce_cloud_events_for_new_messages() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("session_test001.json");

            let snapshot = make_snapshot(
                "test001",
                vec![
                    user_msg("read the file"),
                    assistant_tool("read_file", r#"{"path":"README.md"}"#),
                    tool_result("tc_read_file", r#"{"content":"hello"}"#),
                    assistant_text("The file says hello."),
                ],
            );
            std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

            let mut states = HashMap::new();
            let events = process_snapshot(&path, &mut states).unwrap();

            assert!(!events.is_empty(), "should produce CloudEvents");

            // Should have: session_start + user + tool_use + tool_result + text
            let subtypes: Vec<&str> = events
                .iter()
                .filter_map(|e| e.subtype.as_deref())
                .collect();
            assert!(
                subtypes.contains(&"system.session.start"),
                "should emit session_start on first read"
            );
            assert!(
                subtypes.contains(&"message.user.prompt"),
                "should emit user prompt"
            );
            assert!(
                subtypes.contains(&"message.assistant.tool_use"),
                "should emit tool_use"
            );
            assert!(
                subtypes.contains(&"message.user.tool_result"),
                "should emit tool_result"
            );
            assert!(
                subtypes.contains(&"message.assistant.text"),
                "should emit final text"
            );

            // All events should carry the hermes agent tag
            for ev in &events {
                assert_eq!(ev.agent.as_deref(), Some("hermes"));
            }
        }

        #[test]
        fn it_should_emit_only_new_messages_on_second_read() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("session_inc001.json");

            // First write: 2 messages
            let snap1 = make_snapshot(
                "inc001",
                vec![user_msg("hello"), assistant_text("hi")],
            );
            std::fs::write(&path, serde_json::to_string(&snap1).unwrap()).unwrap();

            let mut states = HashMap::new();
            let events1 = process_snapshot(&path, &mut states).unwrap();
            let count1 = events1.len();

            // Second write: 4 messages (2 new)
            let snap2 = make_snapshot(
                "inc001",
                vec![
                    user_msg("hello"),
                    assistant_text("hi"),
                    user_msg("read file"),
                    assistant_tool("read_file", r#"{"path":"x"}"#),
                ],
            );
            atomic_replace(&path, &serde_json::to_string(&snap2).unwrap());

            let events2 = process_snapshot(&path, &mut states).unwrap();

            assert!(
                events2.len() < count1,
                "second read ({}) should emit fewer events than first ({})",
                events2.len(),
                count1
            );

            // Should have user + tool_use (the 2 new messages)
            let subtypes: Vec<&str> = events2
                .iter()
                .filter_map(|e| e.subtype.as_deref())
                .collect();
            assert!(subtypes.contains(&"message.user.prompt"));
            assert!(subtypes.contains(&"message.assistant.tool_use"));
            // Should NOT re-emit session_start
            assert!(
                !subtypes.contains(&"system.session.start"),
                "should not re-emit session_start"
            );
        }

        #[test]
        fn it_should_skip_non_session_files() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("config.json");
            std::fs::write(&path, "{}").unwrap();

            let mut states = HashMap::new();
            let events = process_snapshot(&path, &mut states).unwrap();
            assert!(events.is_empty(), "should skip non-session files");
        }

        #[test]
        fn it_should_skip_dotfiles() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join(".session_test.json.tmp");
            std::fs::write(&path, "{}").unwrap();

            let mut states = HashMap::new();
            let events = process_snapshot(&path, &mut states).unwrap();
            assert!(events.is_empty(), "should skip temp/dotfiles");
        }

        #[test]
        fn it_should_skip_system_messages_in_output() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("session_sys001.json");

            let snapshot = make_snapshot(
                "sys001",
                vec![
                    json!({"role": "system", "content": "You are helpful."}),
                    user_msg("hello"),
                    assistant_text("hi"),
                ],
            );
            std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

            let mut states = HashMap::new();
            let events = process_snapshot(&path, &mut states).unwrap();

            // System messages from the messages array should be skipped
            // (the system prompt is in session_start instead)
            let subtypes: Vec<&str> = events
                .iter()
                .filter_map(|e| e.subtype.as_deref())
                .collect();
            assert!(
                !subtypes.iter().any(|s| s.contains("system.injected")),
                "system messages should not be emitted as separate events"
            );
        }

        #[test]
        fn it_should_handle_compression_as_new_session() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("session_comp001.json");

            // First session
            let snap1 = make_snapshot(
                "comp001",
                vec![user_msg("hello"), assistant_text("hi")],
            );
            std::fs::write(&path, serde_json::to_string(&snap1).unwrap()).unwrap();

            let mut states = HashMap::new();
            let _events1 = process_snapshot(&path, &mut states).unwrap();

            // Compression: new session_id, different messages
            let snap2 = make_snapshot(
                "comp002",
                vec![
                    json!({"role": "system", "content": "[Compressed] previous conversation..."}),
                    user_msg("continue from where we left off"),
                ],
            );
            atomic_replace(&path, &serde_json::to_string(&snap2).unwrap());

            let events2 = process_snapshot(&path, &mut states).unwrap();

            // Should emit session_start for the new session
            let subtypes: Vec<&str> = events2
                .iter()
                .filter_map(|e| e.subtype.as_deref())
                .collect();
            assert!(
                subtypes.contains(&"system.session.start"),
                "compression should emit new session_start"
            );
            assert!(
                subtypes.contains(&"message.user.prompt"),
                "should emit messages from new session"
            );
        }
    }

    // ── BDD: describe wrap_message ───────────────────────────────

    mod describe_wrap_message {
        use super::*;

        #[test]
        fn it_should_produce_hermes_format_envelope() {
            let msg = user_msg("hello");
            let wrapped = wrap_message("sess-1", 1, &msg, "2026-04-10T10:00:00Z");

            assert!(is_hermes_format(&wrapped), "wrapped message should be detected as Hermes format");
            assert_eq!(wrapped["envelope"]["source"], "hermes");
            assert_eq!(wrapped["envelope"]["session_id"], "sess-1");
            assert_eq!(wrapped["event_type"], "message");
            assert_eq!(wrapped["data"]["role"], "user");
        }

        #[test]
        fn it_should_carry_correct_sequence_number() {
            let msg = assistant_text("done");
            let wrapped = wrap_message("sess-1", 42, &msg, "2026-04-10T10:00:00Z");

            assert_eq!(wrapped["envelope"]["event_seq"], 42);
        }
    }

    // ── BDD: describe wrap_session_start ─────────────────────────

    mod describe_wrap_session_start {
        use super::*;

        #[test]
        fn it_should_produce_session_start_event() {
            let snapshot = make_snapshot(
                "sess-1",
                vec![user_msg("hello")],
            );
            let start = wrap_session_start(&snapshot);

            assert!(is_hermes_format(&start));
            assert_eq!(start["event_type"], "session_start");
            assert_eq!(start["envelope"]["model"], "mock-model");
            assert_eq!(start["envelope"]["platform"], "cli");
        }

        #[test]
        fn it_should_extract_tool_names_from_function_objects() {
            let snapshot = json!({
                "session_id": "sess-1",
                "model": "test",
                "platform": "cli",
                "session_start": "",
                "system_prompt": "",
                "tools": [
                    {"function": {"name": "read_file"}},
                    {"function": {"name": "terminal"}},
                    "write_file",
                ],
                "messages": [],
            });
            let start = wrap_session_start(&snapshot);

            let tools = start["data"]["tools"].as_array().unwrap();
            let names: Vec<&str> = tools.iter().filter_map(|t| t.as_str()).collect();
            assert!(names.contains(&"read_file"));
            assert!(names.contains(&"terminal"));
            assert!(names.contains(&"write_file"));
        }
    }
}
