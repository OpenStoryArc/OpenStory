//! E2E invariant test: subagent identity fields round-trip through the full pipeline.
//!
//! Story 037: agentId, parentToolUseID, isSidechain must flow from raw JSONL
//! through CloudEvent → ViewRecord with correct values.

mod helpers;

use std::fs;

use open_story::translate::{translate_line, TranscriptState};
use open_story_views::from_cloud_event::from_cloud_event;
use helpers::fixtures_dir;

/// Load synth_origin.jsonl, translate all lines, convert to ViewRecords,
/// and verify that subagent identity fields are preserved through the pipeline.
#[test]
fn real_fixture_preserves_is_sidechain_through_pipeline() {
    let fixture_path = fixtures_dir().join("synth_origin.jsonl");
    let content = fs::read_to_string(&fixture_path).expect("fixture exists");

    let mut state = TranscriptState::new("test-session".into());
    let mut total_view_records = 0;
    let mut with_is_sidechain_false = 0;
    let mut with_agent_id = 0;

    for line in content.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
        let cloud_events = translate_line(&value, &mut state);

        for ce in &cloud_events {
            let ce_value = serde_json::to_value(ce).expect("serialize CloudEvent");
            let view_records = from_cloud_event(&ce_value);

            for vr in &view_records {
                total_view_records += 1;

                // All events in this fixture have isSidechain=false
                assert_eq!(
                    vr.is_sidechain, false,
                    "synth_origin has no sidechain events, but record {} has is_sidechain=true",
                    vr.id
                );

                if !vr.is_sidechain {
                    with_is_sidechain_false += 1;
                }

                if vr.agent_id.is_some() {
                    with_agent_id += 1;
                }
            }
        }
    }

    // Sanity: we should have produced a meaningful number of ViewRecords
    assert!(
        total_view_records > 100,
        "expected >100 ViewRecords from fixture, got {}",
        total_view_records
    );
    assert_eq!(with_is_sidechain_false, total_view_records);
}

/// Verify that progress events with nested data.agentId are extracted
/// and flow through to ViewRecords.
#[test]
fn real_fixture_progress_events_carry_agent_id() {
    let fixture_path = fixtures_dir().join("synth_origin.jsonl");
    let content = fs::read_to_string(&fixture_path).expect("fixture exists");

    let mut state = TranscriptState::new("test-session".into());
    let mut progress_with_agent_id = 0;

    for line in content.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid JSON");

        // Check if this is a progress event with nested agentId
        let is_progress_with_nested = value.get("type").and_then(|v| v.as_str()) == Some("progress")
            && value
                .get("data")
                .and_then(|d| d.get("agentId"))
                .and_then(|v| v.as_str())
                .is_some();

        let cloud_events = translate_line(&value, &mut state);

        if is_progress_with_nested {
            for ce in &cloud_events {
                // Verify the agentId made it into CloudEvent.data
                assert!(
                    ce.data.get("agent_id").is_some(),
                    "progress event with nested data.agentId should have agent_id in CloudEvent.data"
                );

                // Convert to ViewRecords and verify agent_id is set
                let ce_value = serde_json::to_value(ce).expect("serialize CloudEvent");
                let view_records = from_cloud_event(&ce_value);
                for vr in &view_records {
                    assert!(
                        vr.agent_id.is_some(),
                        "ViewRecord from progress with data.agentId should have agent_id set"
                    );
                    progress_with_agent_id += 1;
                }
            }
        }
    }

    // The fixture has 21 progress events with nested agentId
    assert!(
        progress_with_agent_id > 0,
        "expected at least some progress events with agent_id, got 0"
    );
}

/// Synthetic test: subagent event with isSidechain=true and agentId round-trips.
#[test]
fn synthetic_subagent_event_round_trips() {
    let mut state = TranscriptState::new("test-session".into());
    let line = serde_json::json!({
        "type": "assistant",
        "uuid": "evt-sub-1",
        "parentUuid": "evt-main-1",
        "sessionId": "test-session",
        "isSidechain": true,
        "agentId": "agent-explore-abc",
        "message": {
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [{"type": "text", "text": "exploring..."}]
        }
    });

    let cloud_events = translate_line(&line, &mut state);
    assert_eq!(cloud_events.len(), 1);

    // Verify CloudEvent.data carries the fields
    let ce = &cloud_events[0];
    assert_eq!(ce.data["is_sidechain"], true);
    assert_eq!(ce.data["agent_id"], "agent-explore-abc");

    // Convert to ViewRecords
    let ce_value = serde_json::to_value(ce).expect("serialize CloudEvent");
    let view_records = from_cloud_event(&ce_value);
    assert_eq!(view_records.len(), 1);

    let vr = &view_records[0];
    assert_eq!(vr.agent_id, Some("agent-explore-abc".to_string()));
    assert_eq!(vr.is_sidechain, true);

    // Verify JSON serialization
    let json = serde_json::to_value(vr).expect("serialize ViewRecord");
    assert_eq!(json["agent_id"], "agent-explore-abc");
    assert_eq!(json["is_sidechain"], true);
}
