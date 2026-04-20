//! E2E tests: pi-mono JSONL → translator → views → ViewRecords.
//!
//! These tests verify that EVERY piece of data from a pi-mono session
//! makes it through the full pipeline into ViewRecords. They catch
//! rendering gaps where events exist but ViewRecords are missing or empty.
//!
//! Run with: cargo test -p open-story --test test_pi_mono_views_e2e

use open_story::event_data::AgentPayload;
use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use open_story_views::view_record::ViewRecord;

/// Read a pi-mono fixture and produce all ViewRecords.
fn fixture_to_records(fixture_name: &str) -> Vec<ViewRecord> {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono")
        .join(fixture_name);

    let mut state = TranscriptState::new("e2e-test".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read fixture");

    events.iter().flat_map(|e| from_cloud_event(e)).collect()
}

/// Count records by type.
fn count_by_type(records: &[ViewRecord]) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for r in records {
        let rt = match &r.body {
            RecordBody::UserMessage(_) => "user_message",
            RecordBody::AssistantMessage(_) => "assistant_message",
            RecordBody::ToolCall(_) => "tool_call",
            RecordBody::ToolResult(_) => "tool_result",
            RecordBody::Reasoning(_) => "reasoning",
            RecordBody::TokenUsage(_) => "token_usage",
            RecordBody::SystemEvent(_) => "system_event",
            RecordBody::TurnEnd(_) => "turn_end",
            _ => "other",
        };
        *counts.entry(rt.to_string()).or_insert(0) += 1;
    }
    counts
}

// ── Scenario: snake game (thinking + write + bash + text) ──────────

/// Every assistant text block produces an AssistantMessage ViewRecord.
#[test]
fn snake_game_assistant_text_visible() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let asst: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::AssistantMessage(_))).collect();
    assert!(
        !asst.is_empty(),
        "should have at least one AssistantMessage from the final text response"
    );
    // The final response should have non-empty content
    let has_content = asst.iter().any(|r| {
        if let RecordBody::AssistantMessage(a) = &r.body {
            a.content.iter().any(|b| match b {
                open_story_views::unified::ContentBlock::Text { text } => !text.is_empty(),
                _ => false,
            })
        } else {
            false
        }
    });
    assert!(has_content, "AssistantMessage should have non-empty text content");
}

/// The write tool call produces a ToolCall ViewRecord with name and input.
#[test]
fn snake_game_write_tool_call_visible() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let tools: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();

    // Should have both write and bash tool calls
    let tool_names: Vec<String> = tools.iter().filter_map(|r| {
        if let RecordBody::ToolCall(tc) = &r.body { Some(tc.name.clone()) } else { None }
    }).collect();

    assert!(
        tool_names.contains(&"write".to_string()),
        "should have a 'write' tool call, got: {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"bash".to_string()),
        "should have a 'bash' tool call, got: {:?}",
        tool_names
    );
}

/// The write tool call input contains actual code (not empty/null).
#[test]
fn snake_game_write_tool_has_code_content() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let write_call = records.iter().find(|r| {
        if let RecordBody::ToolCall(tc) = &r.body { tc.name == "write" } else { false }
    });
    assert!(write_call.is_some(), "should have a write tool call");

    if let RecordBody::ToolCall(tc) = &write_call.unwrap().body {
        let input = &tc.input;
        // write tool input should have a "content" or file content field
        let input_str = serde_json::to_string(input).unwrap();
        assert!(
            input_str.len() > 100,
            "write tool input should contain substantial code, got {} chars",
            input_str.len()
        );
    }
}

/// The write tool call has a non-empty call_id for linking to results.
#[test]
fn snake_game_write_tool_has_call_id() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let write_call = records.iter().find(|r| {
        if let RecordBody::ToolCall(tc) = &r.body { tc.name == "write" } else { false }
    });
    assert!(write_call.is_some(), "should have a write tool call");

    if let RecordBody::ToolCall(tc) = &write_call.unwrap().body {
        assert!(
            !tc.call_id.is_empty(),
            "write tool call_id should not be empty"
        );
    }
}

/// Thinking/reasoning is visible with actual text content.
#[test]
fn snake_game_reasoning_has_content() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let reasoning: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::Reasoning(_))).collect();
    assert!(!reasoning.is_empty(), "should have reasoning records");

    let has_content = reasoning.iter().any(|r| {
        if let RecordBody::Reasoning(r) = &r.body {
            r.content.as_ref().map_or(false, |c| !c.is_empty())
        } else {
            false
        }
    });
    assert!(has_content, "reasoning should have non-empty content text");
}

