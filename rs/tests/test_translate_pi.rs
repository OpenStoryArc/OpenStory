//! Integration tests: pi-mono JSONL → CloudEvents via reader pipeline.
//!
//! Validates the end-to-end path: file on disk → read_new_lines → format
//! detection → translate_pi_line → CloudEvents with correct subtypes.

use std::io::Write;

use open_story::event_data::AgentPayload;
use open_story::reader::read_new_lines;
use open_story::translate::{TranscriptFormat, TranscriptState};
use tempfile::NamedTempFile;

/// Helper: extract PiMonoPayload from event, panicking if not present.
fn pi_payload(event: &open_story::cloud_event::CloudEvent) -> &open_story::event_data::PiMonoPayload {
    match event.data.agent_payload.as_ref().expect("agent_payload should be Some") {
        AgentPayload::PiMono(pm) => pm,
        _ => panic!("expected PiMono payload"),
    }
}

/// Read the pi-mono fixture and verify end-to-end translation.
#[test]
fn reader_detects_pi_mono_and_translates() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono_session.jsonl");

    let mut state = TranscriptState::new("pi-test-session".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read should succeed");

    // Format should be detected as PiMono
    assert_eq!(state.format, TranscriptFormat::PiMono);

    // 10 JSONL lines → 12 CloudEvents (2 lines decompose: [text,toolCall]→2, [thinking,text]→2)
    assert_eq!(events.len(), 12, "expected 12 events from fixture (decomposed)");

    // Verify subtypes in order
    let subtypes: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or("none"))
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "system.session_start",        // line 1: session
            "message.user.prompt",         // line 2: user
            "message.assistant.text",      // line 3: [text, toolCall] → decomposed
            "message.assistant.tool_use",  //   (second block from line 3)
            "message.user.tool_result",    // line 4: toolResult
            "message.assistant.thinking",  // line 5: [thinking, text] → decomposed
            "message.assistant.text",      //   (second block from line 5)
            "system.model_change",         // line 6: model_change
            "message.user.prompt",         // line 7: user
            "progress.bash",              // line 8: bashExecution
            "system.compact",             // line 9: compaction
            "message.assistant.text",      // line 10: [text] → 1 event
        ]
    );

    // All events use pi:// source
    for e in &events {
        assert!(
            e.source.starts_with("pi://session/"),
            "event source should use pi:// scheme: {}",
            e.source,
        );
    }

    // All events are io.arc.event
    for e in &events {
        assert_eq!(e.event_type, "io.arc.event");
    }
}

/// Verify that Claude Code files are still detected correctly (not pi-mono).
#[test]
fn reader_detects_claude_code_format() {
    let mut file = NamedTempFile::new().expect("create temp file");
    writeln!(
        file,
        r#"{{"type":"user","uuid":"abc-123","sessionId":"sess-1","message":{{"role":"user","content":[{{"type":"text","text":"hello"}}]}},"timestamp":"2025-01-01T00:00:00Z"}}"#
    )
    .expect("write");

    let mut state = TranscriptState::new("test-claude".to_string());
    let events = read_new_lines(file.path(), &mut state).expect("read should succeed");

    assert_eq!(state.format, TranscriptFormat::ClaudeCode);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].subtype.as_deref(), Some("message.user.prompt"));
    assert!(events[0].source.starts_with("arc://transcript/"));
}

/// Verify incremental reads work for pi-mono files.
#[test]
fn reader_incremental_read_pi_mono() {
    let mut file = NamedTempFile::new().expect("create temp file");

    // Write first line
    writeln!(
        file,
        r#"{{"type":"session","id":"s1","timestamp":"2025-01-01T00:00:00Z","cwd":"/work"}}"#
    )
    .expect("write");
    file.flush().expect("flush");

    let mut state = TranscriptState::new("test-incr".to_string());
    let events1 = read_new_lines(file.path(), &mut state).expect("first read");
    assert_eq!(events1.len(), 1);
    assert_eq!(state.format, TranscriptFormat::PiMono);

    // Append second line
    writeln!(
        file,
        r#"{{"type":"message","timestamp":"2025-01-01T00:00:01Z","message":{{"role":"user","content":"hello","timestamp":1}}}}"#
    )
    .expect("write");
    file.flush().expect("flush");

    let events2 = read_new_lines(file.path(), &mut state).expect("second read");
    assert_eq!(events2.len(), 1);
    assert_eq!(
        events2[0].subtype.as_deref(),
        Some("message.user.prompt")
    );

    // Format stays locked
    assert_eq!(state.format, TranscriptFormat::PiMono);
}

