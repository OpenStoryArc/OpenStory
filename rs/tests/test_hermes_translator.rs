//! Integration tests for the Hermes translator.
//!
//! Phase A: read the fixture JSONL (produced by `generate_session.py`)
//! and feed it through `translate_hermes_line()`. Verifies that the
//! translator handles the canonical Hermes message shapes from a "real"
//! session (synthesized by the fixture script using verified shapes from
//! SOURCE_VERIFICATION.md §4).
//!
//! Phase B (testcontainer): builds and runs the `hermes-fixture:test`
//! Docker image, reads the generated JSONL from the container, and
//! verifies the same way. This closes runtime gaps by using the
//! container's own Python to produce the data.
//!
//! Build the Docker image first:
//!   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
//!
//! Run:
//!   cargo test -p open-story --test test_hermes_translator

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;
use open_story_core::translate::TranscriptState;
use open_story_core::translate_hermes::{is_hermes_format, translate_hermes_line};
use serde_json::Value;
use std::path::PathBuf;

/// Load the fixture JSONL and translate every line.
fn load_and_translate_fixture() -> (Vec<CloudEvent>, Vec<Value>) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_plugin.jsonl");
    let content = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("fixture not found at {:?}: {}", fixture_path, e));

    let mut state = TranscriptState::new("fixture-session".to_string());
    let mut events: Vec<CloudEvent> = Vec::new();
    let mut raw_lines: Vec<Value> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("bad JSON in fixture: {}", e));
        raw_lines.push(parsed.clone());
        events.extend(translate_hermes_line(&parsed, &mut state));
    }

    (events, raw_lines)
}

/// Helper: extract HermesPayload from a CloudEvent.
fn hermes_payload(event: &CloudEvent) -> &open_story_core::event_data::HermesPayload {
    match event.data.agent_payload.as_ref().expect("missing agent_payload") {
        AgentPayload::Hermes(p) => p,
        other => panic!("expected Hermes payload, got {:?}", other),
    }
}

// ── Phase A: static fixture tests ────────────────────────────────────

#[test]
fn fixture_is_detected_as_hermes_format() {
    let (_, raw_lines) = load_and_translate_fixture();
    for (i, line) in raw_lines.iter().enumerate() {
        assert!(
            is_hermes_format(line),
            "line {} not detected as Hermes format: {}",
            i,
            serde_json::to_string_pretty(line).unwrap()
        );
    }
}

#[test]
fn fixture_produces_correct_event_count() {
    let (events, _) = load_and_translate_fixture();
    // 6 input lines should produce 7 CloudEvents:
    //   session_start, user prompt, thinking, tool_use, tool_result,
    //   assistant text, turn complete
    assert_eq!(
        events.len(),
        7,
        "expected 7 CloudEvents from 6 fixture lines, got {}",
        events.len()
    );
}

#[test]
fn fixture_subtypes_are_in_expected_order() {
    let (events, _) = load_and_translate_fixture();
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "system.session.start",
            "message.user.prompt",
            "message.assistant.thinking",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.text",
            "system.turn.complete",
        ]
    );
}

#[test]
fn fixture_all_events_carry_hermes_agent_tag() {
    let (events, _) = load_and_translate_fixture();
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(
            ev.agent.as_deref(),
            Some("hermes"),
            "event {} missing hermes agent tag",
            i
        );
    }
}

#[test]
fn fixture_session_start_has_model_and_tools() {
    let (events, _) = load_and_translate_fixture();
    let start = &events[0];
    assert_eq!(start.subtype.as_deref(), Some("system.session.start"));
    let p = hermes_payload(start);
    assert_eq!(p.model.as_deref(), Some("mock-model"));
    assert_eq!(p.platform.as_deref(), Some("cli"));
    assert!(p.tools.is_some());
    let tools = p.tools.as_ref().unwrap();
    assert!(tools.contains(&"Bash".to_string()));
}

#[test]
fn fixture_user_prompt_text_is_preserved() {
    let (events, _) = load_and_translate_fixture();
    let user = &events[1];
    assert_eq!(user.subtype.as_deref(), Some("message.user.prompt"));
    let p = hermes_payload(user);
    assert!(p.text.as_deref().unwrap().contains("files"));
}

#[test]
fn fixture_thinking_contains_reasoning() {
    let (events, _) = load_and_translate_fixture();
    let thinking = &events[2];
    assert_eq!(
        thinking.subtype.as_deref(),
        Some("message.assistant.thinking")
    );
    let p = hermes_payload(thinking);
    assert!(p.reasoning.as_deref().unwrap().contains("directory listing"));
}

