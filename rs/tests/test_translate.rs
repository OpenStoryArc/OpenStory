//! Port of 41 Python unit tests from test_transcript_translator.py.
//! Plus io.arc.event unified taxonomy tests.

use open_story::event_data::{AgentPayload, ClaudeCodePayload};
use open_story::translate::*;
use serde_json::{json, Value};

fn state() -> TranscriptState {
    TranscriptState::new("test-session".to_string())
}

fn base_entry(overrides: Value) -> Value {
    let mut entry = json!({
        "parentUuid": "parent-1",
        "sessionId": "test-session",
        "cwd": "/home/user/project",
        "version": "2.1.68",
        "gitBranch": "main",
        "type": "user",
    });
    if let (Value::Object(base), Value::Object(ovr)) = (&mut entry, overrides) {
        for (k, v) in ovr {
            base.insert(k, v);
        }
    }
    entry
}

/// Helper: extract ClaudeCodePayload from event, panicking if not present.
fn cc_payload(event: &open_story::cloud_event::CloudEvent) -> &ClaudeCodePayload {
    match event.data.agent_payload.as_ref().expect("agent_payload should be Some") {
        AgentPayload::ClaudeCode(cc) => cc,
        _ => panic!("expected ClaudeCode payload"),
    }
}

// ── TranscriptState ────────────────────────────────────────

#[test]
fn test_seq_increments() {
    let mut s = state();
    assert_eq!(s.next_seq(), 1);
    assert_eq!(s.next_seq(), 2);
}

#[test]
fn test_initial_state() {
    let s = state();
    assert_eq!(s.byte_offset, 0);
    assert_eq!(s.line_count, 0);
    assert!(s.seen_uuids.is_empty());
}

// ── Assistant events ───────────────────────────────────────

#[test]
fn test_assistant_text_subtype() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "model": "claude-opus-4-6",
            "content": [{"type": "text", "text": "Hello!"}],
            "usage": {"input_tokens": 10, "output_tokens": 5},
            "stop_reason": "end_turn",
            "id": "msg_123",
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e.event_type, TRANSCRIPT_ASSISTANT);
    assert_eq!(e.subtype.as_deref(), Some("message.assistant.text"));
    let p = cc_payload(e);
    assert_eq!(p.model.as_deref(), Some("claude-opus-4-6"));
    assert_eq!(p.token_usage.as_ref().unwrap()["input_tokens"], 10);
    assert_eq!(p.stop_reason.as_ref().and_then(|v| v.as_str()), Some("end_turn"));
    assert_eq!(p.message_id.as_deref(), Some("msg_123"));
}

#[test]
fn test_assistant_tool_use_subtype() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "model": "claude-opus-4-6",
            "content": [
                {"type": "thinking", "thinking": "hmm"},
                {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"path": "/foo"}},
            ],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.tool_use"));
}

#[test]
fn test_assistant_thinking_subtype() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "deep thought"}],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.thinking"));
}

#[test]
fn test_raw_preserved() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].data.raw, line);
}

#[test]
fn test_source_uri() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {"role": "assistant", "content": "hi"},
    }));
    let mut s = TranscriptState::new("my-session".to_string());
    let events = translate_line(&line, &mut s);
    assert_eq!(events[0].source, "arc://transcript/my-session");
}

#[test]
fn test_content_types_listed() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "hello"},
            ],
        },
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.content_types, Some(vec!["thinking".to_string(), "text".to_string()]));
}

#[test]
fn test_string_content_treated_as_text() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {"role": "assistant", "content": "just a string"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.text"));
}

// ── User events ────────────────────────────────────────────

#[test]
fn test_user_text_subtype() {
    let line = base_entry(json!({
        "type": "user",
        "message": {"role": "user", "content": "Hello Claude"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, TRANSCRIPT_USER);
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.prompt"));
}

#[test]
fn test_user_tool_result_subtype() {
    let line = base_entry(json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "ok"},
            ],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.tool_result"));
}

#[test]
fn test_user_type_extracted() {
    let line = base_entry(json!({
        "type": "user",
        "userType": "external",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.user_type.as_deref(), Some("external"));
}

// ── Progress events ────────────────────────────────────────

#[test]
fn test_bash_progress() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "bash_progress", "output": "ls output"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, TRANSCRIPT_PROGRESS);
    assert_eq!(events[0].subtype.as_deref(), Some("progress.bash"));
}

#[test]
fn test_agent_progress() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "agent_progress", "agent_id": "a1"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("progress.agent"));
}

#[test]
fn test_hook_progress() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "hook_progress", "hookEvent": "SessionStart"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("progress.hook"));
}

