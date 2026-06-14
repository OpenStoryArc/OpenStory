//! Codex rollout JSONL -> CloudEvents via the reader pipeline.
//!
//! Fixture lines are minimized from the active Codex session rollout shape:
//! `session_meta`, `turn_context`, `event_msg`, and `response_item`.

use std::io::Write;

use open_story_core::event_data::{AgentPayload, CodexPayload};
use open_story_core::reader::read_new_lines;
use open_story_core::translate::{TranscriptFormat, TranscriptState};
use tempfile::NamedTempFile;

fn codex_payload(event: &open_story_core::cloud_event::CloudEvent) -> &CodexPayload {
    match event.data.agent_payload.as_ref().expect("agent payload") {
        AgentPayload::Codex(payload) => payload,
        _ => panic!("expected Codex payload"),
    }
}

#[test]
fn reader_detects_codex_rollout_and_translates_current_session_shapes() {
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.894Z","type":"session_meta","payload":{{"id":"019e54bf-aa76-7d03-b3f4-a2571d0c2117","timestamp":"2026-05-23T12:11:47.715Z","cwd":"/Users/maxglassie/projects/codex","originator":"codex-tui","cli_version":"0.133.0","source":"cli","thread_source":"user","model_provider":"openai"}}}}"#
    )
    .expect("write session_meta");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.903Z","type":"turn_context","payload":{{"turn_id":"019e54bf-d231-7462-86b5-e3b4ca1cb27e","cwd":"/Users/maxglassie/projects/codex","current_date":"2026-05-23","timezone":"America/New_York","model":"gpt-5.5"}}}}"#
    )
    .expect("write turn_context");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.908Z","type":"event_msg","payload":{{"type":"user_message","message":"Can you find the session data for this current codex session?","images":[],"local_images":[],"text_elements":[]}}}}"#
    )
    .expect("write user_message");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:00.552Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"pwd\",\"workdir\":\"/Users/maxglassie/projects/codex\",\"yield_time_ms\":1000,\"max_output_tokens\":2000}}","call_id":"call_KOBy4QUYLSBcz03uCr2E9EpT"}}}}"#
    )
    .expect("write function_call");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:00.614Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"call_KOBy4QUYLSBcz03uCr2E9EpT","output":"Output:\n/Users/maxglassie/projects/codex\n"}}}}"#
    )
    .expect("write function_call_output");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:01.896Z","type":"event_msg","payload":{{"type":"agent_message","message":"Current Codex session data is in ~/.codex/sessions/...jsonl.","phase":"final_answer","memory_citation":null}}}}"#
    )
    .expect("write agent_message");
    file.flush().expect("flush");

    let mut state = TranscriptState::new("fixture-session".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read should succeed");

    assert_eq!(state.format, TranscriptFormat::Codex);
    assert_eq!(events.len(), 6);

    let subtypes: Vec<&str> = events
        .iter()
        .map(|event| event.subtype.as_deref().unwrap_or("none"))
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "system.session_start",
            "system.turn.context",
            "message.user.prompt",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.text",
        ]
    );

    for event in &events {
        assert_eq!(event.agent.as_deref(), Some("codex"));
        assert!(event.source.starts_with("codex://session/"));
        assert_eq!(event.event_type, "io.arc.event");
    }

    let session = codex_payload(&events[0]);
    assert_eq!(
        session.thread_id.as_deref(),
        Some("019e54bf-aa76-7d03-b3f4-a2571d0c2117")
    );
    assert_eq!(
        session.cwd.as_deref(),
        Some("/Users/maxglassie/projects/codex")
    );
    assert_eq!(session.originator.as_deref(), Some("codex-tui"));
    assert_eq!(events[0].data.raw["type"], "session_meta");

    let prompt = codex_payload(&events[2]);
    assert_eq!(
        prompt.text.as_deref(),
        Some("Can you find the session data for this current codex session?")
    );

    let tool_use = codex_payload(&events[3]);
    assert_eq!(tool_use.tool.as_deref(), Some("exec_command"));
    assert_eq!(
        tool_use.call_id.as_deref(),
        Some("call_KOBy4QUYLSBcz03uCr2E9EpT")
    );
    assert_eq!(tool_use.args.as_ref().expect("args")["cmd"], "pwd");

    let tool_result = codex_payload(&events[4]);
    assert_eq!(
        tool_result.call_id.as_deref(),
        Some("call_KOBy4QUYLSBcz03uCr2E9EpT")
    );
    assert!(tool_result
        .output
        .as_deref()
        .expect("output")
        .contains("/Users/maxglassie/projects/codex"));

    let agent = codex_payload(&events[5]);
    assert_eq!(agent.phase.as_deref(), Some("final_answer"));
    assert_eq!(
        agent.text.as_deref(),
        Some("Current Codex session data is in ~/.codex/sessions/...jsonl.")
    );
}

