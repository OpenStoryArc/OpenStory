//! Grok Build ACP `updates.jsonl` → CloudEvents via the reader pipeline.
//!
//! Spec-as-tests (TDD boundary tables). Each case is a behavior contract:
//! given an ACP line shape, when we translate, then subtypes/agent/raw hold.
//!
//! Fixture: `rs/tests/fixtures/grok/sample_updates.jsonl` (minimized from a
//! real Grok Build session under `~/.grok/sessions/…/updates.jsonl`).

use std::io::Write;
use std::path::PathBuf;

use open_story_core::event_data::{AgentPayload, GrokPayload, ToolOutcome};
use open_story_core::reader::read_new_lines;
use open_story_core::translate::{TranscriptFormat, TranscriptState};
use open_story_core::translate_grok::{is_grok_format, translate_grok_line};
use serde_json::{json, Value};
use tempfile::NamedTempFile;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/grok/sample_updates.jsonl")
}

fn grok_payload(event: &open_story_core::cloud_event::CloudEvent) -> &GrokPayload {
    match event.data.agent_payload.as_ref().expect("agent payload") {
        AgentPayload::Grok(payload) => payload,
        other => panic!("expected Grok payload, got agent={}", other.agent()),
    }
}

// ── Boundary table: format detection ───────────────────────────────

