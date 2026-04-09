//! Integration tests for the Hermes translator.
//!
//! Phase A: read the fixture JSONL (produced by `generate_session.py`)
//! and feed it through `translate_hermes_line()`. Verifies that the
//! translator handles the canonical Hermes message shapes from a "real"
//! session (synthesized by the fixture script using verified shapes from
//! SOURCE_VERIFICATION.md §4).
//!
//! Phase B (testcontainer): builds and runs the `hermes-fixture:test`
//! Docker image, reads the generated JSONL from the container, and
//! verifies the same way. This closes runtime gaps by using the
//! container's own Python to produce the data.
//!
//! Build the Docker image first:
//!   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
//!
//! Run:
//!   cargo test -p open-story --test test_hermes_translator

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;
use open_story_core::translate::TranscriptState;
use open_story_core::translate_hermes::{is_hermes_format, translate_hermes_line};
use serde_json::Value;
use std::path::PathBuf;

/// Load the fixture JSONL and translate every line.
fn load_and_translate_fixture() -> (Vec<CloudEvent>, Vec<Value>) {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_plugin.jsonl");
    let content = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("fixture not found at {:?}: {}", fixture_path, e));

    let mut state = TranscriptState::new("fixture-session".to_string());
    let mut events: Vec<CloudEvent> = Vec::new();
    let mut raw_lines: Vec<Value> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("bad JSON in fixture: {}", e));
        raw_lines.push(parsed.clone());
        events.extend(translate_hermes_line(&parsed, &mut state));
    }

    (events, raw_lines)
}

/// Helper: extract HermesPayload from a CloudEvent.
fn hermes_payload(event: &CloudEvent) -> &open_story_core::event_data::HermesPayload {
    match event.data.agent_payload.as_ref().expect("missing agent_payload") {
        AgentPayload::Hermes(p) => p,
        other => panic!("expected Hermes payload, got {:?}", other),
    }
}

// ── Phase A: static fixture tests ────────────────────────────────────

#[test]
fn fixture_is_detected_as_hermes_format() {
    let (_, raw_lines) = load_and_translate_fixture();
    for (i, line) in raw_lines.iter().enumerate() {
        assert!(
            is_hermes_format(line),
            "line {} not detected as Hermes format: {}",
            i,
            serde_json::to_string_pretty(line).unwrap()
        );
    }
}

#[test]
fn fixture_produces_correct_event_count() {
    let (events, _) = load_and_translate_fixture();
    // 6 input lines should produce 7 CloudEvents:
    //   session_start, user prompt, thinking, tool_use, tool_result,
    //   assistant text, turn complete
    assert_eq!(
        events.len(),
        7,
        "expected 7 CloudEvents from 6 fixture lines, got {}",
        events.len()
    );
}

#[test]
fn fixture_subtypes_are_in_expected_order() {
    let (events, _) = load_and_translate_fixture();
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.subtype.as_deref())
        .collect();
    assert_eq!(
        subtypes,
        vec![
            "system.session.start",
            "message.user.prompt",
            "message.assistant.thinking",
            "message.assistant.tool_use",
            "message.user.tool_result",
            "message.assistant.text",
            "system.turn.complete",
        ]
    );
}

#[test]
fn fixture_all_events_carry_hermes_agent_tag() {
    let (events, _) = load_and_translate_fixture();
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(
            ev.agent.as_deref(),
            Some("hermes"),
            "event {} missing hermes agent tag",
            i
        );
    }
}

#[test]
fn fixture_session_start_has_model_and_tools() {
    let (events, _) = load_and_translate_fixture();
    let start = &events[0];
    assert_eq!(start.subtype.as_deref(), Some("system.session.start"));
    let p = hermes_payload(start);
    assert_eq!(p.model.as_deref(), Some("mock-model"));
    assert_eq!(p.platform.as_deref(), Some("cli"));
    assert!(p.tools.is_some());
    let tools = p.tools.as_ref().unwrap();
    assert!(tools.contains(&"Bash".to_string()));
}

