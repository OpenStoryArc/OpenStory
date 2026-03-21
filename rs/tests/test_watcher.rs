//! Integration tests for file watcher.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use open_story::translate::TranscriptState;
use open_story::watcher::backfill;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn test_backfill_existing_files() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("session1.jsonl");

    let content = format!(
        "{}\n{}\n",
        json!({"type": "user", "message": {"role": "user", "content": "hello"}}),
        json!({"type": "assistant", "message": {"role": "assistant", "content": "hi"}}),
    );
    fs::write(&path, &content).unwrap();

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    let count = backfill(dir.path(), &mut states, None, false).unwrap();
    assert_eq!(count, 2);
    assert!(states.contains_key(&path));
}

#[test]
fn test_backfill_multiple_files() {
    let dir = TempDir::new().unwrap();

    for name in &["a.jsonl", "b.jsonl"] {
        let path = dir.path().join(name);
        let content = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "hi"}}));
        fs::write(&path, &content).unwrap();
    }

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    let count = backfill(dir.path(), &mut states, None, false).unwrap();
    assert_eq!(count, 2);
    assert_eq!(states.len(), 2);
}

#[test]
fn test_backfill_ignores_non_jsonl() {
    let dir = TempDir::new().unwrap();

    fs::write(
        dir.path().join("notes.txt"),
        "not a jsonl file\n",
    ).unwrap();
    fs::write(
        dir.path().join("data.jsonl"),
        format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "hi"}})),
    ).unwrap();

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    let count = backfill(dir.path(), &mut states, None, false).unwrap();
    assert_eq!(count, 1);
    assert_eq!(states.len(), 1);
}

#[test]
fn test_backfill_nested_directories() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("project").join("subdir");
    fs::create_dir_all(&nested).unwrap();

    let path = nested.join("session.jsonl");
    let content = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "nested"}}));
    fs::write(&path, &content).unwrap();

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    let count = backfill(dir.path(), &mut states, None, false).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_backfill_with_output_file() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("session.jsonl");
    let output = dir.path().join("output.jsonl");

    let content = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "hi"}}));
    fs::write(&input, &content).unwrap();

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    let count = backfill(dir.path(), &mut states, Some(&output), false).unwrap();
    assert_eq!(count, 1);

    // Verify output file was written (excluding itself from processing)
    let output_content = fs::read_to_string(&output).unwrap();
    assert!(output_content.contains("io.arc.event"));
}

#[test]
fn test_session_id_from_filename() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("my-cool-session.jsonl");
    let content = format!("{}\n", json!({"type": "user", "message": {"role": "user", "content": "hi"}}));
    fs::write(&path, &content).unwrap();

    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
    backfill(dir.path(), &mut states, None, false).unwrap();

    let state = states.get(&path).unwrap();
    assert_eq!(state.session_id, "my-cool-session");
}
