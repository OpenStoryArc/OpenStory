//! Comprehensive parameterized end-to-end tests for the Hermes integration.
//!
//! Tests the full pipeline: Hermes JSONL/snapshot → translator → CloudEvents
//! → views → ViewRecords. Covers:
//!
//! - Parameterized session tests (all 8 real fixtures)
//! - Parameterized message type tests (each role → correct subtype)
//! - Parameterized tool type tests (each Hermes tool → correct payload)
//! - Snapshot watcher pipeline test (process_snapshot → ViewRecords)
//! - Multi-tool ID uniqueness test (B2 fix validation)
//! - Stop reason preservation test (C2 fix validation)
//! - Raw preservation test (C1 validation)
//!
//! Run:
//!   cargo test -p open-story --test test_hermes_e2e

use open_story::snapshot_watcher::{diff_snapshot, process_snapshot, SnapshotState};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;
use open_story_core::translate::TranscriptState;
use open_story_core::translate_hermes::{is_hermes_format, translate_hermes_line};
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes")
        .join(name)
}

/// Load a fixture JSONL, translate every line, return events + raw lines.
fn load_and_translate(name: &str) -> (Vec<CloudEvent>, Vec<Value>) {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture not found at {:?}: {}", path, e));

    // Extract session_id from the first line's envelope
    let first_line: Value = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .expect("fixture must have at least one line");
    let session_id = first_line["envelope"]["session_id"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let mut state = TranscriptState::new(session_id);
    let mut events: Vec<CloudEvent> = Vec::new();
    let mut raw_lines: Vec<Value> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("bad JSON in fixture {}: {}", name, e));
        raw_lines.push(parsed.clone());
        events.extend(translate_hermes_line(&parsed, &mut state));
    }

    (events, raw_lines)
}

/// Extract HermesPayload from a CloudEvent, panicking if not Hermes.
fn hermes_payload(
    event: &CloudEvent,
) -> &open_story_core::event_data::HermesPayload {
    match event
        .data
        .agent_payload
        .as_ref()
        .expect("missing agent_payload")
    {
        AgentPayload::Hermes(p) => p,
        other => panic!("expected Hermes payload, got {:?}", other),
    }
}

/// Wrap a raw Hermes message in the plugin envelope.
fn wrap_message(session_id: &str, seq: u64, msg: Value) -> Value {
    json!({
        "envelope": {
            "session_id": session_id,
            "event_seq": seq,
            "timestamp": "2026-04-10T10:00:00Z",
            "source": "hermes",
        },
        "event_type": "message",
        "data": msg,
    })
}

/// Build a snapshot JSON value (same shape as session_*.json files).
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

// ═══════════════════════════════════════════════════════════════════════
// 1. Parameterized session tests — every real fixture through the pipeline
// ═══════════════════════════════════════════════════════════════════════

/// (filename, description, expected_event_count, expected_tool_use_count)
const FIXTURES: &[(&str, &str, usize, usize)] = &[
    ("real_simple_read.jsonl", "simple read_file", 6, 1),
    ("real_tool_error.jsonl", "read nonexistent file", 6, 1),
    ("real_write_read_chain.jsonl", "write then read", 8, 2),
    ("real_search_bash.jsonl", "search + terminal", 8, 2),
    ("real_delegate.jsonl", "delegate_task subagent", 6, 1),
    ("real_code_review_patch.jsonl", "read + patch", 8, 2),
    ("real_execute_code.jsonl", "terminal execution", 6, 1),
    ("real_complex_refactor.jsonl", "7-tool refactor", 18, 7),
];

macro_rules! parameterized_fixture_test {
    ($test_name:ident, $assertion_fn:ident) => {
        #[test]
        fn $test_name() {
            for (filename, desc, expected_events, expected_tools) in FIXTURES {
                let (events, raw_lines) = load_and_translate(filename);
                $assertion_fn(
                    filename,
                    desc,
                    *expected_events,
                    *expected_tools,
                    &events,
                    &raw_lines,
                );
            }
        }
    };
}

fn assert_all_lines_detected_as_hermes(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    _events: &[CloudEvent],
    raw_lines: &[Value],
) {
    for (i, line) in raw_lines.iter().enumerate() {
        assert!(
            is_hermes_format(line),
            "[{}] line {} not detected as Hermes format",
            filename,
            i
        );
    }
}