#[test]
fn is_grok_format_boundary_table() {
    // (name, line, expected)
    let cases: Vec<(&str, Value, bool)> = vec![
        (
            "session/update with sessionUpdate",
            json!({"method":"session/update","params":{"update":{"sessionUpdate":"tool_call"}}}),
            true,
        ),
        (
            "x.ai turn_completed method",
            json!({"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"turn_completed"}}}),
            true,
        ),
        (
            "missing sessionUpdate",
            json!({"method":"session/update","params":{"update":{}}}),
            false,
        ),
        (
            "claude assistant line",
            json!({"type":"assistant","message":{"role":"assistant"}}),
            false,
        ),
        (
            "codex session_meta",
            json!({"type":"session_meta","payload":{"id":"x"}}),
            false,
        ),
        (
            "empty object",
            json!({}),
            false,
        ),
    ];

    for (name, line, expected) in cases {
        assert_eq!(
            is_grok_format(&line),
            expected,
            "case `{name}`: is_grok_format({line})"
        );
    }
}

// ── Boundary table: subtype mapping ────────────────────────────────

#[test]
fn session_update_to_subtype_boundary_table() {
    // (name, sessionUpdate, status?, expected_subtype_or_empty)
    let cases: Vec<(&str, &str, Option<&str>, Option<&str>)> = vec![
        ("user chunk", "user_message_chunk", None, Some("message.user.prompt")),
        (
            "thought chunk",
            "agent_thought_chunk",
            None,
            Some("message.assistant.thinking"),
        ),
        (
            "message chunk",
            "agent_message_chunk",
            None,
            Some("message.assistant.text"),
        ),
        ("tool_call", "tool_call", None, Some("message.assistant.tool_use")),
        (
            "tool completed",
            "tool_call_update",
            Some("completed"),
            Some("message.user.tool_result"),
        ),
        ("tool in_progress skipped", "tool_call_update", Some("in_progress"), None),
        (
            "turn_completed",
            "turn_completed",
            None,
            Some("system.turn.complete"),
        ),
        (
            "unknown kind passthrough",
            "plan",
            None,
            Some("system.grok.plan"),
        ),
    ];

    for (name, kind, status, expected) in cases {
        let mut state = TranscriptState::new("boundary".into());
        let mut update = json!({
            "sessionUpdate": kind,
            "content": {"type": "text", "text": "x"},
            "toolCallId": "call-1",
            "title": "list_dir",
            "rawInput": {"target_directory": "/tmp"},
            "rawOutput": {"Content": {"content": "out"}},
        });
        if let Some(st) = status {
            update
                .as_object_mut()
                .unwrap()
                .insert("status".into(), json!(st));
        }
        // Prefer x.ai method for turn_completed (matches production wire).
        let method = if kind == "turn_completed" {
            "_x.ai/session/update"
        } else {
            "session/update"
        };
        let line = json!({
            "timestamp": 1_700_000_000,
            "method": method,
            "params": {
                "sessionId": "sess-boundary",
                "update": update,
                "_meta": {"eventId": format!("e-{name}")}
            }
        });

        let events = translate_grok_line(&line, &mut state);
        match expected {
            None => assert!(
                events.is_empty(),
                "case `{name}`: expected no events, got {:?}",
                events.iter().map(|e| e.subtype.clone()).collect::<Vec<_>>()
            ),
            Some(sub) => {
                assert_eq!(events.len(), 1, "case `{name}`: expected one event");
                assert_eq!(
                    events[0].subtype.as_deref(),
                    Some(sub),
                    "case `{name}` subtype"
                );
                assert_eq!(events[0].agent.as_deref(), Some("grok"));
                assert_eq!(
                    events[0].data.raw, line,
                    "case `{name}`: raw must equal source line"
                );
            }
        }
    }
}

// ── Boundary table: ToolOutcome derivation ─────────────────────────

#[test]
fn tool_outcome_boundary_table() {
    // (name, tool, input, output, is_error, expected type tag)
    type OutcomeCase =
        (&'static str, &'static str, Value, &'static str, bool, Option<&'static str>);
    let cases: Vec<OutcomeCase> = vec![
        (
            "read_file ok",
            "read_file",
            json!({"target_file": "/a.rs"}),
            "fn main()",
            false,
            Some("FileRead"),
        ),
        (
            "read_file error",
            "read_file",
            json!({"target_file": "/missing"}),
            "not found",
            true,
            Some("FileReadFailed"),
        ),
        (
            "search_replace",
            "search_replace",
            json!({"file_path": "/a.rs"}),
            "ok",
            false,
            Some("FileModified"),
        ),
        (
            "run_terminal_command",
            "run_terminal_command",
            json!({"command": "ls"}),
            "a\nb",
            false,
            Some("CommandExecuted"),
        ),
        (
            "list_dir",
            "list_dir",
            json!({"target_directory": "/tmp"}),
            "files",
            false,
            Some("SearchPerformed"),
        ),
        (
            "unknown tool",
            "image_gen",
            json!({}),
            "png",
            false,
            None,
        ),
    ];

    for (name, tool, input, output, is_error, expected_tag) in cases {
        let mut state = TranscriptState::new("outcome".into());
        // Seed pending tool so completed update can resolve name+args.
        let call = json!({
            "timestamp": 1,
            "method": "session/update",
            "params": {
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": format!("call-{name}"),
                    "title": tool,
                    "rawInput": input,
                    "_meta": {"x.ai/tool": {"name": tool}}
                },
                "_meta": {"eventId": format!("call-{name}")}
            }
        });
        let done = json!({
            "timestamp": 2,
            "method": "session/update",
            "params": {
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": format!("call-{name}"),
                    "status": "completed",
                    "rawOutput": {"Content": {"content": output}},
                    "isError": is_error
                },
                "_meta": {"eventId": format!("done-{name}")}
            }
        });
        assert_eq!(translate_grok_line(&call, &mut state).len(), 1);
        let result = translate_grok_line(&done, &mut state);
        assert_eq!(result.len(), 1, "case `{name}`");
        let outcome = grok_payload(&result[0]).tool_outcome.as_ref();
        match expected_tag {
            None => assert!(outcome.is_none(), "case `{name}` expected no outcome"),
            Some(tag) => {
                let outcome =
                    outcome.unwrap_or_else(|| panic!("case `{name}` expected outcome"));
                let v = serde_json::to_value(outcome).unwrap();
                assert_eq!(
                    v.get("type").and_then(|t| t.as_str()),
                    Some(tag),
                    "case `{name}` outcome tag"
                );
            }
        }
    }
}

// ── Reader pipeline integration ────────────────────────────────────

#[test]
fn reader_detects_grok_format_and_locks_it() {
    let mut file = NamedTempFile::new().expect("temp");
    writeln!(
        file,
        r#"{{"timestamp":1700000000,"method":"session/update","params":{{"sessionId":"abc","update":{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"hi"}}}},"_meta":{{"eventId":"e1"}}}}}}"#
    )
    .unwrap();
    // Second line would look like Claude if format weren't locked — but after
    // first Grok line, format stays Grok and Claude-shaped lines yield nothing
    // useful without sessionUpdate. We only assert lock on first detection.
    file.flush().unwrap();

    let mut state = TranscriptState::new("fixture".into());
    let events = read_new_lines(file.path(), &mut state).expect("read");
    assert_eq!(state.format, TranscriptFormat::Grok);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].agent.as_deref(), Some("grok"));
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.prompt"));
    assert_eq!(state.session_id, "abc");
}