/// Verify field extraction on key event types.
#[test]
fn reader_pi_mono_field_extraction() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono_session.jsonl");

    let mut state = TranscriptState::new("pi-test-session".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read");

    // Session header (index 0)
    let p0 = pi_payload(&events[0]);
    assert_eq!(p0.provider.as_deref(), Some("anthropic"));
    assert_eq!(p0.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(p0.cwd.as_deref(), Some("/work/my-project"));

    // User message text (index 1)
    let p1 = pi_payload(&events[1]);
    assert_eq!(p1.text.as_deref(), Some("Read the config file and explain it"));

    // Decomposed: line 3 [text, toolCall] → index 2 is text, index 3 is tool_use
    let p2 = pi_payload(&events[2]);
    assert_eq!(p2.text.as_deref(), Some("I'll read the config file for you."));
    let p3 = pi_payload(&events[3]);
    assert_eq!(p3.tool.as_deref(), Some("read"));
    assert_eq!(p3.args.as_ref().unwrap()["path"], "/work/config.toml");
    // Raw is untouched — both decomposed events share the original bundled line
    assert_eq!(events[2].data.raw["message"]["content"][1]["type"], "toolCall");
    assert_eq!(events[3].data.raw["message"]["content"][1]["type"], "toolCall");
    // Agent field identifies the source
    assert_eq!(events[3].agent.as_deref(), Some("pi-mono"));

    // Tool result (index 4)
    let p4 = pi_payload(&events[4]);
    assert_eq!(p4.tool_name.as_deref(), Some("read"));
    assert_eq!(p4.is_error, Some(false));

    // Decomposed: line 5 [thinking, text] → index 5 is thinking, index 6 is text
    let p5 = pi_payload(&events[5]);
    assert_eq!(p5.text.as_deref(), Some("The config file is a TOML file with a server section containing host and port."));
    let p6 = pi_payload(&events[6]);
    assert_eq!(p6.text.as_deref(), Some("This is a TOML configuration file with a server section. It binds to localhost on port 3002."));

    // Model change (index 7)
    let p7 = pi_payload(&events[7]);
    assert_eq!(p7.provider.as_deref(), Some("openai"));
    assert_eq!(p7.model.as_deref(), Some("gpt-4o"));

    // Bash execution (index 9)
    let p9 = pi_payload(&events[9]);
    assert_eq!(p9.command.as_deref(), Some("cargo test"));
    assert_eq!(p9.exit_code, Some(serde_json::json!(0)));

    // Compaction (index 10)
    let p10 = pi_payload(&events[10]);
    assert_eq!(p10.summary.as_deref(), Some("Read config file and explained TOML structure"));
}

// ── Real captured scenario tests ─────────────────────────────────

/// Scenario 04: [thinking, text] produces both events (was invisible before).
#[test]
fn scenario_04_thinking_text_both_visible() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono/scenario_04_thinking_text.jsonl");

    let mut state = TranscriptState::new("s04".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read");

    let subtypes: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or("none"))
        .collect();

    assert!(subtypes.contains(&"message.assistant.thinking"), "thinking should be visible");
    assert!(subtypes.contains(&"message.assistant.text"), "text should be visible — THIS WAS THE BUG");

    // The text event should contain the actual answer
    let text_events: Vec<_> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.text"))
        .collect();
    assert!(!text_events.is_empty(), "should have text events");
    let p = pi_payload(text_events[0]);
    assert!(p.text.as_ref().unwrap().len() > 10, "text should have substantial content");
}

/// Scenario 06: [thinking, text, toolCall] — worst case, all three visible.
#[test]
fn scenario_06_full_decomposition() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono/scenario_06_thinking_text_tool.jsonl");

    let mut state = TranscriptState::new("s06".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read");

    let subtypes: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or("none"))
        .collect();

    assert!(subtypes.contains(&"message.assistant.thinking"), "thinking visible");
    assert!(subtypes.contains(&"message.assistant.text"), "text visible");
    assert!(subtypes.contains(&"message.assistant.tool_use"), "tool_use visible");
    assert!(subtypes.contains(&"message.user.tool_result"), "tool_result visible");

    // Should have at least 2 text events (one from bundled line, one from final response)
    let text_count = subtypes.iter().filter(|s| **s == "message.assistant.text").count();
    assert!(text_count >= 2, "expected >=2 text events, got {text_count}");
}

/// Scenario 07: [toolCall, toolCall] — both tools visible.
#[test]
fn scenario_07_multi_tool_both_visible() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono/scenario_07_multi_tool.jsonl");

    let mut state = TranscriptState::new("s07".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read");

    let tool_events: Vec<_> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("message.assistant.tool_use"))
        .collect();

    assert!(tool_events.len() >= 2, "expected >=2 tool_use events, got {}", tool_events.len());

    // Each tool has a unique tool_call_id
    let tool_ids: Vec<_> = tool_events.iter().map(|e| {
        pi_payload(e).tool_call_id.as_deref().unwrap_or("")
    }).collect();
    let unique: std::collections::HashSet<_> = tool_ids.iter().collect();
    assert_eq!(tool_ids.len(), unique.len(), "tool_call_ids should be unique");
}
