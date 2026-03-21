//! Tests for incremental file reader with partial-line handling.

use std::fs;
use std::io::Write;

use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;
use serde_json::json;
use tempfile::TempDir;

fn state() -> TranscriptState {
    TranscriptState::new("test-session".to_string())
}

#[test]
fn test_reads_all_lines_from_start() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    let lines = vec![
        json!({"type": "user", "message": {"role": "user", "content": "hello"}}),
        json!({"type": "assistant", "message": {"role": "assistant", "content": "hi"}}),
    ];
    let content: String = lines.iter().map(|l| format!("{}\n", l)).collect();
    fs::write(&path, &content).unwrap();

    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "io.arc.event");
    assert_eq!(events[1].event_type, "io.arc.event");
}

#[test]
fn test_incremental_read_only_new_lines() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");

    let line1 = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "first"}}));
    fs::write(&path, &line1).unwrap();

    let mut s = state();
    let events1 = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events1.len(), 1);

    // Append a new line
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(f, "{}", json!({"type": "user", "message": {"role": "user", "content": "second"}})).unwrap();

    let events2 = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].data["raw"]["message"]["content"], "second");
}

#[test]
fn test_byte_offset_tracked() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    let line = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "hi"}}));
    fs::write(&path, &line).unwrap();

    let mut s = state();
    assert_eq!(s.byte_offset, 0);
    read_new_lines(&path, &mut s).unwrap();
    assert!(s.byte_offset > 0);
    assert_eq!(s.line_count, 1);
}

#[test]
fn test_missing_file_returns_empty() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nonexistent.jsonl");
    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert!(events.is_empty());
}

#[test]
fn test_invalid_json_lines_skipped() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    let content = format!(
        "not json\n{}\n",
        json!({"type": "user", "message": {"role": "user", "content": "ok"}})
    );
    fs::write(&path, &content).unwrap();

    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn test_seq_numbers_continuous_across_reads() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    let line = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "a"}}));
    fs::write(&path, &line).unwrap();

    let mut s = state();
    let events1 = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events1[0].data["seq"], 1);

    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(f, "{}", json!({"type": "user", "message": {"role": "user", "content": "b"}})).unwrap();

    let events2 = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events2[0].data["seq"], 2);
}

#[test]
fn test_partial_line_not_consumed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");

    // Write a partial line (no trailing newline)
    fs::write(&path, r#"{"type":"us"#).unwrap();

    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert!(events.is_empty(), "partial line should produce no events");
    assert_eq!(s.byte_offset, 0, "byte_offset should not advance for partial line");

    // Now complete the line
    fs::write(&path, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n").unwrap();

    let events = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events.len(), 1, "completed line should produce one event");
    assert!(s.byte_offset > 0);
}

#[test]
fn test_empty_file_returns_empty() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    fs::write(&path, "").unwrap();

    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert!(events.is_empty());
}

#[test]
fn test_blank_lines_skipped() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.jsonl");
    let content = format!(
        "\n\n{}\n\n",
        json!({"type": "user", "message": {"role": "user", "content": "hi"}})
    );
    fs::write(&path, &content).unwrap();

    let mut s = state();
    let events = read_new_lines(&path, &mut s).unwrap();
    assert_eq!(events.len(), 1);
}