#[test]
fn fixture_sample_produces_tool_pairs_and_turn_boundaries() {
    let path = fixture_path();
    assert!(
        path.exists(),
        "missing fixture at {} — capture from a real Grok session",
        path.display()
    );

    let mut state = TranscriptState::new("sample".into());
    let events = read_new_lines(&path, &mut state).expect("read fixture");

    assert_eq!(state.format, TranscriptFormat::Grok);
    assert!(
        !events.is_empty(),
        "fixture should emit at least one CloudEvent"
    );

    // Every event carries the Grok discriminator + io.arc.event type.
    for e in &events {
        assert_eq!(e.agent.as_deref(), Some("grok"));
        assert_eq!(e.event_type, "io.arc.event");
        assert!(e.source.starts_with("grok://session/"));
    }

    let subtypes: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or(""))
        .collect();

    // Must see the core agentic shapes somewhere in the fixture stream.
    assert!(
        subtypes.contains(&"message.user.prompt"),
        "expected user prompt; got {subtypes:?}"
    );
    assert!(
        subtypes.contains(&"message.assistant.tool_use"),
        "expected tool_use; got {subtypes:?}"
    );
    assert!(
        subtypes.contains(&"message.user.tool_result"),
        "expected tool_result; got {subtypes:?}"
    );
    assert!(
        subtypes.contains(&"system.turn.complete"),
        "expected synthetic turn boundary; got {subtypes:?}"
    );

    // Session id should rekey from the wire sessionId field.
    assert_ne!(state.session_id, "sample");
    assert!(
        state.session_id.len() >= 32,
        "session id should look like a uuid, got {}",
        state.session_id
    );
}

#[test]
fn tool_call_then_completed_correlates_by_id() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"tool_call","toolCallId":"call-xyz","title":"list_dir","rawInput":{{"target_directory":"/tmp"}},"_meta":{{"x.ai/tool":{{"name":"list_dir"}}}}}},"_meta":{{"eventId":"e-call"}}}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":2,"method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"call-xyz","status":"completed","rawOutput":{{"Content":{{"content":"a\nb"}}}}}},"_meta":{{"eventId":"e-done"}}}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":3,"method":"_x.ai/session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"turn_completed","prompt_id":"p1","stop_reason":"end_turn","usage":{{"inputTokens":10}}}},"_meta":{{"eventId":"e-turn"}}}}}}"#
    )
    .unwrap();
    file.flush().unwrap();

    let mut state = TranscriptState::new("corr".into());
    let events = read_new_lines(file.path(), &mut state).unwrap();
    let subtypes: Vec<_> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap())
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "message.assistant.tool_use",
            "message.user.tool_result",
            "system.turn.complete",
        ]
    );

    let use_ev = &events[0];
    let res_ev = &events[1];
    assert_eq!(grok_payload(use_ev).tool.as_deref(), Some("list_dir"));
    assert_eq!(
        grok_payload(use_ev).tool_call_id.as_deref(),
        Some("call-xyz")
    );
    assert_eq!(
        grok_payload(res_ev).tool_call_id.as_deref(),
        Some("call-xyz")
    );
    assert!(
        grok_payload(res_ev)
            .text
            .as_deref()
            .unwrap_or("")
            .contains("a\nb")
    );
    assert!(matches!(
        grok_payload(res_ev).tool_outcome.as_ref(),
        Some(ToolOutcome::SearchPerformed { .. })
    ));

    let turn = &events[2];
    assert_eq!(
        grok_payload(turn).stop_reason.as_deref(),
        Some("end_turn")
    );
    assert!(grok_payload(turn).token_usage.is_some());
}

#[test]
fn dedupe_by_event_id_across_replay() {
    let line = json!({
        "timestamp": 1,
        "method": "session/update",
        "params": {
            "sessionId": "s",
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": "hi"}
            },
            "_meta": {"eventId": "same-id"}
        }
    });
    let mut state = TranscriptState::new("dedupe".into());
    assert_eq!(translate_grok_line(&line, &mut state).len(), 1);
    assert_eq!(
        translate_grok_line(&line, &mut state).len(),
        0,
        "second sighting of same eventId must be dropped (boot backfill safety)"
    );
}
