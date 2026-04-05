//! Probability-class tests for the eval-apply detector.
//!
//! These tests are driven by REAL session data extracted from OpenStory.
//! Each test covers one probability class — a distinct pattern of events
//! observed in production sessions.
//!
//! The fixture file `tests/fixtures/turn_probability_classes.json` contains
//! one representative turn per class, extracted from real sessions via
//! `scripts/analyze_turn_shapes.py`.
//!
//! Probability classes (from 33 sessions, 129 complete turns):
//!
//!   tool_then_text      90.0%  — tool use followed by text summary
//!   multi_tool          82.9%  — multiple tool dispatches per turn
//!   multi_user_prompt   50.4%  — multiple user messages within a turn
//!   with_thinking       41.1%  — reasoning blocks present
//!   pure_text           10.9%  — no tools, just conversation
//!   single_tool          7.0%  — exactly one tool dispatch
//!
//! The detector must handle ALL of these correctly. If it only handles
//! single_tool (7%), it's broken for 93% of real usage.

use std::collections::HashMap;

use serde_json::Value;

use open_story_core::cloud_event::CloudEvent;
use open_story::patterns::{Detector, EvalApplyDetector};

/// Load the fixture file and parse into class → events map.
fn load_fixtures() -> HashMap<String, Vec<CloudEvent>> {
    let raw = include_str!("fixtures/turn_probability_classes.json");
    let parsed: HashMap<String, Value> = serde_json::from_str(raw)
        .expect("fixture file should be valid JSON");

    let mut classes = HashMap::new();
    for (class_name, class_data) in &parsed {
        let events_raw = class_data
            .get("events")
            .and_then(|v| v.as_array())
            .expect("each class should have events array");

        let events: Vec<CloudEvent> = events_raw
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();

        classes.insert(class_name.clone(), events);
    }
    classes
}

/// Feed a sequence of CloudEvents through the detector, return
/// (pattern_events, completed_turns).
fn feed_sequence(
    events: &[CloudEvent],
) -> (Vec<open_story::patterns::PatternEvent>, Vec<open_story::patterns::StructuralTurn>) {
    let mut det = EvalApplyDetector::new();
    let mut all_patterns = Vec::new();

    for event in events {
        all_patterns.extend(det.feed_cloud_event(event));
    }

    let mut turns = det.take_completed_turns();

    // Also flush in case turn didn't end with system.turn.complete
    let (flushed_patterns, flushed_turns) = {
        let p = det.flush();
        let t = det.flush_turns();
        (p, t)
    };
    all_patterns.extend(flushed_patterns);
    turns.extend(flushed_turns);

    (all_patterns, turns)
}

