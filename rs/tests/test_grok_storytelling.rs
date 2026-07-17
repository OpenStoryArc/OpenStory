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

/// Multi-turn stability: four real extracted turns in one pipeline.
/// Locks turn.sentence count + agent tag + eval-apply shape so regressions
/// in translate_grok or patterns show up without Docker.
#[test]
fn grok_real_turns_multi_turn_pattern_shape_is_stable() {
    let files = [
        "real_turn_01_text_only.jsonl",
        "real_turn_02_session_storage.jsonl",
        "real_turn_07_openstory_vs_grok.jsonl",
        "real_turn_09_acp_and_mcp.jsonl",
    ];

    let mut state = TranscriptState::new("grok-multi-turn".to_string());
    let mut events = Vec::new();
    for name in files {
        let path = grok_fixture(name);
        assert!(path.exists(), "missing {}", path.display());
        // Each fixture is a separate file; reset byte_offset so we re-read
        // from the start while keeping format + uuid dedupe across turns.
        state.byte_offset = 0;
        let batch = read_new_lines(&path, &mut state).expect("read fixture");
        events.extend(batch);
    }
    assert_eq!(state.format, TranscriptFormat::Grok);
    assert!(
        events.len() >= 20,
        "expected multi-turn event volume, got {}",
        events.len()
    );

    for e in &events {
        assert_eq!(e.agent.as_deref(), Some("grok-build"));
    }

    let turn_completes = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("system.turn.complete"))
        .count();
    assert_eq!(
        turn_completes, 4,
        "four real turns → four turn.complete (got {turn_completes})"
    );

    let mut pipeline = PatternPipeline::new();
    let mut all_patterns = Vec::new();
    let mut all_turns = Vec::new();
    for ev in &events {
        let (patterns, turns) = pipeline.feed_event(ev);
        all_patterns.extend(patterns);
        all_turns.extend(turns);
    }

    assert_eq!(all_turns.len(), 4, "expected 4 StructuralTurns");

    let mut type_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for p in &all_patterns {
        *type_counts.entry(p.pattern_type.as_str()).or_default() += 1;
    }

    let sentences: Vec<_> = all_patterns
        .iter()
        .filter(|p| p.pattern_type == "turn.sentence")
        .collect();
    assert_eq!(sentences.len(), 4, "one sentence per real turn");
    for s in &sentences {
        assert!(
            s.summary.starts_with("Grok"),
            "sentence must start with Grok: {}",
            s.summary
        );
    }

    // Exact shape for these four frozen real-turn extracts (golden counts).
    // Bump deliberately if fixtures or the coalgebra change.
    assert_eq!(
        type_counts.get("eval_apply.eval").copied().unwrap_or(0),
        24,
        "eval_apply.eval: {type_counts:?}"
    );
    assert_eq!(
        type_counts.get("eval_apply.apply").copied().unwrap_or(0),
        17,
        "eval_apply.apply: {type_counts:?}"
    );
    assert_eq!(
        type_counts.get("eval_apply.turn_end").copied().unwrap_or(0),
        4,
        "eval_apply.turn_end: {type_counts:?}"
    );
    assert_eq!(
        type_counts.get("turn.sentence").copied().unwrap_or(0),
        4
    );

    eprintln!("multi-turn pattern_type_counts: {type_counts:?}");
    for s in sentences {
        eprintln!("  • {}", s.summary);
    }
}
