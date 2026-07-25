//! Live-watch simulation: append ACP lines to updates.jsonl, read incrementally.
//!
//! Mirrors the shape of `test_codex_live_watch.rs` but for Grok's path layout:
//! `{watch}/{encoded-cwd}/{session-id}/updates.jsonl`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;
use tempfile::TempDir;

const SESSION: &str = "019f6cb5-f7e4-7bc1-bb25-aaaaaaaaaaaa";
const CWD_ENC: &str = "%2Fworkspace%2Fdemo";

fn grok_tree(root: &Path) -> PathBuf {
    root.join("sessions").join(CWD_ENC).join(SESSION)
}

fn updates_path(root: &Path) -> PathBuf {
    grok_tree(root).join("updates.jsonl")
}

fn append_line(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open updates.jsonl");
    writeln!(file, "{line}").expect("write");
    file.flush().expect("flush");
    file.sync_data().expect("sync");
}

fn acp_user(text: &str, event_id: &str) -> String {
    format!(
        r#"{{"timestamp":1700000000,"method":"session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"{text}"}}}},"_meta":{{"eventId":"{event_id}"}}}}}}"#
    )
}

fn acp_tool_call(call_id: &str, event_id: &str) -> String {
    format!(
        r#"{{"timestamp":1700000001,"method":"session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"tool_call","toolCallId":"{call_id}","title":"list_dir","rawInput":{{"target_directory":"/workspace/demo"}},"_meta":{{"x.ai/tool":{{"name":"list_dir"}}}}}},"_meta":{{"eventId":"{event_id}"}}}}}}"#
    )
}

fn acp_tool_done(call_id: &str, event_id: &str) -> String {
    format!(
        r#"{{"timestamp":1700000002,"method":"session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"tool_call_update","toolCallId":"{call_id}","status":"completed","rawOutput":{{"Content":{{"content":"a\nb"}}}}}},"_meta":{{"eventId":"{event_id}"}}}}}}"#
    )
}

fn acp_turn_done(event_id: &str) -> String {
    format!(
        r#"{{"timestamp":1700000003,"method":"_x.ai/session/update","params":{{"sessionId":"{SESSION}","update":{{"sessionUpdate":"turn_completed","prompt_id":"p-live","stop_reason":"end_turn","usage":{{"inputTokens":10,"outputTokens":2}}}},"_meta":{{"eventId":"{event_id}"}}}}}}"#
    )
}

fn read_path(path: &Path, states: &mut HashMap<PathBuf, TranscriptState>) -> Vec<String> {
    if path.file_name().and_then(|s| s.to_str()) != Some("updates.jsonl") {
        // Simulate the production filter: only updates.jsonl is a transcript.
        return Vec::new();
    }
    let state = states
        .entry(path.to_path_buf())
        .or_insert_with(|| TranscriptState::new(SESSION.to_string()));
    read_new_lines(path, state)
        .expect("read")
        .into_iter()
        .filter_map(|e| e.subtype)
        .collect()
}

#[test]
fn grok_incremental_appends_are_translated_in_order() {
    let tmp = TempDir::new().unwrap();
    let dir = grok_tree(tmp.path());
    fs::create_dir_all(&dir).unwrap();
    let path = updates_path(tmp.path());
    // Create empty file first (watcher would create on first write).
    fs::write(&path, "").unwrap();

    let mut states = HashMap::new();

    // Wave 1: user prompt only
    append_line(&path, &acp_user("hello live", "e1"));
    let s1 = read_path(&path, &mut states);
    assert_eq!(s1, vec!["message.user.prompt".to_string()]);

    // Wave 2: tool call + result
    append_line(&path, &acp_tool_call("call-live", "e2"));
    append_line(&path, &acp_tool_done("call-live", "e3"));
    let s2 = read_path(&path, &mut states);
    assert_eq!(
        s2,
        vec![
            "message.assistant.tool_use".to_string(),
            "message.user.tool_result".to_string(),
        ]
    );

    // Wave 3: turn boundary
    append_line(&path, &acp_turn_done("e4"));
    let s3 = read_path(&path, &mut states);
    assert_eq!(s3, vec!["system.turn.complete".to_string()]);

    // Session id rekeyed from wire
    let st = states.get(&path).expect("state");
    assert_eq!(st.session_id, SESSION);
    assert_eq!(
        st.format,
        open_story::translate::TranscriptFormat::Grok
    );

    // Sibling noise files must not initialize transcript state if filtered.
    let noise = dir.join("chat_history.jsonl");
    fs::write(&noise, "{\"type\":\"user\"}\n").unwrap();
    let noise_subtypes = read_path(&noise, &mut states);
    assert!(
        noise_subtypes.is_empty(),
        "chat_history.jsonl must be ignored"
    );
    assert!(
        !states.contains_key(&noise),
        "noise path must not create TranscriptState"
    );
}

#[test]
fn path_helpers_resolve_grok_session_and_project() {
    use open_story_core::paths::{nats_subject_from_path, project_id_from_path, session_id_from_path};

    let tmp = TempDir::new().unwrap();
    let watch = tmp.path().join("sessions");
    let path = watch
        .join(CWD_ENC)
        .join(SESSION)
        .join("updates.jsonl");

    assert_eq!(session_id_from_path(&path), SESSION);
    assert_eq!(
        project_id_from_path(&path, &watch).as_deref(),
        Some(CWD_ENC)
    );
    let subject = nats_subject_from_path(&path, &watch, "testhost");
    assert_eq!(
        subject,
        format!("events.testhost.{CWD_ENC}.{SESSION}.main")
    );
}