#[test]
fn session_meta_rekeys_subsequent_codex_events_to_thread_id() {
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.894Z","type":"session_meta","payload":{{"id":"019e54bf-aa76-7d03-b3f4-a2571d0c2117","timestamp":"2026-05-23T12:11:47.715Z","cwd":"/Users/maxglassie/projects/codex","originator":"codex-tui","cli_version":"0.133.0","source":"cli","thread_source":"user","model_provider":"openai"}}}}"#
    )
    .expect("write session_meta");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:11:57.908Z","type":"event_msg","payload":{{"type":"user_message","message":"Can you find the session data for this current codex session?","images":[],"local_images":[],"text_elements":[]}}}}"#
    )
    .expect("write user_message");
    file.flush().expect("flush");

    let mut state = TranscriptState::new(
        "rollout-2026-05-23T08-11-47-019e54bf-aa76-7d03-b3f4-a2571d0c2117".to_string(),
    );
    let events = read_new_lines(file.path(), &mut state).expect("read should succeed");

    assert_eq!(state.session_id, "019e54bf-aa76-7d03-b3f4-a2571d0c2117");
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .map(|event| event.data.session_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "019e54bf-aa76-7d03-b3f4-a2571d0c2117",
            "019e54bf-aa76-7d03-b3f4-a2571d0c2117"
        ]
    );
    assert!(events
        .iter()
        .all(|event| event.source == "codex://session/019e54bf-aa76-7d03-b3f4-a2571d0c2117"));
}

#[test]
fn reader_translates_codex_custom_tool_calls_from_apply_patch_shape() {
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:38:07.128Z","type":"response_item","payload":{{"type":"custom_tool_call","status":"completed","call_id":"call_apply","name":"apply_patch","input":"*** Begin Patch\n*** Update File: rs/core/src/lib.rs\n@@\n+pub mod translate_codex;\n*** End Patch"}}}}"#
    )
    .expect("write custom tool call");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T13:15:07.926Z","type":"response_item","payload":{{"type":"custom_tool_call_output","call_id":"call_apply","output":"Exit code: 0\nSuccess. Updated files.\n"}}}}"#
    )
    .expect("write custom tool output");
    file.flush().expect("flush");

    let mut state = TranscriptState::new("fixture-session".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read should succeed");

    assert_eq!(state.format, TranscriptFormat::Codex);
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].subtype.as_deref(),
        Some("message.assistant.tool_use")
    );
    assert_eq!(
        events[1].subtype.as_deref(),
        Some("message.user.tool_result")
    );

    let tool_use = codex_payload(&events[0]);
    assert_eq!(tool_use.item_type.as_deref(), Some("custom_tool_call"));
    assert_eq!(tool_use.tool.as_deref(), Some("apply_patch"));
    assert_eq!(tool_use.call_id.as_deref(), Some("call_apply"));
    assert!(tool_use
        .args
        .as_ref()
        .expect("input")
        .as_str()
        .expect("patch text")
        .contains("translate_codex"));

    let tool_result = codex_payload(&events[1]);
    assert_eq!(
        tool_result.item_type.as_deref(),
        Some("custom_tool_call_output")
    );
    assert_eq!(tool_result.call_id.as_deref(), Some("call_apply"));
    assert!(tool_result
        .output
        .as_deref()
        .expect("output")
        .contains("Success. Updated files."));
}

#[test]
fn task_complete_synthesizes_turn_complete_boundary() {
    // Codex never emits `system.turn.complete` natively; the translator
    // synthesizes one from `event_msg.task_complete` so the eval-apply
    // detector can crystallize StructuralTurns (same approach as pi-mono).
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:12:10.000Z","type":"event_msg","payload":{{"type":"task_complete","turn_id":"019e54bf-d231-7462-86b5-e3b4ca1cb27e","last_agent_message":"Done."}}}}"#
    )
    .expect("write task_complete");

    let mut state = TranscriptState::new("codex-turn-test".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read lines");
    assert_eq!(state.format, TranscriptFormat::Codex);
    assert_eq!(events.len(), 2, "task_complete + synthetic turn boundary");

    assert_eq!(events[0].subtype.as_deref(), Some("system.task.complete"));
    assert_eq!(events[1].subtype.as_deref(), Some("system.turn.complete"));
    assert_eq!(events[1].agent.as_deref(), Some("codex"));
    assert_eq!(
        codex_payload(&events[1]).text.as_deref(),
        Some("Done."),
        "boundary event carries the final assistant text"
    );
}

#[test]
fn compacted_rollout_item_maps_to_system_compact() {
    // Codex emits a `compacted` RolloutItem (codex CompactedItem shape:
    // { message, replacement_history?, window_id? }) when it auto-compacts
    // history. OpenStory lifts it to `system.compact` so the UI can mark the
    // boundary. NOTE: not exercised by the lab runs — the local responses-API
    // backend returns no token usage, so codex auto-compaction never arms —
    // so this branch is covered here from the real codex type shape.
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"timestamp":"2026-05-23T12:30:00.000Z","type":"compacted","payload":{{"message":"Summary of earlier turns: explored the repo and summarized each script.","window_id":3}}}}"#
    )
    .expect("write compacted");

    let mut state = TranscriptState::new("codex-compacted".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read lines");
    assert_eq!(state.format, TranscriptFormat::Codex);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].subtype.as_deref(), Some("system.compact"));
    assert_eq!(events[0].agent.as_deref(), Some("codex"));
    assert_eq!(events[0].data.raw["type"], "compacted");
    assert_eq!(events[0].data.raw["payload"]["window_id"], 3);
}