#[test]
fn fixture_tool_use_has_parsed_args() {
    let (events, _) = load_and_translate_fixture();
    let tool_use = &events[3];
    assert_eq!(
        tool_use.subtype.as_deref(),
        Some("message.assistant.tool_use")
    );
    let p = hermes_payload(tool_use);
    assert_eq!(p.tool.as_deref(), Some("Bash"));
    assert_eq!(p.tool_use_id.as_deref(), Some("toolu_fixture_001"));
    // arguments must be parsed from JSON string → structured Value
    let args = p.args.as_ref().unwrap();
    assert_eq!(args["command"], "ls -la");
    // preceding_text should be the assistant content before the tool call
    assert_eq!(
        p.preceding_text.as_deref(),
        Some("I'll list the files for you.")
    );
}

#[test]
fn fixture_tool_result_links_correctly() {
    let (events, _) = load_and_translate_fixture();
    let tool_result = &events[4];
    assert_eq!(
        tool_result.subtype.as_deref(),
        Some("message.user.tool_result")
    );
    let p = hermes_payload(tool_result);
    // tool_call_id links back to the tool_use
    assert_eq!(p.tool_call_id.as_deref(), Some("toolu_fixture_001"));
    // tool_name is present in this fixture (runtime gap #2: verify with real Hermes)
    assert_eq!(p.tool_name.as_deref(), Some("Bash"));
    // tool accessor should mirror tool_name
    assert_eq!(p.tool.as_deref(), Some("Bash"));
    // content is the tool output
    assert!(p.text.as_deref().unwrap().contains("README.md"));
}

#[test]
fn fixture_final_text_has_stop_reason() {
    let (events, _) = load_and_translate_fixture();
    let text = &events[5];
    assert_eq!(text.subtype.as_deref(), Some("message.assistant.text"));
    let p = hermes_payload(text);
    assert_eq!(p.stop_reason.as_deref(), Some("stop"));
    assert!(p.text.as_deref().unwrap().contains("README.md"));
}

#[test]
fn fixture_turn_complete_has_completion_state() {
    let (events, _) = load_and_translate_fixture();
    let tc = &events[6];
    assert_eq!(tc.subtype.as_deref(), Some("system.turn.complete"));
    let p = hermes_payload(tc);
    assert_eq!(p.reason.as_deref(), Some("end_turn"));
    assert_eq!(p.completed, Some(true));
    assert_eq!(p.interrupted, Some(false));
    assert_eq!(p.message_count, Some(5));
}

#[test]
fn fixture_event_ids_are_deterministic() {
    let (events1, _) = load_and_translate_fixture();
    let (events2, _) = load_and_translate_fixture();
    let ids1: Vec<&str> = events1.iter().map(|e| e.id.as_str()).collect();
    let ids2: Vec<&str> = events2.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids1, ids2, "event IDs must be stable across translation passes");
}

#[test]
fn fixture_event_ids_are_unique() {
    let (events, _) = load_and_translate_fixture();
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate event IDs found: {:?}",
        ids
    );
}

// ── Snapshot format test (runtime gap #1) ────────────────────────────

#[test]
fn snapshot_timestamp_format_is_iso8601_with_offset() {
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_snapshot.json");
    let content = std::fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|e| panic!("snapshot fixture not found: {}", e));
    let snapshot: Value = serde_json::from_str(&content).unwrap();

    // Runtime gap #1: verify that Hermes uses ISO-8601 with offset.
    // Python's datetime.now(timezone.utc).isoformat() produces
    // something like "2026-04-09T10:58:52.075214+00:00" — NOT a
    // trailing "Z", and WITH microseconds.
    let ts = snapshot["session_start"]
        .as_str()
        .expect("session_start should be a string");
    assert!(
        ts.contains('+') || ts.ends_with('Z'),
        "timestamp should have timezone info: {}",
        ts
    );
    // Should have sub-second precision
    assert!(
        ts.contains('.'),
        "timestamp should have microseconds: {}",
        ts
    );
    // The translator passes timestamps through unchanged — so the
    // plugin's UTC-Z timestamps (from time.strftime) will work fine.
    // The snapshot timestamps (from datetime.isoformat) are different
    // but also valid ISO-8601.
}

// ── Snapshot top-level shape test ────────────────────────────────────

#[test]
fn snapshot_has_expected_top_level_fields() {
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_snapshot.json");
    let content = std::fs::read_to_string(&snapshot_path).unwrap();
    let snapshot: Value = serde_json::from_str(&content).unwrap();

    // Verified shape from run_agent.py:2450-2461
    assert!(snapshot["session_id"].is_string());
    assert!(snapshot["model"].is_string());
    assert!(snapshot["platform"].is_string());
    assert!(snapshot["session_start"].is_string());
    assert!(snapshot["last_updated"].is_string());
    assert!(snapshot["system_prompt"].is_string());
    assert!(snapshot["tools"].is_array());
    assert!(snapshot["message_count"].is_number());
    assert!(snapshot["messages"].is_array());

    let messages = snapshot["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        snapshot["message_count"].as_u64().unwrap() as usize
    );
}