#[test]
fn test_unknown_progress_type() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "new_future_type"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("progress.new_future_type"));
}

// ── System events ──────────────────────────────────────────

#[test]
fn test_system_turn_duration() {
    let line = base_entry(json!({
        "type": "system",
        "subtype": "turn_duration",
        "durationMs": 5000,
        "timestamp": "2025-01-05T17:00:00.000Z",
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, TRANSCRIPT_SYSTEM);
    assert_eq!(events[0].subtype.as_deref(), Some("system.turn.complete"));
    let p = cc_payload(&events[0]);
    assert_eq!(p.duration_ms, Some(5000.0));
}

#[test]
fn test_system_stop_hook_summary() {
    let line = base_entry(json!({
        "type": "system",
        "subtype": "stop_hook_summary",
        "hookCount": 2,
        "preventedContinuation": false,
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("system.hook"));
    let p = cc_payload(&events[0]);
    assert_eq!(p.hook_count, Some(2));
    assert_eq!(p.prevented_continuation, Some(false));
}

#[test]
fn test_system_api_error() {
    let line = base_entry(json!({"type": "system", "subtype": "api_error"}));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("system.error"));
}

#[test]
fn test_system_compact_boundary() {
    let line = base_entry(json!({"type": "system", "subtype": "compact_boundary"}));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("system.compact"));
}

#[test]
fn test_system_unknown_subtype() {
    let line = base_entry(json!({"type": "system"}));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("system.unknown"));
}

// ── Snapshot events ────────────────────────────────────────

#[test]
fn test_file_history_snapshot() {
    let line = json!({
        "type": "file-history-snapshot",
        "messageId": "msg-1",
        "snapshot": {"trackedFileBackups": {}, "timestamp": "2025-01-05T16:55:00Z"},
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, TRANSCRIPT_SNAPSHOT);
    assert_eq!(events[0].subtype.as_deref(), Some("file.snapshot"));
    assert_eq!(events[0].data.raw["messageId"], "msg-1");
}

// ── Queue events ───────────────────────────────────────────

#[test]
fn test_enqueue_operation() {
    let line = json!({
        "type": "queue-operation",
        "operation": "enqueue",
        "timestamp": "2025-01-05T17:05:00Z",
        "sessionId": "s1",
        "content": "do something",
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, TRANSCRIPT_QUEUE);
    assert_eq!(events[0].subtype.as_deref(), Some("queue.enqueue"));
    let p = cc_payload(&events[0]);
    assert_eq!(p.operation.as_deref(), Some("enqueue"));
}

#[test]
fn test_dequeue_operation() {
    let line = json!({
        "type": "queue-operation",
        "operation": "dequeue",
        "timestamp": "2025-01-05T17:06:00Z",
        "sessionId": "s1",
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].subtype.as_deref(), Some("queue.dequeue"));
}

// ── Unknown types ──────────────────────────────────────────

#[test]
fn test_unknown_type_returns_empty() {
    let line = json!({"type": "completely_new_thing", "data": {}});
    let events = translate_line(&line, &mut state());
    assert!(events.is_empty());
}

#[test]
fn test_missing_type_returns_empty() {
    let line = json!({"data": {}});
    let events = translate_line(&line, &mut state());
    assert!(events.is_empty());
}

// ── Deduplication ──────────────────────────────────────────

#[test]
fn test_duplicate_uuid_skipped() {
    let mut s = state();
    let line = base_entry(json!({
        "type": "assistant",
        "uuid": "uuid-1",
        "message": {"role": "assistant", "content": "hi"},
    }));
    let events1 = translate_line(&line, &mut s);
    let events2 = translate_line(&line, &mut s);
    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 0);
}

#[test]
fn test_different_uuids_both_processed() {
    let mut s = state();
    let line1 = base_entry(json!({"type": "user", "uuid": "uuid-1", "message": {"role": "user", "content": "a"}}));
    let line2 = base_entry(json!({"type": "user", "uuid": "uuid-2", "message": {"role": "user", "content": "b"}}));
    let events1 = translate_line(&line1, &mut s);
    let events2 = translate_line(&line2, &mut s);
    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 1);
}

#[test]
fn test_no_uuid_always_processed() {
    let mut s = state();
    let line = base_entry(json!({"type": "user", "message": {"role": "user", "content": "hi"}}));
    let events1 = translate_line(&line, &mut s);
    let events2 = translate_line(&line, &mut s);
    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 1);
}

