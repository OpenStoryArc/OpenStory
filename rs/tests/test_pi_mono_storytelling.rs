//! End-to-end test: pi-mono → translator → eval-apply → sentence.
//!
//! Proves the recursion-principle fix from
//! `docs/research/architecture-audit/PRINCIPLES.md` is actually closed:
//! synthetic `system.turn.complete` events emitted by `decompose_assistant`
//! drive eval-apply to crystallize StructuralTurns that drive
//! SentenceDetector to render `turn.sentence` patterns.
//!
//! Before this fix, pi-mono sessions produced events but no story —
//! the live recursion test (`rs/tests/test_principle_recursive_observability.rs`)
//! caught it. This is the unit-level proof that the fix works.

use open_story_core::reader::read_new_lines;
use open_story_core::translate::TranscriptState;
use open_story_patterns::PatternPipeline;
use std::path::PathBuf;

fn pi_mono_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono")
        .join(name)
}

#[test]
fn pi_mono_scenario_06_produces_a_turn_sentence() {
    // scenario_06 is the canonical pi-mono shape:
    //   user prompt → assistant [thinking, text, toolCall] → tool result
    //                → assistant [text] (stopReason="stop")
    // The second assistant message ends the turn; the synthetic
    // turn.complete from translate_pi triggers eval-apply.
    let path = pi_mono_fixture("scenario_06_thinking_text_tool.jsonl");
    let mut state = TranscriptState::new("pi-storytelling-06".to_string());
    let events = read_new_lines(&path, &mut state).expect("read fixture");

    // Sanity: synthetic turn.complete is in the event stream
    let turn_completes: Vec<_> = events
        .iter()
        .filter(|e| e.subtype.as_deref() == Some("system.turn.complete"))
        .collect();
    assert!(
        !turn_completes.is_empty(),
        "translate_pi must emit at least one synthetic system.turn.complete \
         (the recursion-principle fix). Found {} events but no turn.complete.",
        events.len()
    );

    // Drive the patterns pipeline
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
        "pi-mono session must produce at least one StructuralTurn after the \
         synthetic turn.complete fires. Got {} events but 0 turns.",
        events.len()
    );

    let sentences: Vec<_> = all_patterns
        .iter()
        .filter(|p| p.pattern_type == "turn.sentence")
        .collect();
    assert!(
        !sentences.is_empty(),
        "pi-mono session must produce at least one turn.sentence pattern. \
         Got {} StructuralTurns but 0 sentences.",
        all_turns.len()
    );

    // The summary should look legible: contain the agent name "Pi"
    // (sentence builder uses "Pi" as the subject for pi-mono per
    // patterns/src/sentence.rs:118) and have a verb-like middle.
    let sentence = &sentences[0];
    assert!(
        sentence.summary.starts_with("Pi"),
        "pi-mono sentence must start with 'Pi' (the agent subject), got: {}",
        sentence.summary
    );
    assert!(
        sentence.summary.chars().count() >= 30,
        "pi-mono sentence must be substantive (>=30 chars), got: {}",
        sentence.summary
    );

    eprintln!("\npi-mono produced sentence: {}", sentence.summary);
}