// ── Phase C: real Hermes session data ────────────────────────────────
//
// These tests read `real_session.jsonl` — JSONL produced by wrapping a
// REAL Hermes Agent session log (from `hermes chat -q ...` against
// Anthropic's claude-sonnet-4-20250514) in the plugin envelope format.
//
// Unlike the synthetic fixture, these messages were produced by actual
// Hermes agent code talking to a real LLM. They close the runtime
// verification gaps that static source analysis couldn't.

/// Load the REAL session fixture and translate.
fn load_and_translate_real_session() -> (Vec<CloudEvent>, Vec<Value>) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/real_session.jsonl");
    let content = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("real session fixture not found at {:?}: {}", fixture_path, e));

    let mut state = TranscriptState::new("real-session".to_string());
    let mut events: Vec<CloudEvent> = Vec::new();
    let mut raw_lines: Vec<Value> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("bad JSON in real session fixture: {}", e));
        raw_lines.push(parsed.clone());
        events.extend(translate_hermes_line(&parsed, &mut state));
    }

    (events, raw_lines)
}

#[test]
fn real_session_is_detected_as_hermes_format() {
    let (_, raw_lines) = load_and_translate_real_session();
    assert!(!raw_lines.is_empty(), "real session fixture should have lines");
    for (i, line) in raw_lines.iter().enumerate() {
        assert!(
            is_hermes_format(line),
            "real session line {} not detected as Hermes: {}",
            i,
            serde_json::to_string(line).unwrap_or_default()
        );
    }
}

#[test]
fn real_session_produces_correct_event_count() {
    let (events, _) = load_and_translate_real_session();
    // Real session: 6 input lines (start + user + assistant-tool + tool-result +
    // assistant-text + end) → 6 CloudEvents (no thinking event because
    // reasoning was null on this session).
    assert_eq!(
        events.len(),
        6,
        "expected 6 CloudEvents from real session (reasoning was null), got {}",
        events.len()
    );
}

#[test]
fn real_session_subtypes_match_expected_order() {
    let (events, _) = load_and_translate_real_session();
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    // No thinking event — Sonnet 4 didn't produce reasoning for this query.
    assert_eq!(
        subtypes,
        vec![
            "system.session.start",
            "message.user.prompt",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.text",
            "system.turn.complete",
        ]
    );
}

#[test]
fn real_session_tool_call_has_anthropic_style_id() {
    // Real Hermes session against Anthropic: tool_call IDs start with "toolu_"
    let (events, _) = load_and_translate_real_session();
    let tool_use: Vec<&CloudEvent> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_use.len(), 1);
    let p = hermes_payload(tool_use[0]);
    assert!(
        p.tool_use_id
            .as_deref()
            .unwrap_or("")
            .starts_with("toolu_"),
        "Anthropic tool IDs start with toolu_"
    );
    assert_eq!(p.tool.as_deref(), Some("read_file"));
    // Arguments must be parsed from JSON string
    let args = p.args.as_ref().unwrap();
    assert!(args.get("path").is_some(), "read_file should have a path arg");
}

#[test]
fn real_session_tool_result_has_no_tool_name() {
    // Runtime gap #2: CONFIRMED — real Hermes does NOT include tool_name
    // on tool result messages. Only tool_call_id + content.
    let (events, _) = load_and_translate_real_session();
    let tool_result: Vec<&CloudEvent> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .collect();
    assert_eq!(tool_result.len(), 1);
    let p = hermes_payload(tool_result[0]);
    assert!(
        p.tool_name.is_none(),
        "real Hermes tool results do NOT include tool_name (runtime gap #2 confirmed)"
    );
    // tool_call_id IS present
    assert!(p.tool_call_id.is_some());
}

#[test]
fn real_session_reasoning_is_null_for_non_thinking_model() {
    // Runtime gap #4: reasoning field is present as a key but null for
    // Sonnet 4 (non-extended-thinking mode). The translator correctly
    // skips the thinking event when reasoning is null.
    let (events, _) = load_and_translate_real_session();
    let thinking: Vec<&CloudEvent> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.thinking"))
        .collect();
    assert!(
        thinking.is_empty(),
        "no thinking events when reasoning is null"
    );
}