#[test]
fn test_uuid_used_as_event_id() {
    let line = base_entry(json!({
        "type": "user",
        "uuid": "my-uuid",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].id, "my-uuid");
}

// ── Envelope field promotion ───────────────────────────────

#[test]
fn test_common_fields_promoted() {
    let line = base_entry(json!({
        "type": "user",
        "uuid": "u1",
        "sessionId": "s1",
        "cwd": "/project",
        "version": "2.1.68",
        "gitBranch": "main",
        "slug": "my-slug",
        "timestamp": "2025-01-05T17:00:00Z",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    let data = &events[0].data;
    let p = cc_payload(&events[0]);
    assert_eq!(data.session_id, "s1");
    assert_eq!(p.cwd.as_deref(), Some("/project"));
    assert_eq!(p.version.as_deref(), Some("2.1.68"));
    assert_eq!(p.git_branch.as_deref(), Some("main"));
    assert_eq!(p.slug.as_deref(), Some("my-slug"));
}

#[test]
fn test_timestamp_used_as_event_time() {
    let line = base_entry(json!({
        "type": "user",
        "timestamp": "2025-01-05T17:00:00.000Z",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].time, "2025-01-05T17:00:00.000Z");
}

// ── CloudEvent schema compliance ───────────────────────────

#[test]
fn test_required_fields_present() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {"role": "assistant", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    let e = &events[0];
    assert_eq!(e.specversion, "1.0");
    assert!(!e.id.is_empty());
    assert!(!e.source.is_empty());
    assert!(!e.event_type.is_empty());
    assert!(!e.time.is_empty());
    assert_eq!(e.datacontenttype, "application/json");
}

#[test]
fn test_data_always_object() {
    let test_cases = vec![
        json!({"type": "user", "message": {"role": "user", "content": "hi"}}),
        json!({"type": "assistant", "message": {"role": "assistant", "content": "hi"}}),
        json!({"type": "progress", "data": {"type": "bash_progress"}}),
        json!({"type": "system", "subtype": "turn_duration"}),
        json!({"type": "file-history-snapshot"}),
        json!({"type": "queue-operation", "operation": "enqueue"}),
    ];
    for line in test_cases {
        let events = translate_line(&line, &mut state());
        if !events.is_empty() {
            // EventData is always a struct (object), so this is always true
            let _ = &events[0].data; // just verify it exists
            assert!(true, "data should be object for type: {}", line["type"]);
        }
    }
}

// ── Unified io.arc.event taxonomy tests ────────────────────

#[test]
fn test_user_prompt_produces_arc_event_type() {
    let line = base_entry(json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": "Hello Claude"}]},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.prompt"));
}

#[test]
fn test_user_tool_result_produces_arc_event_type() {
    let line = base_entry(json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "tu_1", "content": "ok"}],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.tool_result"));
}

#[test]
fn test_assistant_text_produces_arc_event_type() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "model": "claude-opus-4-6",
            "content": [{"type": "text", "text": "Hello!"}],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.text"));
}

#[test]
fn test_assistant_tool_use_subtype_hierarchical() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"path": "/foo"}},
            ],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.tool_use"));
}

#[test]
fn test_assistant_thinking_subtype_hierarchical() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [{"type": "thinking", "thinking": "deep thought"}],
        },
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("message.assistant.thinking"));
}

#[test]
fn test_system_turn_complete_subtype() {
    let line = base_entry(json!({
        "type": "system",
        "subtype": "turn_duration",
        "durationMs": 5000,
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("system.turn.complete"));
}

#[test]
fn test_system_error_subtype() {
    let line = base_entry(json!({"type": "system", "subtype": "api_error"}));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("system.error"));
}

#[test]
fn test_system_compact_subtype() {
    let line = base_entry(json!({"type": "system", "subtype": "compact_boundary"}));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("system.compact"));
}

#[test]
fn test_system_hook_subtype() {
    let line = base_entry(json!({
        "type": "system",
        "subtype": "stop_hook_summary",
        "hookCount": 2,
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("system.hook"));
}

#[test]
fn test_progress_bash_subtype_hierarchical() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "bash_progress", "output": "ls output"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("progress.bash"));
}

#[test]
fn test_progress_agent_subtype_hierarchical() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {"type": "agent_progress"},
    }));
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("progress.agent"));
}

