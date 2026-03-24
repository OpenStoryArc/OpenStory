//! Integration tests: pi-mono JSONL → CloudEvents via reader pipeline.
//!
//! Validates the end-to-end path: file on disk → read_new_lines → format
//! detection → translate_pi_line → CloudEvents with correct subtypes.

use std::io::Write;

use open_story::reader::read_new_lines;
use open_story::translate::{TranscriptFormat, TranscriptState};
use tempfile::NamedTempFile;

/// Read the pi-mono fixture and verify end-to-end translation.
#[test]
fn reader_detects_pi_mono_and_translates() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono_session.jsonl");

    let mut state = TranscriptState::new("pi-test-session".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read should succeed");

    // Format should be detected as PiMono
    assert_eq!(state.format, TranscriptFormat::PiMono);

    // 10 lines in fixture, all should translate
    assert_eq!(events.len(), 10, "expected 10 events from fixture");

    // Verify subtypes in order
    let subtypes: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or("none"))
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "system.session_start",
            "message.user.prompt",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.thinking",  // has thinking + text, thinking wins
            "system.model_change",
            "message.user.prompt",
            "progress.bash",
            "system.compact",
            "message.assistant.text",
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
    assert_eq!(events[0].data["provider"], "anthropic");
    assert_eq!(events[0].data["model"], "claude-sonnet-4-5");
    assert_eq!(events[0].data["cwd"], "/work/my-project");

    // User message text (index 1)
    assert_eq!(
        events[1].data["text"],
        "Read the config file and explain it"
    );

    // Tool use (index 2)
    assert_eq!(events[2].data["tool"], "read");
    assert_eq!(events[2].data["args"]["path"], "/work/config.toml");
    // Raw is untouched — preserves pi-mono's native toolCall type
    assert_eq!(
        events[2].data["raw"]["message"]["content"][1]["type"],
        "toolCall"
    );
    // Agent field identifies the source
    assert_eq!(events[2].agent.as_deref(), Some("pi-mono"));

    // Tool result (index 3)
    assert_eq!(events[3].data["tool_name"], "read");
    assert_eq!(events[3].data["is_error"], false);

    // Model change (index 5)
    assert_eq!(events[5].data["provider"], "openai");
    assert_eq!(events[5].data["model"], "gpt-4o");

    // Bash execution (index 7)
    assert_eq!(events[7].data["command"], "cargo test");
    assert_eq!(events[7].data["exit_code"], 0);

    // Compaction (index 8)
    assert_eq!(
        events[8].data["summary"],
        "Read config file and explained TOML structure"
    );
}