fn assert_all_events_carry_hermes_agent(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(
            ev.agent.as_deref(),
            Some("hermes"),
            "[{}] event {} missing hermes agent tag",
            filename,
            i
        );
    }
}

fn assert_all_events_have_hermes_variant(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    for (i, ev) in events.iter().enumerate() {
        match &ev.data.agent_payload {
            Some(AgentPayload::Hermes(_)) => {} // correct
            other => panic!(
                "[{}] event {} expected Hermes variant (_variant: hermes), got {:?}",
                filename, i, other
            ),
        }
    }
}

fn assert_event_ids_are_unique(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "[{}] duplicate event IDs: {:?}",
        filename,
        ids
    );
}

fn assert_event_ids_are_deterministic(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    _events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    // Translate twice independently and compare IDs
    let (events1, _) = load_and_translate(filename);
    let (events2, _) = load_and_translate(filename);
    let ids1: Vec<&str> = events1.iter().map(|e| e.id.as_str()).collect();
    let ids2: Vec<&str> = events2.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids1, ids2,
        "[{}] event IDs must be stable across translation passes",
        filename
    );
}

fn assert_subtypes_bookend_correctly(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    assert!(
        !subtypes.is_empty(),
        "[{}] no subtypes found",
        filename
    );
    assert_eq!(
        subtypes[0], "system.session.start",
        "[{}] first subtype should be system.session.start, got {}",
        filename, subtypes[0]
    );
    assert_eq!(
        *subtypes.last().unwrap(),
        "system.turn.complete",
        "[{}] last subtype should be system.turn.complete, got {}",
        filename,
        subtypes.last().unwrap()
    );
}

fn assert_tool_use_events_have_tool_name(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    for (i, ev) in events.iter().enumerate() {
        if ev.subtype.as_deref() == Some("message.assistant.tool_use") {
            let p = hermes_payload(ev);
            assert!(
                p.tool.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
                "[{}] tool_use event {} has empty tool name",
                filename,
                i
            );
        }
    }
}

fn assert_tool_result_events_have_call_id(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    for (i, ev) in events.iter().enumerate() {
        if ev.subtype.as_deref() == Some("message.user.tool_result") {
            let p = hermes_payload(ev);
            assert!(
                p.tool_call_id
                    .as_ref()
                    .map(|id| !id.is_empty())
                    .unwrap_or(false),
                "[{}] tool_result event {} has empty tool_call_id",
                filename,
                i
            );
        }
    }
}

fn assert_tool_use_result_linkage(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    // Collect all tool_use_ids from tool_use events
    let tool_use_ids: HashSet<String> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .filter_map(|e| hermes_payload(e).tool_use_id.clone())
        .collect();

    // Every tool_result's tool_call_id should match a tool_use_id
    for (i, ev) in events.iter().enumerate() {
        if ev.subtype.as_deref() == Some("message.user.tool_result") {
            let p = hermes_payload(ev);
            if let Some(ref call_id) = p.tool_call_id {
                assert!(
                    tool_use_ids.contains(call_id),
                    "[{}] tool_result event {} has tool_call_id '{}' that doesn't match any tool_use_id. Available: {:?}",
                    filename,
                    i,
                    call_id,
                    tool_use_ids
                );
            }
        }
    }
}

fn assert_raw_field_preserved(
    filename: &str,
    _desc: &str,
    _expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    // Every event's data.raw should be a non-null JSON value (the original input line)
    for (i, ev) in events.iter().enumerate() {
        assert!(
            !ev.data.raw.is_null(),
            "[{}] event {} has null raw field",
            filename,
            i
        );
        // Raw should be an object with envelope + event_type (the plugin wire format)
        assert!(
            ev.data.raw.get("envelope").is_some(),
            "[{}] event {} raw field missing envelope",
            filename,
            i
        );
    }
}

fn assert_expected_event_count(
    filename: &str,
    desc: &str,
    expected_events: usize,
    _expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    assert_eq!(
        events.len(),
        expected_events,
        "[{} — {}] expected {} events, got {}",
        filename,
        desc,
        expected_events,
        events.len()
    );
}