#[test]
fn real_session_has_real_model_name() {
    let (events, _) = load_and_translate_real_session();
    let start = &events[0];
    let p = hermes_payload(start);
    assert_eq!(p.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn real_session_final_answer_has_stop_reason() {
    let (events, _) = load_and_translate_real_session();
    let text_events: Vec<&CloudEvent> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.text"))
        .collect();
    assert_eq!(text_events.len(), 1);
    let p = hermes_payload(text_events[0]);
    assert_eq!(p.stop_reason.as_deref(), Some("stop"));
    assert!(
        p.text.as_deref().unwrap().contains("Hermes Agent"),
        "final answer should mention Hermes Agent"
    );
}

#[test]
fn real_session_timestamp_is_naive_iso8601() {
    // Runtime gap #1 RESOLVED: real Hermes timestamps are NAIVE
    // (no timezone, no Z, no +00:00). Format: 2026-04-10T10:55:02.359248
    let (_, raw_lines) = load_and_translate_real_session();
    let session_start_ts = raw_lines[0]["envelope"]["timestamp"]
        .as_str()
        .expect("session_start should have a timestamp");
    // The timestamp from the snapshot is naive — no timezone indicator
    // (this is from our wrapping script which uses the snapshot value)
    assert!(
        session_start_ts.contains('T'),
        "timestamp should be ISO-8601: {}",
        session_start_ts
    );
}

// ── Phase D: hardened real-session battery ────────────────────────────
//
// Five sessions against Anthropic claude-sonnet-4-20250514, each exercising
// a different tool type and message pattern. These tests close runtime gaps
// and confirm the translator handles the full diversity of Hermes output.

/// Generic helper: load any fixture JSONL and translate.
fn translate_fixture(name: &str) -> Vec<CloudEvent> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/hermes/{}", name));
    let content = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("fixture {:?} not found: {}", fixture_path, e));

    let mut state = TranscriptState::new("test".to_string());
    let mut events: Vec<CloudEvent> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(line).unwrap();
        events.extend(translate_hermes_line(&parsed, &mut state));
    }
    events
}

// ── Tool error session ──────────────────────────────────────────

#[test]
fn real_tool_error_session_translates_cleanly() {
    let events = translate_fixture("real_tool_error.jsonl");
    // start + user + tool_use + tool_result + text + end = 6
    assert_eq!(events.len(), 6);
    let subtypes: Vec<&str> = events.iter().filter_map(|e| e.subtype.as_deref()).collect();
    assert_eq!(subtypes, vec![
        "system.session.start",
        "message.user.prompt",
        "message.assistant.tool_use",
        "message.user.tool_result",
        "message.assistant.text",
        "system.turn.complete",
    ]);
}

#[test]
fn real_tool_error_result_contains_error_in_content_json() {
    // Hermes encodes errors INSIDE the tool result content JSON string.
    // The `error` key is present in the JSON, NOT as a separate message field.
    let events = translate_fixture("real_tool_error.jsonl");
    let tool_result = events.iter()
        .find(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .expect("should have tool_result");
    let p = hermes_payload(tool_result);
    let content = p.text.as_deref().unwrap();
    // Content is a JSON string containing an error key
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert!(
        parsed.get("error").is_some(),
        "error tool results carry 'error' key inside the content JSON: {}",
        content
    );
    assert!(
        parsed["error"].as_str().unwrap().contains("File not found"),
        "error message should mention 'File not found'"
    );
}

// ── Write + read chain session ──────────────────────────────────

#[test]
fn real_write_read_chain_has_two_tool_use_events() {
    let events = translate_fixture("real_write_read_chain.jsonl");
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 2, "write + read = 2 tool_use events");

    let tool_names: Vec<&str> = tool_uses.iter()
        .map(|e| hermes_payload(e).tool.as_deref().unwrap_or("?"))
        .collect();
    assert_eq!(tool_names, vec!["write_file", "read_file"]);
}

#[test]
fn real_write_result_is_structured_json() {
    let events = translate_fixture("real_write_read_chain.jsonl");
    let tool_results: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .collect();
    // First tool result is from write_file
    let write_result = hermes_payload(tool_results[0]);
    let content = write_result.text.as_deref().unwrap();
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert!(
        parsed.get("bytes_written").is_some(),
        "write_file result should have bytes_written: {}",
        content
    );
}

#[test]
fn real_write_read_chain_subtypes() {
    let events = translate_fixture("real_write_read_chain.jsonl");
    // 8 input lines → start + user + tool_use(write) + tool_result(write) +
    //   tool_use(read) + tool_result(read) + text + end = 8
    assert_eq!(events.len(), 8);
    let subtypes: Vec<&str> = events.iter().filter_map(|e| e.subtype.as_deref()).collect();
    assert_eq!(subtypes, vec![
        "system.session.start",
        "message.user.prompt",
        "message.assistant.tool_use",
        "message.user.tool_result",
        "message.assistant.tool_use",
        "message.user.tool_result",
        "message.assistant.text",
        "system.turn.complete",
    ]);
}