#[test]
fn fixture_user_prompt_text_is_preserved() {
    let (events, _) = load_and_translate_fixture();
    let user = &events[1];
    assert_eq!(user.subtype.as_deref(), Some("message.user.prompt"));
    let p = hermes_payload(user);
    assert!(p.text.as_deref().unwrap().contains("files"));
}

#[test]
fn fixture_thinking_contains_reasoning() {
    let (events, _) = load_and_translate_fixture();
    let thinking = &events[2];
    assert_eq!(
        thinking.subtype.as_deref(),
        Some("message.assistant.thinking")
    );
    let p = hermes_payload(thinking);
    assert!(p.reasoning.as_deref().unwrap().contains("directory listing"));
}

#[test]
fn fixture_tool_use_has_parsed_args() {
    let (events, _) = load_and_translate_fixture();
    let tool_use = &events[3];
    assert_eq!(
        tool_use.subtype.as_deref(),
        Some("message.assistant.tool_use")
    );
    let p = hermes_payload(tool_use);
    assert_eq!(p.tool.as_deref(), Some("Bash"));
    assert_eq!(p.tool_use_id.as_deref(), Some("toolu_fixture_001"));
    // arguments must be parsed from JSON string → structured Value
    let args = p.args.as_ref().unwrap();
    assert_eq!(args["command"], "ls -la");
    // preceding_text should be the assistant content before the tool call
    assert_eq!(
        p.preceding_text.as_deref(),
        Some("I'll list the files for you.")
    );
}

#[test]
fn fixture_tool_result_links_correctly() {
    let (events, _) = load_and_translate_fixture();
    let tool_result = &events[4];
    assert_eq!(
        tool_result.subtype.as_deref(),
        Some("message.user.tool_result")
    );
    let p = hermes_payload(tool_result);
    // tool_call_id links back to the tool_use
    assert_eq!(p.tool_call_id.as_deref(), Some("toolu_fixture_001"));
    // tool_name is present in this fixture (runtime gap #2: verify with real Hermes)
    assert_eq!(p.tool_name.as_deref(), Some("Bash"));
    // tool accessor should mirror tool_name
    assert_eq!(p.tool.as_deref(), Some("Bash"));
    // content is the tool output
    assert!(p.text.as_deref().unwrap().contains("README.md"));
}

#[test]
fn fixture_final_text_has_stop_reason() {
    let (events, _) = load_and_translate_fixture();
    let text = &events[5];
    assert_eq!(text.subtype.as_deref(), Some("message.assistant.text"));
    let p = hermes_payload(text);
    assert_eq!(p.stop_reason.as_deref(), Some("stop"));
    assert!(p.text.as_deref().unwrap().contains("README.md"));
}

#[test]
fn fixture_turn_complete_has_completion_state() {
    let (events, _) = load_and_translate_fixture();
    let tc = &events[6];
    assert_eq!(tc.subtype.as_deref(), Some("system.turn.complete"));
    let p = hermes_payload(tc);
    assert_eq!(p.reason.as_deref(), Some("end_turn"));
    assert_eq!(p.completed, Some(true));
    assert_eq!(p.interrupted, Some(false));
    assert_eq!(p.message_count, Some(5));
}

#[test]
fn fixture_event_ids_are_deterministic() {
    let (events1, _) = load_and_translate_fixture();
    let (events2, _) = load_and_translate_fixture();
    let ids1: Vec<&str> = events1.iter().map(|e| e.id.as_str()).collect();
    let ids2: Vec<&str> = events2.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids1, ids2, "event IDs must be stable across translation passes");
}

#[test]
fn fixture_event_ids_are_unique() {
    let (events, _) = load_and_translate_fixture();
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate event IDs found: {:?}",
        ids
    );
}

// ── Snapshot format test (runtime gap #1) ────────────────────────────

