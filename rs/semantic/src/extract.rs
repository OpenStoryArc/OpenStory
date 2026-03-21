//! Text extraction from ViewRecords — pure functions for the embedding pipeline.
//!
//! `extract_text` pulls embeddable text from a ViewRecord.
//! `extract_metadata` pulls structured metadata for filtering/display.

use open_story_views::unified::{ContentBlock, MessageContent, RecordBody};
use open_story_views::view_record::ViewRecord;

use crate::ChunkMetadata;

/// Maximum characters to extract from tool results (prevents embedding huge outputs).
const MAX_TOOL_RESULT_CHARS: usize = 2000;

/// Extract embeddable text from a ViewRecord. Returns None for non-embeddable types.
pub fn extract_text(vr: &ViewRecord) -> Option<String> {
    match &vr.body {
        RecordBody::UserMessage(msg) => {
            let text = match &msg.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        ContentBlock::CodeBlock { text, .. } => Some(text.as_str()),
                        ContentBlock::Image { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if text.is_empty() { None } else { Some(text) }
        }

        RecordBody::AssistantMessage(msg) => {
            let text: String = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::CodeBlock { text, .. } => Some(text.as_str()),
                    ContentBlock::Image { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() { None } else { Some(text) }
        }

        RecordBody::ToolCall(tc) => {
            let input_str = if tc.input.is_string() {
                tc.input.as_str().unwrap_or("").to_string()
            } else {
                serde_json::to_string(&tc.input).unwrap_or_default()
            };
            Some(format!("Tool: {} \u{2014} {}", tc.name, input_str))
        }

        RecordBody::ToolResult(tr) => {
            let output = tr.output.as_deref().unwrap_or("");
            if output.is_empty() {
                return None;
            }
            let truncated = if output.len() > MAX_TOOL_RESULT_CHARS {
                // Find a valid char boundary at or before MAX_TOOL_RESULT_CHARS
                let mut end = MAX_TOOL_RESULT_CHARS;
                while end > 0 && !output.is_char_boundary(end) {
                    end -= 1;
                }
                &output[..end]
            } else {
                output
            };
            if tr.is_error {
                Some(format!("[ERROR] {}", truncated))
            } else {
                Some(truncated.to_string())
            }
        }

        RecordBody::Reasoning(r) => {
            // Prefer content, fall back to summary
            if let Some(content) = &r.content {
                if !content.is_empty() {
                    return Some(content.clone());
                }
            }
            let summary = r.summary.join(" ");
            if summary.is_empty() { None } else { Some(summary) }
        }

        RecordBody::Error(e) => Some(format!("Error: {}", e.message)),

        RecordBody::SystemEvent(se) => {
            se.message.as_ref().map(|m| format!("System: {}", m))
        }

        // Non-embeddable types
        RecordBody::SessionMeta(_)
        | RecordBody::TurnStart(_)
        | RecordBody::TurnEnd(_)
        | RecordBody::TokenUsage(_)
        | RecordBody::ContextCompaction(_)
        | RecordBody::FileSnapshot(_) => None,
    }
}

/// Extract metadata from a ViewRecord for filtering and display.
pub fn extract_metadata(vr: &ViewRecord) -> ChunkMetadata {
    let record_type = match &vr.body {
        RecordBody::UserMessage(_) => "user_message",
        RecordBody::AssistantMessage(_) => "assistant_message",
        RecordBody::ToolCall(_) => "tool_call",
        RecordBody::ToolResult(_) => "tool_result",
        RecordBody::Reasoning(_) => "reasoning",
        RecordBody::Error(_) => "error",
        RecordBody::SystemEvent(_) => "system_event",
        RecordBody::SessionMeta(_) => "session_meta",
        RecordBody::TurnStart(_) => "turn_start",
        RecordBody::TurnEnd(_) => "turn_end",
        RecordBody::TokenUsage(_) => "token_usage",
        RecordBody::ContextCompaction(_) => "context_compaction",
        RecordBody::FileSnapshot(_) => "file_snapshot",
    };

    let tool_name = match &vr.body {
        RecordBody::ToolCall(tc) => Some(tc.name.clone()),
        _ => None,
    };

    ChunkMetadata {
        record_type: record_type.to_string(),
        timestamp: vr.timestamp.clone(),
        tool_name,
        session_label: None, // Populated by the caller if available
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_views::unified::*;
    use serde_json::json;

    fn make_vr(body: RecordBody) -> ViewRecord {
        ViewRecord {
            id: "evt-1".into(),
            seq: 1,
            session_id: "sess-1".into(),
            timestamp: "2025-01-17T00:00:00Z".into(),
            agent_id: None,
            is_sidechain: false,
            body,
        }
    }

    // describe("extract_text") — boundary table
    //
    // | RecordBody variant        | Expected output              | Embeddable? |
    // |---------------------------|------------------------------|-------------|
    // | UserMessage("fix auth")   | "fix auth"                   | yes         |
    // | UserMessage("")           | None                         | no          |
    // | AssistantMessage(text)    | "response text"              | yes         |
    // | ToolCall(Bash, "cargo t") | "Tool: Bash — ..."           | yes         |
    // | ToolCall(Read, "/foo.rs") | "Tool: Read — ..."           | yes         |
    // | ToolResult(output, ok)    | "output" (truncated 2000ch)  | yes         |
    // | ToolResult(output, error) | "[ERROR] output"             | yes         |
    // | Reasoning(text)           | "thinking text"              | yes         |
    // | Error(msg)                | "Error: msg"                 | yes         |
    // | TurnEnd / TokenUsage      | None                         | no          |
    // | SessionMeta / FileSnapshot| None                         | no          |

    mod extract_text_boundary_table {
        use super::*;

        #[test]
        fn user_message_with_text() {
            let vr = make_vr(RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text("fix auth".into()),
                images: vec![],
            }));
            assert_eq!(extract_text(&vr), Some("fix auth".into()));
        }

        #[test]
        fn user_message_empty() {
            let vr = make_vr(RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text(String::new()),
                images: vec![],
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn user_message_with_blocks() {
            let vr = make_vr(RecordBody::UserMessage(UserMessage {
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text { text: "hello".into() },
                    ContentBlock::Text { text: "world".into() },
                ]),
                images: vec![],
            }));
            assert_eq!(extract_text(&vr), Some("hello\nworld".into()));
        }

        #[test]
        fn assistant_message_with_text() {
            let vr = make_vr(RecordBody::AssistantMessage(Box::new(AssistantMessage {
                model: "claude-4".into(),
                content: vec![ContentBlock::Text {
                    text: "response text".into(),
                }],
                stop_reason: None,
                end_turn: None,
                phase: None,
            })));
            assert_eq!(extract_text(&vr), Some("response text".into()));
        }

        #[test]
        fn assistant_message_empty_content() {
            let vr = make_vr(RecordBody::AssistantMessage(Box::new(AssistantMessage {
                model: "claude-4".into(),
                content: vec![],
                stop_reason: None,
                end_turn: None,
                phase: None,
            })));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn tool_call_bash() {
            let vr = make_vr(RecordBody::ToolCall(Box::new(ToolCall {
                call_id: "toolu_1".into(),
                name: "Bash".into(),
                input: json!({"command": "cargo test"}),
                raw_input: json!({"command": "cargo test"}),
                typed_input: None,
                status: None,
            })));
            let text = extract_text(&vr).unwrap();
            assert!(text.starts_with("Tool: Bash"));
            assert!(text.contains("cargo test"));
        }

        #[test]
        fn tool_call_read() {
            let vr = make_vr(RecordBody::ToolCall(Box::new(ToolCall {
                call_id: "toolu_2".into(),
                name: "Read".into(),
                input: json!({"file_path": "/foo.rs"}),
                raw_input: json!({"file_path": "/foo.rs"}),
                typed_input: None,
                status: None,
            })));
            let text = extract_text(&vr).unwrap();
            assert!(text.starts_with("Tool: Read"));
            assert!(text.contains("/foo.rs"));
        }

        #[test]
        fn tool_result_ok() {
            let vr = make_vr(RecordBody::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                output: Some("test result: ok. 5 passed".into()),
                is_error: false,
            }));
            assert_eq!(
                extract_text(&vr),
                Some("test result: ok. 5 passed".into())
            );
        }

        #[test]
        fn tool_result_error() {
            let vr = make_vr(RecordBody::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                output: Some("compilation failed".into()),
                is_error: true,
            }));
            assert_eq!(
                extract_text(&vr),
                Some("[ERROR] compilation failed".into())
            );
        }

        #[test]
        fn tool_result_truncated_at_2000_chars() {
            let long_output = "x".repeat(3000);
            let vr = make_vr(RecordBody::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                output: Some(long_output),
                is_error: false,
            }));
            let text = extract_text(&vr).unwrap();
            assert_eq!(text.len(), 2000);
        }

        #[test]
        fn tool_result_empty_output() {
            let vr = make_vr(RecordBody::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                output: Some(String::new()),
                is_error: false,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn tool_result_none_output() {
            let vr = make_vr(RecordBody::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                output: None,
                is_error: false,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn reasoning_with_content() {
            let vr = make_vr(RecordBody::Reasoning(Reasoning {
                summary: vec!["summary line".into()],
                content: Some("thinking text".into()),
                encrypted: false,
            }));
            assert_eq!(extract_text(&vr), Some("thinking text".into()));
        }

        #[test]
        fn reasoning_with_summary_only() {
            let vr = make_vr(RecordBody::Reasoning(Reasoning {
                summary: vec!["Thinking about the problem".into()],
                content: None,
                encrypted: false,
            }));
            assert_eq!(
                extract_text(&vr),
                Some("Thinking about the problem".into())
            );
        }

        #[test]
        fn reasoning_empty() {
            let vr = make_vr(RecordBody::Reasoning(Reasoning {
                summary: vec![],
                content: None,
                encrypted: false,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn error_record() {
            let vr = make_vr(RecordBody::Error(ErrorRecord {
                code: "rate_limit".into(),
                message: "Too many requests".into(),
                details: None,
            }));
            assert_eq!(
                extract_text(&vr),
                Some("Error: Too many requests".into())
            );
        }

        #[test]
        fn system_event_with_message() {
            let vr = make_vr(RecordBody::SystemEvent(SystemEvent {
                subtype: "system.hook".into(),
                message: Some("hook fired".into()),
                duration_ms: None,
            }));
            assert_eq!(extract_text(&vr), Some("System: hook fired".into()));
        }

        #[test]
        fn system_event_without_message() {
            let vr = make_vr(RecordBody::SystemEvent(SystemEvent {
                subtype: "system.hook".into(),
                message: None,
                duration_ms: None,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        // Non-embeddable types
        #[test]
        fn turn_end_not_embeddable() {
            let vr = make_vr(RecordBody::TurnEnd(TurnEnd {
                turn_id: None,
                reason: Some("end_turn".into()),
                duration_ms: Some(3000),
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn turn_start_not_embeddable() {
            let vr = make_vr(RecordBody::TurnStart(TurnStart { turn_id: None }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn token_usage_not_embeddable() {
            let vr = make_vr(RecordBody::TokenUsage(TokenUsage {
                input_tokens: Some(1000),
                output_tokens: Some(500),
                total_tokens: None,
                scope: TokenScope::Turn,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn session_meta_not_embeddable() {
            let vr = make_vr(RecordBody::SessionMeta(SessionMeta {
                cwd: "/home/user".into(),
                model: "claude-4".into(),
                version: "1.0".into(),
                git: None,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn file_snapshot_not_embeddable() {
            let vr = make_vr(RecordBody::FileSnapshot(FileSnapshot {
                git_commit: Some("abc123".into()),
                git_message: Some("fix bug".into()),
                tracked_files: None,
            }));
            assert_eq!(extract_text(&vr), None);
        }

        #[test]
        fn context_compaction_not_embeddable() {
            let vr = make_vr(RecordBody::ContextCompaction(ContextCompaction {
                reason: Some("context too long".into()),
                message: Some("compacted".into()),
            }));
            assert_eq!(extract_text(&vr), None);
        }
    }

    // describe("extract_metadata")
    mod extract_metadata_tests {
        use super::*;

        #[test]
        fn user_message_metadata() {
            let vr = make_vr(RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text("hello".into()),
                images: vec![],
            }));
            let meta = extract_metadata(&vr);
            assert_eq!(meta.record_type, "user_message");
            assert_eq!(meta.timestamp, "2025-01-17T00:00:00Z");
            assert_eq!(meta.tool_name, None);
        }

        #[test]
        fn tool_call_metadata_includes_tool_name() {
            let vr = make_vr(RecordBody::ToolCall(Box::new(ToolCall {
                call_id: "toolu_1".into(),
                name: "Bash".into(),
                input: json!({}),
                raw_input: json!({}),
                typed_input: None,
                status: None,
            })));
            let meta = extract_metadata(&vr);
            assert_eq!(meta.record_type, "tool_call");
            assert_eq!(meta.tool_name, Some("Bash".into()));
        }

        #[test]
        fn error_metadata() {
            let vr = make_vr(RecordBody::Error(ErrorRecord {
                code: "err".into(),
                message: "oops".into(),
                details: None,
            }));
            let meta = extract_metadata(&vr);
            assert_eq!(meta.record_type, "error");
            assert_eq!(meta.tool_name, None);
        }
    }
}