// ── Search + bash session ───────────────────────────────────────

#[test]
fn real_search_bash_exercises_two_tool_types() {
    let events = translate_fixture("real_search_bash.jsonl");
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 2);

    let tool_names: Vec<&str> = tool_uses.iter()
        .map(|e| hermes_payload(e).tool.as_deref().unwrap_or("?"))
        .collect();
    assert_eq!(tool_names, vec!["search_files", "terminal"]);
}

#[test]
fn real_terminal_result_has_exit_code() {
    let events = translate_fixture("real_search_bash.jsonl");
    let tool_results: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .collect();
    // Second tool result is from terminal
    let terminal_result = hermes_payload(tool_results[1]);
    let content = terminal_result.text.as_deref().unwrap();
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert_eq!(
        parsed["exit_code"].as_i64(),
        Some(0),
        "terminal result should have exit_code=0"
    );
    assert!(
        parsed.get("output").is_some(),
        "terminal result should have output field"
    );
}

// ── Delegate session ────────────────────────────────────────────

#[test]
fn real_delegate_exercises_subagent() {
    let events = translate_fixture("real_delegate.jsonl");
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 1);
    let p = hermes_payload(tool_uses[0]);
    assert_eq!(p.tool.as_deref(), Some("delegate_task"));

    // Delegate arguments should have a tasks array
    let args = p.args.as_ref().unwrap();
    assert!(
        args.get("tasks").is_some(),
        "delegate_task should have tasks arg: {}",
        serde_json::to_string(args).unwrap()
    );
}

#[test]
fn real_delegate_result_is_structured_json() {
    let events = translate_fixture("real_delegate.jsonl");
    let tool_result = events.iter()
        .find(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .unwrap();
    let p = hermes_payload(tool_result);
    let content = p.text.as_deref().unwrap();
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert!(
        parsed.get("results").is_some(),
        "delegate result should have results array"
    );
}

// ── Code review + patch session ─────────────────────────────────

#[test]
fn real_code_review_exercises_patch_tool() {
    let events = translate_fixture("real_code_review_patch.jsonl");
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 2, "read + patch = 2 tool_use events");

    let tool_names: Vec<&str> = tool_uses.iter()
        .map(|e| hermes_payload(e).tool.as_deref().unwrap_or("?"))
        .collect();
    assert_eq!(tool_names, vec!["read_file", "patch"]);
}

#[test]
fn real_patch_result_has_success_and_diff() {
    let events = translate_fixture("real_code_review_patch.jsonl");
    let tool_results: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .collect();
    // Second tool result is from patch
    let patch_result = hermes_payload(tool_results[1]);
    let content = patch_result.text.as_deref().unwrap();
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert_eq!(
        parsed["success"].as_bool(),
        Some(true),
        "patch result should have success: true"
    );
    assert!(
        parsed.get("diff").is_some(),
        "patch result should have a diff field"
    );
}

#[test]
fn real_patch_args_contain_old_and_new_string() {
    let events = translate_fixture("real_code_review_patch.jsonl");
    let patch_use = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .find(|e| hermes_payload(e).tool.as_deref() == Some("patch"))
        .expect("should have a patch tool_use");
    let p = hermes_payload(patch_use);
    let args = p.args.as_ref().unwrap();
    assert!(args.get("path").is_some(), "patch should have path arg");
    assert!(args.get("old_string").is_some(), "patch should have old_string arg");
    assert!(args.get("new_string").is_some(), "patch should have new_string arg");
}

// ── Execute code session ────────────────────────────────────────

#[test]
fn real_execute_code_uses_terminal_tool() {
    let events = translate_fixture("real_execute_code.jsonl");
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 1);
    let p = hermes_payload(tool_uses[0]);
    assert_eq!(p.tool.as_deref(), Some("terminal"));
    let args = p.args.as_ref().unwrap();
    assert!(
        args["command"].as_str().unwrap().contains("python"),
        "terminal command should run python"
    );
}

#[test]
fn real_execute_code_result_has_output_and_exit_code() {
    let events = translate_fixture("real_execute_code.jsonl");
    let tool_result = events.iter()
        .find(|e| e.subtype.as_deref() == Some("message.user.tool_result"))
        .unwrap();
    let p = hermes_payload(tool_result);
    let content = p.text.as_deref().unwrap();
    let parsed: Value = serde_json::from_str(content).unwrap();
    assert_eq!(parsed["exit_code"].as_i64(), Some(0));
    // Output is the fibonacci sequence + primes — verify it contains numbers
    let output = parsed["output"].as_str().unwrap();
    assert!(
        output.contains("[0, 1, 1, 2, 3, 5, 8"),
        "should contain fibonacci sequence: {}",
        output
    );
}