fn assert_expected_tool_count(
    filename: &str,
    desc: &str,
    _expected_events: usize,
    expected_tools: usize,
    events: &[CloudEvent],
    _raw_lines: &[Value],
) {
    let tool_count = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .count();
    assert_eq!(
        tool_count, expected_tools,
        "[{} — {}] expected {} tool_use events, got {}",
        filename, desc, expected_tools, tool_count
    );
}

// Generate one #[test] per assertion, each parameterized over all fixtures.
parameterized_fixture_test!(all_fixtures_detected_as_hermes_format, assert_all_lines_detected_as_hermes);
parameterized_fixture_test!(all_fixtures_carry_hermes_agent_tag, assert_all_events_carry_hermes_agent);
parameterized_fixture_test!(all_fixtures_have_hermes_variant, assert_all_events_have_hermes_variant);
parameterized_fixture_test!(all_fixtures_event_ids_unique, assert_event_ids_are_unique);
parameterized_fixture_test!(all_fixtures_event_ids_deterministic, assert_event_ids_are_deterministic);
parameterized_fixture_test!(all_fixtures_subtypes_bookend, assert_subtypes_bookend_correctly);
parameterized_fixture_test!(all_fixtures_tool_use_has_name, assert_tool_use_events_have_tool_name);
parameterized_fixture_test!(all_fixtures_tool_result_has_call_id, assert_tool_result_events_have_call_id);
parameterized_fixture_test!(all_fixtures_tool_use_result_linked, assert_tool_use_result_linkage);
parameterized_fixture_test!(all_fixtures_raw_preserved, assert_raw_field_preserved);
parameterized_fixture_test!(all_fixtures_expected_event_count, assert_expected_event_count);
parameterized_fixture_test!(all_fixtures_expected_tool_count, assert_expected_tool_count);

// ═══════════════════════════════════════════════════════════════════════
// 2. Parameterized message type tests
// ═══════════════════════════════════════════════════════════════════════

/// (role_json, expected_subtype, description)
const MESSAGE_TYPES: &[(&str, &str, &str)] = &[
    (
        r#"{"role":"user","content":"hello"}"#,
        "message.user.prompt",
        "user prompt",
    ),
    (
        r#"{"role":"assistant","content":"hi","finish_reason":"stop"}"#,
        "message.assistant.text",
        "assistant text",
    ),
    (
        r#"{"role":"tool","tool_call_id":"tc_1","content":"result"}"#,
        "message.user.tool_result",
        "tool result",
    ),
    (
        r#"{"role":"system","content":"compressed history"}"#,
        "system.injected.other",
        "system injected",
    ),
];

