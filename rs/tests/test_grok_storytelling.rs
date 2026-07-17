//! End-to-end: Grok Build ACP → translator → eval-apply → sentence.
//!
//! Mirrors `test_pi_mono_storytelling.rs`. Proves synthetic
//! `system.turn.complete` from `turn_completed` drives StructuralTurns
//! and `turn.sentence` patterns with subject "Grok".

use open_story_core::reader::read_new_lines;
use open_story_core::translate::{TranscriptFormat, TranscriptState};
use open_story_patterns::PatternPipeline;
use std::path::PathBuf;

fn grok_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/grok")
        .join(name)
}

fn assert_story_from_fixture(name: &str) {
    let path = grok_fixture(name);
    assert!(path.exists(), "missing fixture {}", path.display());

    let mut state = TranscriptState::new("grok-storytelling".to_string());
    let events = read_new_lines(&path, &mut state).expect("read fixture");
    assert_eq!(state.format, TranscriptFormat::Grok);

    let turn_completes: Vec<_> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("system.turn.complete"))
        .collect();
    assert!(
        !turn_completes.is_empty(),
        "{name}: translate_grok must emit system.turn.complete (got {} events)",
        events.len()
    );

    for e in &events {
        assert_eq!(
            e.agent.as_deref(),
            Some("grok-build"),
            "{name}: every event must carry agent=grok-build"
        );
    }

    let mut pipeline = PatternPipeline::new();
    let mut all_turns = Vec::new();
    let mut all_patterns = Vec::new();
    for ev in &events {
        let (patterns, turns) = pipeline.feed_event(ev);
        all_patterns.extend(patterns);
        all_turns.extend(turns);
    }

    assert!(
        !all_turns.is_empty(),
        "{name}: expected ≥1 StructuralTurn, got 0 from {} events",
        events.len()
    );

    let sentences: Vec<_> = all_patterns
        .iter()
        .filter(|p| p.pattern_type == "turn.sentence")
        .collect();
    assert!(
        !sentences.is_empty(),
        "{name}: expected ≥1 turn.sentence, got {} turns and 0 sentences",
        all_turns.len()
    );

    let sentence = &sentences[0];
    assert!(
        sentence.summary.starts_with("Grok"),
        "{name}: sentence must start with 'Grok', got: {}",
        sentence.summary
    );
    assert!(
        sentence.summary.chars().count() >= 20,
        "{name}: sentence too short: {}",
        sentence.summary
    );

    eprintln!("\n{name} → {}", sentence.summary);
}

#[test]
fn grok_scenario_01_text_only_produces_sentence() {
    assert_story_from_fixture("scenario_01_text_only.jsonl");
}

#[test]
fn grok_scenario_02_single_tool_produces_sentence() {
    assert_story_from_fixture("scenario_02_single_tool.jsonl");
}

#[test]
fn grok_scenario_03_multi_tool_produces_sentence() {
    assert_story_from_fixture("scenario_03_multi_tool.jsonl");
}

#[test]
fn grok_scenario_04_error_recovery_produces_sentence() {
    assert_story_from_fixture("scenario_04_error_recovery.jsonl");
}

#[test]
fn grok_scenario_05_edit_and_test_produces_sentence() {
    assert_story_from_fixture("scenario_05_edit_and_test.jsonl");
}

// ── Seeds extracted from the live Grok session that built this feature ──
//
// Source: ~/.grok/sessions/…/019f6cb5-…/updates.jsonl
// (see fixtures/grok/SESSION_SEED.md). Truncated tool outputs; wire shape real.

#[test]
fn grok_real_turn_01_text_only_produces_sentence() {
    assert_story_from_fixture("real_turn_01_text_only.jsonl");
}

#[test]
fn grok_real_turn_02_session_storage_produces_sentence() {
    assert_story_from_fixture("real_turn_02_session_storage.jsonl");
}

#[test]
fn grok_real_turn_07_openstory_vs_grok_produces_sentence() {
    assert_story_from_fixture("real_turn_07_openstory_vs_grok.jsonl");
}

#[test]
fn grok_real_turn_09_acp_and_mcp_produces_sentence() {
    assert_story_from_fixture("real_turn_09_acp_and_mcp.jsonl");
}