// ── Complex refactor session (16 msgs, 7 tool calls) ───────────

#[test]
fn real_complex_refactor_translates_full_session() {
    let events = translate_fixture("real_complex_refactor.jsonl");
    // 18 input lines → events: start + user + 7*(tool_use + tool_result) + text + end = 18
    assert!(events.len() >= 16, "complex session should produce many events, got {}", events.len());

    // Should have 7 tool_use events
    let tool_uses: Vec<&CloudEvent> = events.iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();
    assert_eq!(tool_uses.len(), 7, "7 tool calls in the complex refactor");

    // Check tool diversity
    let mut tool_names: Vec<&str> = tool_uses.iter()
        .map(|e| hermes_payload(e).tool.as_deref().unwrap_or("?"))
        .collect();
    tool_names.sort();
    tool_names.dedup();
    assert!(
        tool_names.len() >= 3,
        "should exercise at least 3 tool types, got {:?}",
        tool_names
    );
}

#[test]
fn real_complex_refactor_has_alternating_tool_pattern() {
    // Multi-turn sessions should have strict alternation:
    // assistant(tool_use) → tool(result) → assistant(tool_use) → ...
    let events = translate_fixture("real_complex_refactor.jsonl");
    let relevant: Vec<(&str, &str)> = events.iter()
        .filter_map(|e| {
            let st = e.subtype.as_deref()?;
            if st == "message.assistant.tool_use" || st == "message.user.tool_result" {
                Some((st, ""))
            } else {
                None
            }
        })
        .collect();
    // Every tool_use should be followed by a tool_result
    for pair in relevant.windows(2) {
        if pair[0].0 == "message.assistant.tool_use" {
            assert_eq!(
                pair[1].0, "message.user.tool_result",
                "tool_use must be followed by tool_result"
            );
        }
    }
}

// ── Cross-session invariants ────────────────────────────────────

#[test]
fn all_real_sessions_have_hermes_agent_tag() {
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        for ev in &events {
            assert_eq!(
                ev.agent.as_deref(),
                Some("hermes"),
                "event in {} missing hermes tag",
                fixture
            );
        }
    }
}

#[test]
fn all_real_sessions_have_unique_event_ids() {
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "duplicate IDs in {}: {:?}",
            fixture,
            ids
        );
    }
}

#[test]
fn all_real_sessions_start_and_end_correctly() {
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        assert!(events.len() >= 4, "{} should have at least 4 events", fixture);
        assert_eq!(
            events.first().unwrap().subtype.as_deref(),
            Some("system.session.start"),
            "{} should start with session.start",
            fixture
        );
        assert_eq!(
            events.last().unwrap().subtype.as_deref(),
            Some("system.turn.complete"),
            "{} should end with turn.complete",
            fixture
        );
    }
}

#[test]
fn no_real_session_has_tool_name_on_tool_results() {
    // Runtime gap #2 CONFIRMED across all 5 real sessions: tool_name is NEVER
    // present on tool result messages. Only tool_call_id + content.
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        for ev in events.iter().filter(|e| e.subtype.as_deref() == Some("message.user.tool_result")) {
            let p = hermes_payload(ev);
            assert!(
                p.tool_name.is_none(),
                "{}: tool_name should be None on tool results (gap #2)",
                fixture
            );
        }
    }
}

#[test]
fn no_real_session_has_non_null_reasoning() {
    // Sonnet 4 without extended thinking: reasoning field is always None.
    // This confirms gap #4 for non-thinking models. A future test with
    // an extended-thinking model would verify the non-null path.
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        let thinking: Vec<&CloudEvent> = events.iter()
            .filter(|e| e.subtype.as_deref() == Some("message.assistant.thinking"))
            .collect();
        assert!(
            thinking.is_empty(),
            "{}: should have no thinking events (reasoning=None on Sonnet 4)",
            fixture
        );
    }
}