#[test]
fn parameterized_message_types_produce_correct_subtypes() {
    let session_id = "msg-type-test-001";
    for (i, (role_json, expected_subtype, desc)) in MESSAGE_TYPES.iter().enumerate() {
        let msg: Value = serde_json::from_str(role_json)
            .unwrap_or_else(|e| panic!("bad JSON for {}: {}", desc, e));
        let wrapped = wrap_message(session_id, (i + 1) as u64, msg);

        assert!(
            is_hermes_format(&wrapped),
            "[{}] wrapped message not detected as Hermes",
            desc
        );

        let mut state = TranscriptState::new(session_id.to_string());
        let events = translate_hermes_line(&wrapped, &mut state);

        assert_eq!(
            events.len(),
            1,
            "[{}] expected 1 event, got {}",
            desc,
            events.len()
        );
        assert_eq!(
            events[0].subtype.as_deref(),
            Some(*expected_subtype),
            "[{}] expected subtype {}, got {:?}",
            desc,
            expected_subtype,
            events[0].subtype
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Parameterized tool type tests
// ═══════════════════════════════════════════════════════════════════════

/// (tool_name, arguments_json)
const TOOL_TYPES: &[(&str, &str)] = &[
    ("read_file", r#"{"path":"README.md"}"#),
    ("write_file", r#"{"path":"test.txt","content":"hello"}"#),
    ("terminal", r#"{"command":"ls -la"}"#),
    ("search_files", r#"{"pattern":"*.py","target":"files"}"#),
    ("patch", r#"{"path":"main.py","old_string":"a","new_string":"b"}"#),
    (
        "delegate_task",
        r#"{"tasks":[{"goal":"search"}]}"#,
    ),
];

#[test]
fn parameterized_tool_types_produce_correct_payloads() {
    let session_id = "tool-type-test-001";
    for (i, (tool_name, args_json)) in TOOL_TYPES.iter().enumerate() {
        let msg = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": format!("tc_{}", tool_name),
                "function": {
                    "name": tool_name,
                    "arguments": args_json,
                }
            }],
            "finish_reason": "tool_calls",
        });
        let wrapped = wrap_message(session_id, (i + 10) as u64, msg);
        let mut state = TranscriptState::new(session_id.to_string());
        let events = translate_hermes_line(&wrapped, &mut state);

        assert_eq!(
            events.len(),
            1,
            "[{}] expected 1 event for tool {}, got {}",
            tool_name,
            tool_name,
            events.len()
        );

        let ev = &events[0];
        assert_eq!(
            ev.subtype.as_deref(),
            Some("message.assistant.tool_use"),
            "[{}] wrong subtype",
            tool_name
        );

        let p = hermes_payload(ev);

        // Tool name matches
        assert_eq!(
            p.tool.as_deref(),
            Some(*tool_name),
            "[{}] tool field mismatch",
            tool_name
        );

        // Args are parsed (not a raw JSON string — should be a structured Value)
        let args = p.args.as_ref().expect("args should be present");
        assert!(
            args.is_object(),
            "[{}] args should be parsed into an object, got: {}",
            tool_name,
            args
        );

        // tool_use_id is non-empty
        assert!(
            p.tool_use_id
                .as_ref()
                .map(|id| !id.is_empty())
                .unwrap_or(false),
            "[{}] tool_use_id should be non-empty",
            tool_name
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Snapshot watcher pipeline test (process_snapshot → ViewRecords)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_pipeline_produces_correct_view_records() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session_pipeline001.json");

    let snapshot = make_snapshot(
        "pipeline001",
        vec![
            json!({"role": "user", "content": "read the file"}),
            json!({
                "role": "assistant",
                "content": "I'll read it.",
                "tool_calls": [{
                    "id": "tc_read_1",
                    "function": {"name": "read_file", "arguments": r#"{"path":"test.txt"}"#}
                }],
                "finish_reason": "tool_calls",
            }),
            json!({"role": "tool", "tool_call_id": "tc_read_1", "content": "file contents here"}),
            json!({"role": "assistant", "content": "The file says: file contents here", "finish_reason": "stop"}),
        ],
    );
    std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

    let mut states = HashMap::new();
    let events = process_snapshot(&path, &mut states).unwrap();

    assert!(!events.is_empty(), "should produce CloudEvents");

    // Feed CloudEvents through the views layer
    let view_records: Vec<_> = events
        .iter()
        .flat_map(|ev| from_cloud_event(ev))
        .collect();

    assert!(
        !view_records.is_empty(),
        "should produce ViewRecords from CloudEvents"
    );

    // Check we have the expected record types
    let mut has_user_message = false;
    let mut has_tool_call = false;
    let mut has_tool_result = false;
    let mut has_assistant_message = false;

    for vr in &view_records {
        match &vr.body {
            RecordBody::UserMessage(_) => has_user_message = true,
            RecordBody::ToolCall(tc) => {
                has_tool_call = true;
                // ToolCall should have call_id and name
                assert!(
                    !tc.call_id.is_empty(),
                    "ToolCall should have non-empty call_id"
                );
                assert!(
                    !tc.name.is_empty(),
                    "ToolCall should have non-empty name"
                );
                assert_eq!(tc.name, "read_file", "tool name should be read_file");
            }
            RecordBody::ToolResult(tr) => {
                has_tool_result = true;
                // ToolResult should have call_id linking back to the ToolCall
                assert!(
                    !tr.call_id.is_empty(),
                    "ToolResult should have non-empty call_id"
                );
                assert_eq!(
                    tr.call_id, "tc_read_1",
                    "ToolResult call_id should link back to tool_call"
                );
            }
            RecordBody::AssistantMessage(_) => has_assistant_message = true,
            _ => {} // session meta, turn end, etc.
        }
    }

    assert!(has_user_message, "should have UserMessage ViewRecord");
    assert!(has_tool_call, "should have ToolCall ViewRecord");
    assert!(has_tool_result, "should have ToolResult ViewRecord");
    assert!(has_assistant_message, "should have AssistantMessage ViewRecord");
}

#[test]
fn snapshot_pipeline_tool_call_result_linkage() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("session_linkage001.json");

    let snapshot = make_snapshot(
        "linkage001",
        vec![
            json!({"role": "user", "content": "do two things"}),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "tc_a", "function": {"name": "read_file", "arguments": r#"{"path":"a.txt"}"#}},
                    {"id": "tc_b", "function": {"name": "write_file", "arguments": r#"{"path":"b.txt","content":"x"}"#}},
                ],
                "finish_reason": "tool_calls",
            }),
            json!({"role": "tool", "tool_call_id": "tc_a", "content": "contents of a"}),
            json!({"role": "tool", "tool_call_id": "tc_b", "content": "written"}),
            json!({"role": "assistant", "content": "Done.", "finish_reason": "stop"}),
        ],
    );
    std::fs::write(&path, serde_json::to_string(&snapshot).unwrap()).unwrap();

    let mut states = HashMap::new();
    let events = process_snapshot(&path, &mut states).unwrap();

    let view_records: Vec<_> = events
        .iter()
        .flat_map(|ev| from_cloud_event(ev))
        .collect();

    // Collect call_ids from ToolCall records
    let tool_call_ids: HashSet<String> = view_records
        .iter()
        .filter_map(|vr| match &vr.body {
            RecordBody::ToolCall(tc) => Some(tc.call_id.clone()),
            _ => None,
        })
        .collect();

    // Every ToolResult call_id should match a ToolCall
    for vr in &view_records {
        if let RecordBody::ToolResult(tr) = &vr.body {
            assert!(
                tool_call_ids.contains(&tr.call_id),
                "ToolResult call_id '{}' should link to a ToolCall. Available: {:?}",
                tr.call_id,
                tool_call_ids
            );
        }
    }

    assert_eq!(
        tool_call_ids.len(),
        2,
        "should have 2 unique ToolCall records"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Multi-tool ID uniqueness test (B2 fix validation)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multi_tool_call_produces_unique_event_ids_b2() {
    let session_id = "b2-test-001";
    let msg = json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [
            {"id": "tc_1", "function": {"name": "read_file", "arguments": r#"{"path":"a.txt"}"#}},
            {"id": "tc_2", "function": {"name": "write_file", "arguments": r#"{"path":"b.txt","content":"x"}"#}},
            {"id": "tc_3", "function": {"name": "terminal", "arguments": r#"{"command":"ls"}"#}},
            {"id": "tc_4", "function": {"name": "search_files", "arguments": r#"{"pattern":"*.py"}"#}},
            {"id": "tc_5", "function": {"name": "patch", "arguments": r#"{"path":"c.py","old_string":"a","new_string":"b"}"#}},
        ],
        "finish_reason": "tool_calls",
    });
    let wrapped = wrap_message(session_id, 1, msg);
    let mut state = TranscriptState::new(session_id.to_string());
    let events = translate_hermes_line(&wrapped, &mut state);

    // Should produce exactly 5 CloudEvents
    assert_eq!(events.len(), 5, "5 tool_calls should produce 5 events");

    // All 5 should be tool_use
    for ev in &events {
        assert_eq!(
            ev.subtype.as_deref(),
            Some("message.assistant.tool_use"),
            "all events should be tool_use"
        );
    }

    // All 5 should have different event IDs (B2 fix)
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        5,
        "all 5 tool_use events must have unique IDs. Got: {:?}",
        ids
    );

    // Each should have a different tool name
    let tool_names: Vec<String> = events
        .iter()
        .map(|e| hermes_payload(e).tool.clone().unwrap_or_default())
        .collect();
    let expected_tools = vec![
        "read_file",
        "write_file",
        "terminal",
        "search_files",
        "patch",
    ];
    assert_eq!(
        tool_names, expected_tools,
        "tool names should match in order"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Stop reason preservation test (C2 fix validation)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stop_reason_preserves_tool_calls_not_tool_use_c2() {
    let session_id = "c2-test-001";
    let msg = json!({
        "role": "assistant",
        "content": "Let me read that.",
        "tool_calls": [{
            "id": "tc_1",
            "function": {"name": "read_file", "arguments": r#"{"path":"x.txt"}"#}
        }],
        "finish_reason": "tool_calls",
    });
    let wrapped = wrap_message(session_id, 1, msg);
    let mut state = TranscriptState::new(session_id.to_string());
    let events = translate_hermes_line(&wrapped, &mut state);

    // Find the tool_use event
    let tool_use = events
        .iter()
        .find(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .expect("should have a tool_use event");

    let p = hermes_payload(tool_use);
    assert_eq!(
        p.stop_reason.as_deref(),
        Some("tool_calls"),
        "stop_reason should be 'tool_calls' (Hermes native), not 'tool_use' (Anthropic convention)"
    );
}

#[test]
fn stop_reason_preserves_stop_for_text() {
    let session_id = "c2-text-test-001";
    let msg = json!({
        "role": "assistant",
        "content": "Here is the answer.",
        "finish_reason": "stop",
    });
    let wrapped = wrap_message(session_id, 1, msg);
    let mut state = TranscriptState::new(session_id.to_string());
    let events = translate_hermes_line(&wrapped, &mut state);

    let text_event = events
        .iter()
        .find(|e| e.subtype.as_deref() == Some("message.assistant.text"))
        .expect("should have a text event");

    let p = hermes_payload(text_event);
    assert_eq!(
        p.stop_reason.as_deref(),
        Some("stop"),
        "stop_reason should be 'stop' for text-only assistant messages"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Raw preservation test (C1 validation)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn raw_field_preserves_original_input_c1() {
    let session_id = "c1-test-001";

    let messages: Vec<(&str, Value)> = vec![
        (
            "user prompt",
            json!({"role": "user", "content": "hello world"}),
        ),
        (
            "assistant text",
            json!({"role": "assistant", "content": "hi there", "finish_reason": "stop"}),
        ),
        (
            "tool result",
            json!({"role": "tool", "tool_call_id": "tc_1", "content": "result data"}),
        ),
        (
            "system injected",
            json!({"role": "system", "content": "compressed history"}),
        ),
    ];

    for (i, (desc, msg)) in messages.iter().enumerate() {
        let wrapped = wrap_message(session_id, (i + 1) as u64, msg.clone());
        let mut state = TranscriptState::new(session_id.to_string());
        let events = translate_hermes_line(&wrapped, &mut state);

        assert!(
            !events.is_empty(),
            "[{}] should produce at least one event",
            desc
        );

        for ev in &events {
            // event.data.raw should equal the original wrapped input (byte-for-byte)
            assert_eq!(
                ev.data.raw, wrapped,
                "[{}] raw field should preserve the original plugin envelope",
                desc
            );
        }
    }
}

#[test]
fn raw_field_preserved_for_tool_use_with_fan_out() {
    // When an assistant message fans out to multiple tool_use events,
    // ALL of them should have the same raw (the original wrapped line).
    let session_id = "c1-fanout-001";
    let msg = json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [
            {"id": "tc_a", "function": {"name": "read_file", "arguments": r#"{"path":"a.txt"}"#}},
            {"id": "tc_b", "function": {"name": "terminal", "arguments": r#"{"command":"ls"}"#}},
        ],
        "finish_reason": "tool_calls",
    });
    let wrapped = wrap_message(session_id, 1, msg);
    let mut state = TranscriptState::new(session_id.to_string());
    let events = translate_hermes_line(&wrapped, &mut state);

    assert_eq!(events.len(), 2, "should fan out to 2 events");
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(
            ev.data.raw, wrapped,
            "fan-out event {} should preserve the original wrapped line",
            i
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Real fixture through full views pipeline
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn real_fixtures_produce_valid_view_records() {
    for (filename, desc, _expected_events, _expected_tools) in FIXTURES {
        let (events, _) = load_and_translate(filename);

        let view_records: Vec<_> = events
            .iter()
            .flat_map(|ev| from_cloud_event(ev))
            .collect();

        assert!(
            !view_records.is_empty(),
            "[{} — {}] should produce ViewRecords",
            filename,
            desc
        );

        // All ViewRecords should have non-empty IDs and session_ids
        for (i, vr) in view_records.iter().enumerate() {
            assert!(
                !vr.id.is_empty(),
                "[{} — {}] ViewRecord {} has empty id",
                filename,
                desc,
                i
            );
            assert!(
                !vr.session_id.is_empty(),
                "[{} — {}] ViewRecord {} has empty session_id",
                filename,
                desc,
                i
            );
        }

        // ToolCall records should have call_id and name
        for vr in &view_records {
            if let RecordBody::ToolCall(tc) = &vr.body {
                assert!(
                    !tc.call_id.is_empty(),
                    "[{} — {}] ToolCall has empty call_id",
                    filename,
                    desc
                );
                assert!(
                    !tc.name.is_empty(),
                    "[{} — {}] ToolCall has empty name",
                    filename,
                    desc
                );
            }
        }

        // ToolResult records should have call_id
        for vr in &view_records {
            if let RecordBody::ToolResult(tr) = &vr.body {
                assert!(
                    !tr.call_id.is_empty(),
                    "[{} — {}] ToolResult has empty call_id",
                    filename,
                    desc
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Diff + translate incremental test (snapshot_watcher correctness)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diff_snapshot_incremental_produces_only_new_events() {
    let mut state = SnapshotState::new("incr-001".to_string());

    // First snapshot: 2 messages
    let snap1 = make_snapshot(
        "incr-001",
        vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "hi", "finish_reason": "stop"}),
        ],
    );
    let new1 = diff_snapshot(&mut state, &snap1);
    assert_eq!(new1.len(), 2, "first read should emit all messages");

    // Second snapshot: 4 messages (2 new)
    let snap2 = make_snapshot(
        "incr-001",
        vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "hi", "finish_reason": "stop"}),
            json!({"role": "user", "content": "read a file"}),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{"id": "tc_1", "function": {"name": "read_file", "arguments": r#"{"path":"x.txt"}"#}}],
                "finish_reason": "tool_calls",
            }),
        ],
    );
    let new2 = diff_snapshot(&mut state, &snap2);
    assert_eq!(new2.len(), 2, "second read should emit only 2 new messages");
    assert_eq!(new2[0]["role"], "user");
    assert_eq!(new2[1]["role"], "assistant");
}

#[test]
fn diff_snapshot_compression_resets_state() {
    let mut state = SnapshotState::new("comp-001".to_string());
    state.message_count = 10;

    // New session_id (compression)
    let snap = make_snapshot(
        "comp-002",
        vec![
            json!({"role": "system", "content": "[Compressed]"}),
            json!({"role": "user", "content": "continue"}),
        ],
    );
    let new = diff_snapshot(&mut state, &snap);
    assert_eq!(new.len(), 2, "compression should emit ALL messages");
    assert_eq!(state.session_id, "comp-002");
    assert_eq!(state.message_count, 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Assistant message with reasoning produces thinking event
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn assistant_with_reasoning_produces_thinking_event() {
    let session_id = "thinking-test-001";
    let msg = json!({
        "role": "assistant",
        "content": "I'll read the file.",
        "reasoning": "The user wants me to read a file. I should use read_file.",
        "tool_calls": [{
            "id": "tc_1",
            "function": {"name": "read_file", "arguments": r#"{"path":"x.txt"}"#}
        }],
        "finish_reason": "tool_calls",
    });
    let wrapped = wrap_message(session_id, 1, msg);
    let mut state = TranscriptState::new(session_id.to_string());
    let events = translate_hermes_line(&wrapped, &mut state);

    // Should produce: thinking + tool_use
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    assert!(
        subtypes.contains(&"message.assistant.thinking"),
        "should produce thinking event when reasoning is present"
    );
    assert!(
        subtypes.contains(&"message.assistant.tool_use"),
        "should still produce tool_use event"
    );

    let thinking = events
        .iter()
        .find(|e| e.subtype.as_deref() == Some("message.assistant.thinking"))
        .unwrap();
    let p = hermes_payload(thinking);
    assert!(
        p.reasoning.as_deref().unwrap().contains("read a file"),
        "thinking event should preserve reasoning content"
    );
}
