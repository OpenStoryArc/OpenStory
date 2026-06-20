//! Codex CloudEvents flow into the same typed view records as other adapters.

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, CodexPayload, EventData};
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::{ContentBlock, MessageContent, RecordBody};
use serde_json::json;

fn codex_event(subtype: &str, payload: CodexPayload) -> CloudEvent {
    CloudEvent::new(
        "codex://session/test".to_string(),
        "io.arc.event".to_string(),
        EventData::with_payload(
            json!({"type": "response_item", "payload": {"type": payload.item_type.clone()}}),
            1,
            "test-session".to_string(),
            AgentPayload::Codex(payload),
        ),
        Some(subtype.to_string()),
        Some(format!("event-{subtype}")),
        Some("2026-05-23T12:00:00Z".to_string()),
        None,
        None,
        Some("codex".to_string()),
    )
}

#[test]
fn codex_tool_use_and_result_become_view_records() {
    let mut tool_use = CodexPayload::new();
    tool_use.item_type = Some("function_call".to_string());
    tool_use.tool = Some("exec_command".to_string());
    tool_use.call_id = Some("call_1".to_string());
    tool_use.args = Some(json!({"cmd": "pwd"}));

    let records = from_cloud_event(&codex_event("message.assistant.tool_use", tool_use));
    assert_eq!(records.len(), 1);
    match &records[0].body {
        RecordBody::ToolCall(call) => {
            assert_eq!(call.call_id, "call_1");
            assert_eq!(call.name, "exec_command");
            assert_eq!(call.input, json!({"cmd": "pwd"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }

    let mut result = CodexPayload::new();
    result.item_type = Some("function_call_output".to_string());
    result.call_id = Some("call_1".to_string());
    result.output = Some("Output:\n/Users/maxglassie/projects/codex\n".to_string());

    let records = from_cloud_event(&codex_event("message.user.tool_result", result));
    assert_eq!(records.len(), 1);
    match &records[0].body {
        RecordBody::ToolResult(result) => {
            assert_eq!(result.call_id, "call_1");
            assert_eq!(
                result.output.as_deref(),
                Some("Output:\n/Users/maxglassie/projects/codex\n")
            );
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn codex_messages_use_typed_payload_text() {
    let mut assistant = CodexPayload::new();
    assistant.item_type = Some("message".to_string());
    assistant.text = Some("Back in `/Users/maxglassie/projects/codex`.".to_string());
    assistant.model = Some("gpt-5.5".to_string());

    let records = from_cloud_event(&codex_event("message.assistant.text", assistant));
    assert_eq!(records.len(), 1);
    match &records[0].body {
        RecordBody::AssistantMessage(message) => {
            assert_eq!(message.model, "gpt-5.5");
            assert_eq!(message.content.len(), 1);
            match &message.content[0] {
                ContentBlock::Text { text } => {
                    assert_eq!(text, "Back in `/Users/maxglassie/projects/codex`.");
                }
                other => panic!("expected text content block, got {other:?}"),
            }
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }

    let mut user = CodexPayload::new();
    user.item_type = Some("user_message".to_string());
    user.text = Some("Can you find the session data?".to_string());

    let records = from_cloud_event(&codex_event("message.user.prompt", user));
    assert_eq!(records.len(), 1);
    match &records[0].body {
        RecordBody::UserMessage(message) => match &message.content {
            MessageContent::Text(text) => {
                assert_eq!(text, "Can you find the session data?");
            }
            other => panic!("expected text message content, got {other:?}"),
        },
        other => panic!("expected UserMessage, got {other:?}"),
    }
}

#[test]
fn codex_reasoning_uses_typed_payload_text() {
    let mut reasoning = CodexPayload::new();
    reasoning.item_type = Some("reasoning".to_string());
    reasoning.text = Some("I need to inspect the local session files.".to_string());

    let records = from_cloud_event(&codex_event("message.assistant.thinking", reasoning));
    assert_eq!(records.len(), 1);
    match &records[0].body {
        RecordBody::Reasoning(reasoning) => {
            assert_eq!(
                reasoning.content.as_deref(),
                Some("I need to inspect the local session files.")
            );
            assert!(!reasoning.encrypted);
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}