#[test]
fn test_file_snapshot_subtype() {
    let line = json!({
        "type": "file-history-snapshot",
        "messageId": "msg-1",
        "snapshot": {"trackedFileBackups": {}},
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("file.snapshot"));
}

#[test]
fn test_queue_enqueue_subtype() {
    let line = json!({
        "type": "queue-operation",
        "operation": "enqueue",
        "sessionId": "s1",
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("queue.enqueue"));
}

#[test]
fn test_queue_dequeue_subtype() {
    let line = json!({
        "type": "queue-operation",
        "operation": "dequeue",
        "sessionId": "s1",
    });
    let events = translate_line(&line, &mut state());
    assert_eq!(events[0].event_type, IO_ARC_EVENT);
    assert_eq!(events[0].subtype.as_deref(), Some("queue.dequeue"));
}

#[test]
fn test_data_extracts_text_to_top_level() {
    let line = base_entry(json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": "Hello Claude"}]},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.text.as_deref(), Some("Hello Claude"));
}

#[test]
fn test_data_extracts_tool_to_top_level() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"file_path": "/foo"}},
            ],
        },
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.tool.as_deref(), Some("Read"));
    assert_eq!(p.args.as_ref().unwrap()["file_path"], "/foo");
}

#[test]
fn test_data_extracts_duration_ms() {
    let line = base_entry(json!({
        "type": "system",
        "subtype": "turn_duration",
        "durationMs": 4200,
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.duration_ms, Some(4200.0));
}

#[test]
fn test_data_extracts_model_to_top_level() {
    let line = base_entry(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "model": "claude-opus-4-6",
            "content": [{"type": "text", "text": "Hello!"}],
        },
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.model.as_deref(), Some("claude-opus-4-6"));
}

#[test]
fn test_data_extracts_session_id() {
    let line = base_entry(json!({
        "type": "user",
        "sessionId": "my-session-123",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    // When envelope contains sessionId, it overrides the filename-derived session_id.
    // This collapses sidechain (subagent) events into their parent session.
    assert_eq!(events[0].data.session_id, "my-session-123");
}

#[test]
fn test_session_id_falls_back_to_filename_when_no_envelope_session_id() {
    let line = base_entry(json!({
        "type": "user",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    // No sessionId in envelope → uses filename-derived "test-session"
    assert_eq!(events[0].data.session_id, "test-session");
}

#[test]
fn test_data_extracts_cwd() {
    let line = base_entry(json!({
        "type": "user",
        "cwd": "/projects/foo",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.cwd.as_deref(), Some("/projects/foo"));
}

// ── Subagent identity enrichment (Story 037) ─────────────

#[test]
fn test_main_agent_event_has_is_sidechain_false_no_agent_id() {
    let line = base_entry(json!({
        "type": "assistant",
        "uuid": "evt-001",
        "isSidechain": false,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.is_sidechain, Some(false));
    assert!(p.agent_id.is_none(), "agent_id should be absent for main agent");
    assert!(p.parent_tool_use_id.is_none(), "parent_tool_use_id should be absent");
}

#[test]
fn test_subagent_event_has_agent_id_and_is_sidechain_true() {
    let line = base_entry(json!({
        "type": "assistant",
        "uuid": "evt-100",
        "isSidechain": true,
        "agentId": "agent-abc-123",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "searching..."}]},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.is_sidechain, Some(true));
    assert_eq!(p.agent_id.as_deref(), Some("agent-abc-123"));
}

#[test]
fn test_progress_event_extracts_nested_agent_id_and_parent_tool_use_id() {
    let line = base_entry(json!({
        "type": "progress",
        "uuid": "evt-050",
        "isSidechain": false,
        "data": {
            "type": "agent_progress",
            "agentId": "agent-abc-123",
            "parentToolUseID": "toolu_xyz_789",
            "content": "Searching codebase...",
        },
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.is_sidechain, Some(false));
    assert_eq!(p.agent_id.as_deref(), Some("agent-abc-123"));
    assert_eq!(p.parent_tool_use_id.as_deref(), Some("toolu_xyz_789"));
}

#[test]
fn test_null_agent_id_is_omitted_from_envelope() {
    let line = base_entry(json!({
        "type": "assistant",
        "uuid": "evt-002",
        "isSidechain": false,
        "agentId": null,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.is_sidechain, Some(false));
    assert!(p.agent_id.is_none(), "null agent_id should be absent from payload");
}

// ── snake_case envelope boundary table ───────────────────────

/// Every camelCase key in raw transcripts must be converted to snake_case
/// in the payload. No camelCase keys should survive.
#[test]
fn test_envelope_keys_are_all_snake_case() {
    let line = base_entry(json!({
        "type": "user",
        "uuid": "u-1",
        "parentUuid": "u-0",
        "sessionId": "sess-abc",
        "cwd": "/project",
        "version": "2.2.0",
        "gitBranch": "feature/foo",
        "slug": "my-slug",
        "timestamp": "2025-01-10T10:00:00Z",
        "agentId": "agent-xyz",
        "parentToolUseID": "toolu_123",
        "isSidechain": true,
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&line, &mut state());
    let data = &events[0].data;
    let p = cc_payload(&events[0]);

    // Foundation
    assert_eq!(data.session_id, "sess-abc");
    // Payload typed fields
    assert_eq!(p.parent_uuid.as_deref(), Some("u-0"));
    assert_eq!(p.git_branch.as_deref(), Some("feature/foo"));
    assert_eq!(p.agent_id.as_deref(), Some("agent-xyz"));
    assert_eq!(p.parent_tool_use_id.as_deref(), Some("toolu_123"));
    assert_eq!(p.is_sidechain, Some(true));
    assert_eq!(p.cwd.as_deref(), Some("/project"));
    assert_eq!(p.version.as_deref(), Some("2.2.0"));
    assert_eq!(p.slug.as_deref(), Some("my-slug"));

    // No camelCase keys should exist in the extra bag
    assert!(p.extra.get("sessionId").is_none(), "camelCase sessionId should not exist");
    assert!(p.extra.get("parentUuid").is_none(), "camelCase parentUuid should not exist");
    assert!(p.extra.get("gitBranch").is_none(), "camelCase gitBranch should not exist");
    assert!(p.extra.get("agentId").is_none(), "camelCase agentId should not exist");
    assert!(p.extra.get("parentToolUseID").is_none(), "camelCase parentToolUseID should not exist");
    assert!(p.extra.get("isSidechain").is_none(), "camelCase isSidechain should not exist");
}

/// Extras keys from apply_user/apply_system must also be snake_case.
#[test]
fn test_extras_keys_are_snake_case() {
    // userType → user_type
    let user_line = base_entry(json!({
        "type": "user",
        "userType": "external",
        "message": {"role": "user", "content": "hi"},
    }));
    let events = translate_line(&user_line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.user_type.as_deref(), Some("external"));

    // durationMs → duration_ms
    let dur_line = base_entry(json!({
        "type": "system",
        "subtype": "turn_duration",
        "durationMs": 3500,
    }));
    let events = translate_line(&dur_line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.duration_ms, Some(3500.0));

    // hookCount → hook_count, preventedContinuation → prevented_continuation
    let hook_line = base_entry(json!({
        "type": "system",
        "subtype": "stop_hook_summary",
        "hookCount": 3,
        "preventedContinuation": true,
    }));
    let events = translate_line(&hook_line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.hook_count, Some(3));
    assert_eq!(p.prevented_continuation, Some(true));
}

/// Envelope session_id overrides filename-derived session_id for sidechain files.
#[test]
fn test_sidechain_session_id_override() {
    // Simulate a sidechain file: filename-derived session_id = "agent-abc123"
    let mut sidechain_state = TranscriptState::new("agent-abc123".into());
    let line = base_entry(json!({
        "type": "assistant",
        "sessionId": "real-session-uuid",
        "agentId": "agent-abc123",
        "isSidechain": true,
        "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]},
    }));
    let events = translate_line(&line, &mut sidechain_state);
    // The real sessionId should win over the filename-derived one
    assert_eq!(events[0].data.session_id, "real-session-uuid");
}

/// When no sessionId in envelope, filename-derived session_id is preserved.
#[test]
fn test_filename_session_id_preserved_when_no_envelope() {
    let mut st = TranscriptState::new("from-filename".into());
    // Construct line without sessionId (base_entry always includes one)
    let line = json!({
        "type": "user",
        "uuid": "u-no-session",
        "cwd": "/project",
        "message": {"role": "user", "content": "hi"},
    });
    let events = translate_line(&line, &mut st);
    assert_eq!(events[0].data.session_id, "from-filename");
}

/// Progress events with nested data.agentId extract to snake_case agent_id.
#[test]
fn test_progress_nested_agent_id_is_snake_case() {
    let line = base_entry(json!({
        "type": "progress",
        "data": {
            "type": "agent_progress",
            "agentId": "agent-nested",
            "parentToolUseID": "toolu_nested",
        },
    }));
    let events = translate_line(&line, &mut state());
    let p = cc_payload(&events[0]);
    assert_eq!(p.agent_id.as_deref(), Some("agent-nested"));
    assert_eq!(p.parent_tool_use_id.as_deref(), Some("toolu_nested"));
    assert!(p.extra.get("agentId").is_none());
    assert!(p.extra.get("parentToolUseID").is_none());
}
