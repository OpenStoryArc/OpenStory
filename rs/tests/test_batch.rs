//! Fixture validation and local corpus tests.

use std::path::PathBuf;

use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

#[test]
fn test_synthetic_fixture_all_types() {
    let path = fixtures_dir().join("synthetic.jsonl");
    assert!(path.exists(), "synthetic.jsonl fixture must exist");

    let mut state = TranscriptState::new("synthetic".to_string());
    let events = read_new_lines(&path, &mut state).unwrap();

    // Should have events (the unknown type line produces none)
    assert!(!events.is_empty(), "should produce events from synthetic fixture");

    // All events should use the unified io.arc.event type
    let subtypes: std::collections::HashSet<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();

    // Should have subtypes from each category
    assert!(subtypes.iter().any(|s| s.starts_with("message.user.")), "missing user subtypes");
    assert!(subtypes.iter().any(|s| s.starts_with("message.assistant.")), "missing assistant subtypes");
    assert!(subtypes.iter().any(|s| s.starts_with("progress.")), "missing progress subtypes");
    assert!(subtypes.iter().any(|s| s.starts_with("system.")), "missing system subtypes");
    assert!(subtypes.iter().any(|s| s.starts_with("file.")), "missing file subtypes");
    assert!(subtypes.iter().any(|s| s.starts_with("queue.")), "missing queue subtypes");

    // All events should be valid CloudEvents with unified type
    for e in &events {
        assert_eq!(e.specversion, "1.0");
        assert!(!e.id.is_empty());
        assert!(e.source.starts_with("arc://transcript/"));
        assert_eq!(e.event_type, "io.arc.event");
        assert!(!e.time.is_empty());
        assert_eq!(e.datacontenttype, "application/json");
        assert!(e.data.is_object());
        assert!(e.data.get("raw").is_some(), "data.raw must be present");
    }
}

/// Validate a real session fixture: all events are valid CloudEvents with multiple types.
fn assert_real_session(filename: &str, label: &str) {
    let path = fixtures_dir().join(filename);
    assert!(path.exists(), "{} fixture must exist", filename);

    let session_id = filename.trim_end_matches(".jsonl");
    let mut state = TranscriptState::new(session_id.to_string());
    let events = read_new_lines(&path, &mut state).unwrap();

    assert!(!events.is_empty(), "{}: expected events", label);

    for e in &events {
        assert_eq!(e.specversion, "1.0");
        assert!(e.source.starts_with("arc://transcript/"));
        assert_eq!(e.event_type, "io.arc.event");
        assert!(e.data.is_object());
        assert!(e.data.get("raw").is_some());
    }

    let subtypes: std::collections::HashSet<Option<&str>> = events.iter().map(|e| e.subtype.as_deref()).collect();
    assert!(subtypes.len() >= 2, "{}: expected multiple subtypes, got: {:?}", label, subtypes);
    eprintln!("{}: {} events, {} subtypes", label, events.len(), subtypes.len());
}

#[test]
fn test_real_session_origin() {
    assert_real_session("synth_origin.jsonl", "origin");
}

#[test]
fn test_real_session_hooks() {
    assert_real_session("synth_hooks.jsonl", "hooks + dashboard");
}

#[test]
fn test_real_session_global() {
    assert_real_session("synth_global.jsonl", "global hooks, performance");
}

#[test]
fn test_real_session_translator() {
    assert_real_session("synth_translator.jsonl", "transcript-to-CloudEvents");
}

#[test]
#[ignore]
fn test_local_corpus() {
    // Glob ~/.claude/projects/**/*.jsonl and translate all, assert no panics
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .expect("HOME or USERPROFILE must be set");
    let claude_dir = PathBuf::from(home).join(".claude").join("projects");

    if !claude_dir.exists() {
        eprintln!("Skipping: ~/.claude/projects not found");
        return;
    }

    let mut total_events = 0u64;
    let mut total_files = 0u64;

    for entry in walkdir::WalkDir::new(&claude_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        total_files += 1;
        let session_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
        let mut state = TranscriptState::new(session_id.to_string());
        let events = read_new_lines(path, &mut state).unwrap();
        total_events += events.len() as u64;

        // Every event must be valid
        for e in &events {
            assert_eq!(e.specversion, "1.0");
            assert!(e.data.is_object());
        }
    }

    eprintln!("Local corpus: {} files, {} events", total_files, total_events);
}