/// Tool results are visible.
#[test]
fn snake_game_tool_results_visible() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let results: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolResult(_))).collect();
    assert!(
        results.len() >= 2,
        "should have at least 2 tool results (write + bash), got {}",
        results.len()
    );
}

/// User prompt is visible with the full text.
#[test]
fn snake_game_user_prompt_visible() {
    let records = fixture_to_records("scenario_snake_game.jsonl");
    let user: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::UserMessage(_))).collect();
    assert!(!user.is_empty(), "should have user message");

    if let RecordBody::UserMessage(u) = &user[0].body {
        let text = match &u.content {
            open_story_views::unified::MessageContent::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert!(text.contains("snake"), "user prompt should contain 'snake'");
    }
}

/// No data is lost: every CloudEvent produces at least one ViewRecord.
#[test]
fn snake_game_no_events_dropped() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi_mono/scenario_snake_game.jsonl");

    let mut state = TranscriptState::new("e2e-test".to_string());
    let events = read_new_lines(&fixture, &mut state).expect("read fixture");

    let mut events_with_records = 0;
    let mut events_without_records = Vec::new();

    for e in &events {
        let records = from_cloud_event(e);
        if records.is_empty() {
            let subtype = e.subtype.as_deref().unwrap_or("none");
            // system events (session_start, model_change) may not produce records — that's OK
            if !subtype.starts_with("system.") {
                events_without_records.push(subtype.to_string());
            }
        } else {
            events_with_records += 1;
        }
    }

    assert!(
        events_without_records.is_empty(),
        "these event subtypes produced NO ViewRecords: {:?}",
        events_without_records
    );
}

// ── Scenario 04: thinking + text ────────────────────────────────────

/// [thinking, text] line produces BOTH reasoning AND assistant_message records.
#[test]
fn scenario_04_both_records_present() {
    let records = fixture_to_records("scenario_04_thinking_text.jsonl");
    let counts = count_by_type(&records);

    assert!(
        counts.get("reasoning").copied().unwrap_or(0) >= 1,
        "should have reasoning records, counts: {:?}", counts
    );
    assert!(
        counts.get("assistant_message").copied().unwrap_or(0) >= 1,
        "should have assistant_message records — THIS WAS THE CORE BUG, counts: {:?}", counts
    );
}

// ── Scenario 06: thinking + text + tool ─────────────────────────────

/// [thinking, text, toolCall] line produces reasoning + assistant_message + tool_call.
#[test]
fn scenario_06_all_three_record_types() {
    let records = fixture_to_records("scenario_06_thinking_text_tool.jsonl");
    let counts = count_by_type(&records);

    assert!(
        counts.get("reasoning").copied().unwrap_or(0) >= 1,
        "should have reasoning, counts: {:?}", counts
    );
    assert!(
        counts.get("assistant_message").copied().unwrap_or(0) >= 1,
        "should have assistant_message, counts: {:?}", counts
    );
    assert!(
        counts.get("tool_call").copied().unwrap_or(0) >= 1,
        "should have tool_call, counts: {:?}", counts
    );
    assert!(
        counts.get("tool_result").copied().unwrap_or(0) >= 1,
        "should have tool_result, counts: {:?}", counts
    );
}

/// Tool call in scenario 06 has correct name (read) and non-empty args.
#[test]
fn scenario_06_tool_call_has_detail() {
    let records = fixture_to_records("scenario_06_thinking_text_tool.jsonl");
    let tool = records.iter().find(|r| matches!(&r.body, RecordBody::ToolCall(_)));
    assert!(tool.is_some(), "should have tool call");

    if let RecordBody::ToolCall(tc) = &tool.unwrap().body {
        assert_eq!(tc.name, "read", "tool name should be 'read'");
        assert!(!tc.call_id.is_empty(), "call_id should not be empty");
        assert!(!tc.input.is_null(), "input should not be null");
    }
}

// ── Scenario 07: multi-tool ─────────────────────────────────────────

/// Both parallel tool calls produce separate ToolCall records.
#[test]
fn scenario_07_both_tools_have_records() {
    let records = fixture_to_records("scenario_07_multi_tool.jsonl");
    let tools: Vec<_> = records.iter().filter(|r| matches!(&r.body, RecordBody::ToolCall(_))).collect();

    assert!(
        tools.len() >= 2,
        "should have >= 2 tool call records for parallel reads, got {}",
        tools.len()
    );

    // Each should have a unique call_id
    let ids: Vec<String> = tools.iter().filter_map(|r| {
        if let RecordBody::ToolCall(tc) = &r.body { Some(tc.call_id.clone()) } else { None }
    }).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len(), "call_ids should be unique: {:?}", ids);
}