fn fixture_shape(class_name: &str, fixtures: &HashMap<String, Vec<CloudEvent>>) -> String {
    let raw = include_str!("fixtures/turn_probability_classes.json");
    let parsed: HashMap<String, Value> = serde_json::from_str(raw).unwrap();
    parsed[class_name]["shape"]
        .as_str()
        .unwrap_or("?")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: pure_text (10.9%)
// Shape: prompt -> text -> complete
// The simplest case. User asks, model responds, done.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pure_text_produces_terminal_turn_with_no_applies() {
    let fixtures = load_fixtures();
    let events = fixtures.get("pure_text").expect("fixture should have pure_text class");
    assert!(!events.is_empty(), "pure_text fixture should have events (got 0 — deserialization failed?)");

    let (patterns, turns) = feed_sequence(events);

    assert!(
        !turns.is_empty(),
        "pure_text ({}) should produce at least one turn",
        fixture_shape("pure_text", &fixtures)
    );

    let turn = &turns[0];
    assert!(turn.applies.is_empty(), "pure_text should have no applies");
    assert!(turn.is_terminal, "pure_text should be terminal");
    assert!(
        turn.human.is_some(),
        "pure_text should capture human content"
    );
    assert!(turn.eval.is_some(), "pure_text should capture eval content");

    // Should emit eval pattern
    assert!(
        patterns.iter().any(|p| p.pattern_type == "eval_apply.eval"),
        "should emit eval_apply.eval"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: single_tool (7.0%)
// Shape: prompt -> tool_use -> tool_result -> text -> complete
// One eval-apply cycle. The basic unit.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn single_tool_produces_turn_with_one_apply() {
    let fixtures = load_fixtures();
    let events = fixtures.get("single_tool").expect("fixture should have single_tool class");
    assert!(!events.is_empty(), "single_tool fixture should have events");

    let (patterns, turns) = feed_sequence(events);

    assert!(!turns.is_empty(), "single_tool should produce a turn");
    let turn = &turns[0];
    assert_eq!(
        turn.applies.len(),
        1,
        "single_tool should have exactly one apply, got {}",
        turn.applies.len()
    );
    assert!(
        !turn.applies[0].tool_name.is_empty(),
        "apply should have a tool name"
    );

    // Should emit both eval and apply patterns
    assert!(
        patterns.iter().any(|p| p.pattern_type == "eval_apply.eval"),
        "should emit eval"
    );
    assert!(
        patterns.iter().any(|p| p.pattern_type == "eval_apply.apply"),
        "should emit apply"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: multi_tool (82.9%)
// Shape: prompt -> tool_use -> tool_result -> tool_use -> tool_result -> ... -> text -> complete
// Multiple eval-apply cycles. THE DOMINANT CASE.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn multi_tool_produces_turn_with_multiple_applies() {
    let fixtures = load_fixtures();
    let events = fixtures.get("multi_tool").expect("fixture should have multi_tool class");
    assert!(!events.is_empty(), "multi_tool fixture should have events");

    let (patterns, turns) = feed_sequence(events);

    assert!(!turns.is_empty(), "multi_tool should produce a turn");
    let turn = &turns[0];
    assert!(
        turn.applies.len() > 1,
        "multi_tool should have multiple applies, got {}",
        turn.applies.len()
    );

    // Each apply should have a tool name
    for (i, apply) in turn.applies.iter().enumerate() {
        assert!(
            !apply.tool_name.is_empty(),
            "apply[{i}] should have a tool name"
        );
    }

    // Should emit multiple apply patterns
    let apply_count = patterns
        .iter()
        .filter(|p| p.pattern_type == "eval_apply.apply")
        .count();
    assert!(
        apply_count > 1,
        "multi_tool should emit multiple apply patterns, got {apply_count}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: tool_then_text (90.0%)
// Shape: prompt -> tool_use -> tool_result -> text -> complete
// Tool use followed by text summary. Nearly every tool turn.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn tool_then_text_has_eval_content_from_text_message() {
    let fixtures = load_fixtures();
    let events = fixtures.get("tool_then_text").expect("fixture should have tool_then_text class");
    assert!(!events.is_empty(), "tool_then_text fixture should have events");

    let (_patterns, turns) = feed_sequence(events);

    assert!(!turns.is_empty(), "tool_then_text should produce a turn");
    let turn = &turns[0];

    // The eval content should come from the text message (the summary)
    assert!(
        turn.eval.is_some(),
        "tool_then_text should have eval content"
    );

    // Should have at least one apply
    assert!(
        !turn.applies.is_empty(),
        "tool_then_text should have applies"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: with_thinking (41.1%)
// Shape: prompt -> thinking -> text -> complete (or with tools)
// Reasoning blocks present. 40% of turns!
// ═══════════════════════════════════════════════════════════════════

#[test]
fn with_thinking_turn_is_recognized() {
    let fixtures = load_fixtures();
    let events = fixtures.get("with_thinking").expect("fixture should have with_thinking class");
    assert!(!events.is_empty(), "with_thinking fixture should have events");

    let (_patterns, turns) = feed_sequence(events);

    assert!(!turns.is_empty(), "with_thinking should produce a turn");
    let turn = &turns[0];

    // Should have human and eval content
    assert!(turn.human.is_some(), "should have human content");
    assert!(turn.eval.is_some(), "should have eval content");

    // TODO: once thinking is captured on StructuralTurn, assert it's present
}

// ═══════════════════════════════════════════════════════════════════
// Probability class: multi_user_prompt (50.4%)
// Shape: prompt -> text -> prompt -> text -> complete
// Multiple user messages within one turn. HALF OF ALL TURNS.
// This is the case the Scheme model didn't anticipate.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn multi_user_prompt_produces_turn() {
    let fixtures = load_fixtures();
    let events = fixtures.get("multi_user_prompt").expect("fixture should have multi_user_prompt class");
    assert!(!events.is_empty(), "multi_user_prompt fixture should have events");

    let (_patterns, turns) = feed_sequence(events);

    // Should produce at least one turn (might produce multiple
    // if the detector treats each user_message as a turn boundary)
    assert!(
        !turns.is_empty(),
        "multi_user_prompt should produce at least one turn"
    );

    // The human content should capture the user's message
    let has_human = turns.iter().any(|t| t.human.is_some());
    assert!(has_human, "at least one turn should have human content");
}

// ═══════════════════════════════════════════════════════════════════
// Cross-cutting: every class should deserialize from real data
// ═══════════════════════════════════════════════════════════════════

#[test]
fn all_fixture_classes_deserialize_successfully() {
    let raw = include_str!("fixtures/turn_probability_classes.json");
    let parsed: HashMap<String, Value> = serde_json::from_str(raw).unwrap();

    let expected_classes = [
        "pure_text",
        "single_tool",
        "multi_tool",
        "tool_then_text",
        "with_thinking",
        "multi_user_prompt",
    ];

    for class_name in &expected_classes {
        assert!(
            parsed.contains_key(*class_name),
            "fixture should contain class: {class_name}"
        );

        let events_raw = parsed[*class_name]["events"].as_array().unwrap();
        let deserialized: Vec<Result<CloudEvent, _>> = events_raw
            .iter()
            .map(|v| serde_json::from_value::<CloudEvent>(v.clone()))
            .collect();

        let success_count = deserialized.iter().filter(|r| r.is_ok()).count();
        let fail_count = deserialized.iter().filter(|r| r.is_err()).count();

        assert!(
            success_count > 0,
            "class {class_name}: should deserialize at least some events ({fail_count} failed, 0 succeeded)"
        );

        if fail_count > 0 {
            let first_err = deserialized.iter().find(|r| r.is_err()).unwrap();
            eprintln!(
                "  warning: {class_name}: {fail_count}/{} events failed to deserialize: {:?}",
                events_raw.len(),
                first_err.as_ref().err()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Cross-cutting: detector should not panic on any real data
// ═══════════════════════════════════════════════════════════════════

#[test]
fn detector_does_not_panic_on_any_fixture_class() {
    let fixtures = load_fixtures();

    for (class_name, events) in &fixtures {
        // This should never panic regardless of input shape
        let (_patterns, _turns) = feed_sequence(events);
        // If we get here without panicking, the class is handled
    }
}