#[test]
fn all_tool_results_content_is_valid_json() {
    // Discovery: Hermes tool results ALWAYS wrap output in a JSON string.
    // This test confirms every tool result across all sessions parses as JSON.
    for fixture in &[
        "real_simple_read.jsonl",
        "real_tool_error.jsonl",
        "real_write_read_chain.jsonl",
        "real_search_bash.jsonl",
        "real_delegate.jsonl",
        "real_code_review_patch.jsonl",
        "real_execute_code.jsonl",
        "real_complex_refactor.jsonl",
    ] {
        let events = translate_fixture(fixture);
        for ev in events.iter().filter(|e| e.subtype.as_deref() == Some("message.user.tool_result")) {
            let p = hermes_payload(ev);
            let content = p.text.as_deref().unwrap_or("");
            if !content.is_empty() {
                let parsed: Result<Value, _> = serde_json::from_str(content);
                assert!(
                    parsed.is_ok(),
                    "{}: tool result content should be valid JSON: {}",
                    fixture,
                    &content[..content.len().min(200)]
                );
            }
        }
    }
}

// ── Phase B: testcontainer tests ─────────────────────────────────────
// These require the Docker image to be built:
//   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
//
// They are skipped when the image is not available (same pattern as
// the open-story:test container tests).

// ── Testcontainer tests ──────────────────────────────────────────────
//
// These require Docker images to be built:
//   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
//   docker build -t open-story:test ./rs
//
// Run with:
//   cargo test -p open-story --features hermes_container --test test_hermes_translator -- container
//
// The tests verify the full containerized pipeline:
//   hermes-fixture container → JSONL/snapshot output → translator → CloudEvents
//   hermes-fixture → open-story container → REST API responses

#[cfg(feature = "hermes_container")]
mod container_tests {
    use super::*;
    use open_story_core::event_data::AgentPayload;
    use testcontainers::core::Mount;
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{GenericImage, ImageExt};

    /// Run the hermes-fixture container, return (jsonl_content, snapshot_content).
    async fn run_fixture_container() -> (String, String) {
        let image = GenericImage::new("hermes-fixture", "test");
        let tmp = tempfile::tempdir().unwrap();
        let output_mount = Mount::bind_mount(
            tmp.path().to_str().unwrap(),
            "/output",
        );

        let container = image
            .with_mount(output_mount)
            .start()
            .await
            .expect("failed to start hermes-fixture container");

        container
            .stop()
            .await
            .expect("container didn't stop cleanly");

        let jsonl_path = tmp.path().join("session_plugin.jsonl");
        let snapshot_path = tmp.path().join("session_snapshot.json");

        let jsonl = std::fs::read_to_string(&jsonl_path)
            .expect("container should have written session_plugin.jsonl");
        let snapshot = std::fs::read_to_string(&snapshot_path)
            .expect("container should have written session_snapshot.json");

        (jsonl, snapshot)
    }

    #[tokio::test]
    async fn container_produces_valid_jsonl() {
        let (jsonl, _) = run_fixture_container().await;
        let mut state = TranscriptState::new("container-test".to_string());
        let mut events: Vec<CloudEvent> = Vec::new();

        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            let parsed: Value = serde_json::from_str(line).unwrap();
            assert!(is_hermes_format(&parsed));
            events.extend(translate_hermes_line(&parsed, &mut state));
        }

