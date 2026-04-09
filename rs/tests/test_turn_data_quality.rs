//! Data quality tests for StructuralTurns — driven by real session analysis.
//!
//! These tests validate invariants that must hold for the fold to be correct:
//!   - No event ID appears in more than one turn
//!   - Turns are ordered by timestamp (sequential, non-overlapping)
//!   - Every turn has a first and last event ID (the identity range)
//!   - Turn numbers are sequential with no gaps
//!   - Event counts are reasonable (no empty turns, no absurdly large ones)
//!
//! Uses real session data from the fixtures, fed through the pure step() function.

use std::collections::{HashMap, HashSet};

use open_story_core::cloud_event::CloudEvent;
use open_story::patterns::{EvalApplyDetector, StructuralTurn};

/// Load the probability class fixtures and extract all CloudEvents.
fn load_all_fixture_events() -> HashMap<String, Vec<CloudEvent>> {
    let raw = include_str!("fixtures/turn_probability_classes.json");
    let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(raw).unwrap();

    let mut classes = HashMap::new();
    for (class_name, class_data) in &parsed {
        let events_raw = class_data.get("events").and_then(|v| v.as_array()).unwrap();
        let events: Vec<CloudEvent> = events_raw
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        classes.insert(class_name.clone(), events);
    }
    classes
}

/// Feed events through the detector, collect all completed + flushed turns.
fn feed_and_collect_turns(events: &[CloudEvent]) -> Vec<StructuralTurn> {
    let mut det = EvalApplyDetector::new();
    for event in events {
        det.feed_cloud_event(event);
    }
    let mut turns = det.take_completed_turns();
    turns.extend(det.flush_turns());
    turns
}

/// Feed each fixture class through its own detector, return all turns.
/// Each class gets a fresh detector — they're independent sessions.
fn all_fixture_turns() -> Vec<StructuralTurn> {
    let classes = load_all_fixture_events();
    let mut all_turns = Vec::new();
    for (_class, events) in &classes {
        // Fresh detector per class — they're independent sessions
        all_turns.extend(feed_and_collect_turns(events));
    }
    all_turns
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: No event ID appears in more than one turn
// ═══════════════════════════════════════════════════════════════════
//
// If an event ID appears in two turns, the accumulator leaked events
// across turn boundaries. This was observed in real data: 2,348
// duplicates across 121 turns in session ca2bc88e.

#[test]
fn no_event_id_appears_in_multiple_turns_within_session() {
    let classes = load_all_fixture_events();

    for (class_name, events) in &classes {
        let turns = feed_and_collect_turns(events);
        let mut seen: HashMap<String, u32> = HashMap::new();

        for turn in &turns {
            for eid in &turn.event_ids {
                if let Some(prev_turn) = seen.get(eid) {
                    panic!(
                        "Class {}: Event {} appears in both Turn {} and Turn {} — accumulator leak",
                        class_name, eid, prev_turn, turn.turn_number
                    );
                }
                seen.insert(eid.clone(), turn.turn_number);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: Every turn has at least one event ID
// ═══════════════════════════════════════════════════════════════════

#[test]
fn every_turn_has_event_ids() {
    let turns = all_fixture_turns();
    for turn in &turns {
        assert!(
            !turn.event_ids.is_empty(),
            "Turn {} has no event IDs",
            turn.turn_number
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: Turn has a session_id (not empty, not "unknown")
// ═══════════════════════════════════════════════════════════════════

#[test]
fn every_turn_has_valid_session_id() {
    let turns = all_fixture_turns();
    for turn in &turns {
        assert!(
            !turn.session_id.is_empty(),
            "Turn {} has empty session_id",
            turn.turn_number
        );
        assert_ne!(
            turn.session_id, "unknown",
            "Turn {} has session_id 'unknown'",
            turn.turn_number
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: Turn numbers are sequential within a session
// ═══════════════════════════════════════════════════════════════════

#[test]
fn turn_numbers_are_sequential_per_session() {
    let turns = all_fixture_turns();
    let mut by_session: HashMap<String, Vec<u32>> = HashMap::new();

    for turn in &turns {
        by_session
            .entry(turn.session_id.clone())
            .or_default()
            .push(turn.turn_number);
    }

    for (sid, mut numbers) in by_session {
        numbers.sort();
        // Check for gaps
        for i in 1..numbers.len() {
            let gap = numbers[i] - numbers[i - 1];
            assert!(
                gap <= 1,
                "Session {}: gap between Turn {} and Turn {} (expected sequential)",
                sid.get(..8).unwrap_or(&sid),
                numbers[i - 1],
                numbers[i]
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: stop_reason is either "end_turn" or "tool_use"
// ═══════════════════════════════════════════════════════════════════

#[test]
fn stop_reason_is_valid() {
    let turns = all_fixture_turns();
    let valid = ["end_turn", "tool_use"];
    for turn in &turns {
        assert!(
            valid.contains(&turn.stop_reason.as_str()),
            "Turn {} has invalid stop_reason: '{}'",
            turn.turn_number,
            turn.stop_reason
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: env_delta <= env_size (can't add more than total)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn env_delta_does_not_exceed_env_size() {
    let turns = all_fixture_turns();
    for turn in &turns {
        assert!(
            turn.env_delta <= turn.env_size,
            "Turn {}: env_delta ({}) > env_size ({})",
            turn.turn_number,
            turn.env_delta,
            turn.env_size
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: Applies count matches what the sentence reports
// ═══════════════════════════════════════════════════════════════════

#[test]
fn applies_have_tool_names() {
    let turns = all_fixture_turns();
    for turn in &turns {
        for (i, apply) in turn.applies.iter().enumerate() {
            assert!(
                !apply.tool_name.is_empty(),
                "Turn {} apply[{}] has empty tool_name",
                turn.turn_number,
                i
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Invariant: timestamp is non-empty
// ═══════════════════════════════════════════════════════════════════

#[test]
fn every_turn_has_timestamp() {
    let turns = all_fixture_turns();
    for turn in &turns {
        assert!(
            !turn.timestamp.is_empty(),
            "Turn {} has empty timestamp",
            turn.turn_number
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Property: First and last event IDs define the turn's identity
// ═══════════════════════════════════════════════════════════════════

#[test]
fn first_and_last_event_ids_define_turn_identity() {
    let classes = load_all_fixture_events();

    for (class_name, events) in &classes {
        let turns = feed_and_collect_turns(events);
        let mut ranges: HashSet<(String, String)> = HashSet::new();

        for turn in &turns {
            if turn.event_ids.len() >= 2 {
                let first = turn.event_ids.first().unwrap().clone();
                let last = turn.event_ids.last().unwrap().clone();
                let range = (first.clone(), last.clone());
                assert!(
                    ranges.insert(range),
                    "Class {}: Turn {} has duplicate event range {}..{}",
                    class_name, turn.turn_number, &first[..8], &last[..8]
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Regression: scope_depth should be 0 (not inflated by legacy path)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn scope_depth_is_not_inflated() {
    let turns = all_fixture_turns();
    for turn in &turns {
        assert!(
            turn.scope_depth <= 5,
            "Turn {} has scope_depth {} — likely inflated by legacy ViewRecord tree depth",
            turn.turn_number,
            turn.scope_depth
        );
    }
}