#[test]
fn snapshot_timestamp_format_is_iso8601_with_offset() {
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_snapshot.json");
    let content = std::fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|e| panic!("snapshot fixture not found: {}", e));
    let snapshot: Value = serde_json::from_str(&content).unwrap();

    // Runtime gap #1: verify that Hermes uses ISO-8601 with offset.
    // Python's datetime.now(timezone.utc).isoformat() produces
    // something like "2026-04-09T10:58:52.075214+00:00" — NOT a
    // trailing "Z", and WITH microseconds.
    let ts = snapshot["session_start"]
        .as_str()
        .expect("session_start should be a string");
    assert!(
        ts.contains('+') || ts.ends_with('Z'),
        "timestamp should have timezone info: {}",
        ts
    );
    // Should have sub-second precision
    assert!(
        ts.contains('.'),
        "timestamp should have microseconds: {}",
        ts
    );
    // The translator passes timestamps through unchanged — so the
    // plugin's UTC-Z timestamps (from time.strftime) will work fine.
    // The snapshot timestamps (from datetime.isoformat) are different
    // but also valid ISO-8601.
}

// ── Snapshot top-level shape test ────────────────────────────────────

#[test]
fn snapshot_has_expected_top_level_fields() {
    let snapshot_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes/session_snapshot.json");
    let content = std::fs::read_to_string(&snapshot_path).unwrap();
    let snapshot: Value = serde_json::from_str(&content).unwrap();

    // Verified shape from run_agent.py:2450-2461
    assert!(snapshot["session_id"].is_string());
    assert!(snapshot["model"].is_string());
    assert!(snapshot["platform"].is_string());
    assert!(snapshot["session_start"].is_string());
    assert!(snapshot["last_updated"].is_string());
    assert!(snapshot["system_prompt"].is_string());
    assert!(snapshot["tools"].is_array());
    assert!(snapshot["message_count"].is_number());
    assert!(snapshot["messages"].is_array());

    let messages = snapshot["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        snapshot["message_count"].as_u64().unwrap() as usize
    );
}

// ── Phase B: testcontainer tests ─────────────────────────────────────
// These require the Docker image to be built:
//   docker build -t hermes-fixture:test rs/tests/fixtures/hermes/
//
// They are skipped when the image is not available (same pattern as
// the open-story:test container tests).

#[cfg(feature = "hermes_container")]
mod container_tests {
    use super::*;
    use testcontainers::core::Mount;
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{GenericImage, ImageExt};

    #[tokio::test]
    async fn container_produces_valid_jsonl() {
        let image = GenericImage::new("hermes-fixture", "test");
        let tmp = tempfile::tempdir().unwrap();
        let output_mount = Mount::bind_mount(
            tmp.path().to_str().unwrap(),
            "/output",
        );

        let container = image
            .with_mount(output_mount)
            .start()
            .await
            .expect("failed to start hermes-fixture container");

        // Wait for it to exit (it's a one-shot script)
        container
            .stop()
            .await
            .expect("container didn't stop cleanly");

        // Read the JSONL from the temp dir
        let jsonl_path = tmp.path().join("session_plugin.jsonl");
        assert!(
            jsonl_path.exists(),
            "container should have written session_plugin.jsonl"
        );

        let content = std::fs::read_to_string(&jsonl_path).unwrap();
        let mut state = TranscriptState::new("container-test".to_string());
        let mut events: Vec<CloudEvent> = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: Value = serde_json::from_str(line).unwrap();
            assert!(is_hermes_format(&parsed));
            events.extend(translate_hermes_line(&parsed, &mut state));
        }

        // Same structural checks as the static fixture tests
        assert_eq!(events.len(), 7);
        let subtypes: Vec<&str> = events
            .iter()
            .filter_map(|e| e.subtype.as_deref())
            .collect();
        assert_eq!(
            subtypes,
            vec![
                "system.session.start",
                "message.user.prompt",
                "message.assistant.thinking",
                "message.assistant.tool_use",
                "message.user.tool_result",
                "message.assistant.text",
                "system.turn.complete",
            ]
        );
    }
}