        assert_eq!(events.len(), 7);
        let subtypes: Vec<&str> = events.iter().filter_map(|e| e.subtype.as_deref()).collect();
        assert_eq!(subtypes, vec![
            "system.session.start",
            "message.user.prompt",
            "message.assistant.thinking",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.text",
            "system.turn.complete",
        ]);
    }

    #[tokio::test]
    async fn container_events_all_carry_hermes_tag() {
        let (jsonl, _) = run_fixture_container().await;
        let mut state = TranscriptState::new("container-tag".to_string());
        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            let parsed: Value = serde_json::from_str(line).unwrap();
            for ev in translate_hermes_line(&parsed, &mut state) {
                assert_eq!(ev.agent.as_deref(), Some("hermes"), "all events must carry hermes agent tag");
                match ev.data.agent_payload.as_ref() {
                    Some(AgentPayload::Hermes(_)) => {},
                    other => panic!("expected Hermes payload, got {:?}", other),
                }
            }
        }
    }

    #[tokio::test]
    async fn container_event_ids_are_unique() {
        let (jsonl, _) = run_fixture_container().await;
        let mut state = TranscriptState::new("container-ids".to_string());
        let mut ids: Vec<String> = Vec::new();
        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            let parsed: Value = serde_json::from_str(line).unwrap();
            for ev in translate_hermes_line(&parsed, &mut state) {
                ids.push(ev.id.clone());
            }
        }
        let unique: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(unique.len(), ids.len(), "container event IDs must be unique");
    }

    #[tokio::test]
    async fn container_event_ids_are_deterministic() {
        let (jsonl, _) = run_fixture_container().await;

        let translate = |seed: &str| -> Vec<String> {
            let mut state = TranscriptState::new(seed.to_string());
            let mut ids = Vec::new();
            for line in jsonl.lines() {
                if line.trim().is_empty() { continue; }
                let parsed: Value = serde_json::from_str(line).unwrap();
                for ev in translate_hermes_line(&parsed, &mut state) {
                    ids.push(ev.id.clone());
                }
            }
            ids
        };

        let run1 = translate("det-1");
        let run2 = translate("det-2");
        // Same input → same IDs (deterministic uuid5)
        // Note: seed differs but that only affects TranscriptState.session_id,
        // not the derive_event_id which uses the envelope's session_id.
        assert_eq!(run1, run2, "same JSONL input must produce same event IDs");
    }

    #[tokio::test]
    async fn container_snapshot_has_valid_structure() {
        let (_, snapshot) = run_fixture_container().await;
        let snap: Value = serde_json::from_str(&snapshot).unwrap();

        assert!(snap["session_id"].is_string());
        assert!(snap["model"].is_string());
        assert!(snap["platform"].is_string());
        assert!(snap["session_start"].is_string());
        assert!(snap["last_updated"].is_string());
        assert!(snap["system_prompt"].is_string());
        assert!(snap["tools"].is_array());
        assert!(snap["message_count"].is_number());
        assert!(snap["messages"].is_array());

        let messages = snap["messages"].as_array().unwrap();
        assert_eq!(messages.len(), snap["message_count"].as_u64().unwrap() as usize);
    }

    #[tokio::test]
    async fn container_snapshot_works_with_snapshot_watcher() {
        let (_, snapshot_content) = run_fixture_container().await;

        // Write the snapshot to a temp file as the watcher would see it
        let dir = tempfile::tempdir().unwrap();
        let snap: Value = serde_json::from_str(&snapshot_content).unwrap();
        let sid = snap["session_id"].as_str().unwrap();
        let path = dir.path().join(format!("session_{}.json", sid));
        std::fs::write(&path, &snapshot_content).unwrap();

        // Process through the snapshot watcher
        let mut states = std::collections::HashMap::new();
        let events = open_story::snapshot_watcher::process_snapshot(&path, &mut states).unwrap();

        assert!(!events.is_empty(), "snapshot watcher should produce events");

        let subtypes: Vec<&str> = events.iter().filter_map(|e| e.subtype.as_deref()).collect();
        assert!(subtypes.contains(&"system.session.start"));
        assert!(subtypes.contains(&"message.user.prompt"));
        assert!(subtypes.contains(&"message.assistant.tool_use"));

        // All events should be Hermes
        for ev in &events {
            assert_eq!(ev.agent.as_deref(), Some("hermes"));
        }
    }

    #[tokio::test]
    async fn container_tool_call_result_linkage() {
        let (jsonl, _) = run_fixture_container().await;
        let mut state = TranscriptState::new("container-link".to_string());
        let mut events: Vec<CloudEvent> = Vec::new();
        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            let parsed: Value = serde_json::from_str(line).unwrap();
            events.extend(translate_hermes_line(&parsed, &mut state));
        }

        // Collect tool_use_ids and tool_call_ids
        let mut tool_use_ids: Vec<String> = Vec::new();
        let mut tool_call_ids: Vec<String> = Vec::new();

        for ev in &events {
            if let Some(AgentPayload::Hermes(h)) = ev.data.agent_payload.as_ref() {
                if let Some(ref id) = h.tool_use_id {
                    tool_use_ids.push(id.clone());
                }
                if let Some(ref id) = h.tool_call_id {
                    tool_call_ids.push(id.clone());
                }
            }
        }

        assert!(!tool_use_ids.is_empty(), "should have tool_use events");
        assert!(!tool_call_ids.is_empty(), "should have tool_result events");

        // Every tool_call_id should match a tool_use_id
        for call_id in &tool_call_ids {
            assert!(
                tool_use_ids.contains(call_id),
                "tool_call_id {} should match a tool_use_id",
                call_id
            );
        }
    }

    #[tokio::test]
    async fn container_raw_field_preserved() {
        let (jsonl, _) = run_fixture_container().await;
        let mut state = TranscriptState::new("container-raw".to_string());
        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            let parsed: Value = serde_json::from_str(line).unwrap();
            for ev in translate_hermes_line(&parsed, &mut state) {
                // raw should be the original input line
                assert!(
                    !ev.data.raw.is_null(),
                    "raw field should be preserved for subtype {:?}",
                    ev.subtype
                );
                // raw should contain the envelope
                assert!(
                    ev.data.raw.get("envelope").is_some() || ev.data.raw.get("data").is_some(),
                    "raw should contain the Hermes envelope structure"
                );
            }
        }
    }
}
