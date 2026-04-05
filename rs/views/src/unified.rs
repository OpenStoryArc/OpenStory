// Unified record types adapted from unified-transcript-schema reference.
// These are the typed records that ViewRecord wraps.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tool_input::{self, ToolInput};
use open_story_core::event_data::ToolOutcome;

// ---------------------------------------------------------------------------
// RecordBody — discriminated union of all record types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record_type", content = "payload", rename_all = "snake_case")]
pub enum RecordBody {
    SessionMeta(SessionMeta),
    TurnStart(TurnStart),
    TurnEnd(TurnEnd),
    UserMessage(UserMessage),
    AssistantMessage(Box<AssistantMessage>),
    Reasoning(Reasoning),
    ToolCall(Box<ToolCall>),
    ToolResult(ToolResult),
    TokenUsage(TokenUsage),
    ContextCompaction(ContextCompaction),
    FileSnapshot(FileSnapshot),
    SystemEvent(SystemEvent),
    Error(ErrorRecord),
}

// ---------------------------------------------------------------------------
// Session metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

// ---------------------------------------------------------------------------
// Turn lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnEnd {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub content: MessageContent,
    #[serde(default)]
    pub images: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub model: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_turn: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    CodeBlock { text: String, language: Option<String> },
    Image { source: Value },
}

// ---------------------------------------------------------------------------
// Reasoning / thinking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reasoning {
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub encrypted: bool,
}

// ---------------------------------------------------------------------------
// Tool call / result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    /// Raw tool input as JSON value.
    pub input: Value,
    /// Preserved original input for display/debugging.
    pub raw_input: Value,
    /// Typed tool input, populated by `resolve_typed_input()`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed_input: Option<ToolInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl ToolCall {
    /// Parse raw_input into a typed ToolInput variant.
    pub fn resolve_typed_input(&mut self) {
        self.typed_input = Some(tool_input::parse_tool_input(&self.name, self.raw_input.clone()));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    /// Domain event: what this tool call changed in the world.
    /// Derived from the tool_call + result pair in the translate layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcome: Option<ToolOutcome>,
}

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    pub scope: TokenScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenScope {
    Turn,
    SessionTotal,
}

// ---------------------------------------------------------------------------
// Context compaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// File/git snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_files: Option<Value>,
}

// ---------------------------------------------------------------------------
// System / error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::unified::*;

    // describe("RecordBody deserialization")
    mod record_body {
        use super::*;

        #[test]
        fn it_should_deserialize_user_message() {
            let json = json!({
                "record_type": "user_message",
                "payload": {
                    "content": "Hello, world!"
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::UserMessage(u) => {
                    match u.content {
                        MessageContent::Text(t) => assert_eq!(t, "Hello, world!"),
                        other => panic!("expected Text, got {:?}", other),
                    }
                }
                other => panic!("expected UserMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_assistant_message() {
            let json = json!({
                "record_type": "assistant_message",
                "payload": {
                    "model": "claude-sonnet-4-20250514",
                    "content": [{"type": "text", "text": "Here is my response."}],
                    "stop_reason": "end_turn"
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::AssistantMessage(a) => {
                    assert_eq!(a.model, "claude-sonnet-4-20250514");
                    assert_eq!(a.stop_reason, Some("end_turn".into()));
                }
                other => panic!("expected AssistantMessage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_tool_call_with_typed_input() {
            let json = json!({
                "record_type": "tool_call",
                "payload": {
                    "call_id": "toolu_123",
                    "name": "Bash",
                    "input": {"command": "cargo test"},
                    "raw_input": {"command": "cargo test"}
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::ToolCall(tc) => {
                    assert_eq!(tc.call_id, "toolu_123");
                    assert_eq!(tc.name, "Bash");
                    // typed_input populated by post-processing, not serde
                    assert!(tc.typed_input.is_none());
                    assert_eq!(tc.raw_input["command"], "cargo test");
                }
                other => panic!("expected ToolCall, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_tool_result() {
            let json = json!({
                "record_type": "tool_result",
                "payload": {
                    "call_id": "toolu_123",
                    "output": "test result: ok. 5 passed",
                    "is_error": false
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::ToolResult(tr) => {
                    assert_eq!(tr.call_id, "toolu_123");
                    assert_eq!(tr.output, Some("test result: ok. 5 passed".into()));
                    assert!(!tr.is_error);
                }
                other => panic!("expected ToolResult, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_reasoning() {
            let json = json!({
                "record_type": "reasoning",
                "payload": {
                    "summary": ["Thinking about the problem"],
                    "content": "Let me analyze this...",
                    "encrypted": false
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::Reasoning(r) => {
                    assert_eq!(r.summary, vec!["Thinking about the problem"]);
                    assert_eq!(r.content, Some("Let me analyze this...".into()));
                }
                other => panic!("expected Reasoning, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_turn_end_with_duration() {
            let json = json!({
                "record_type": "turn_end",
                "payload": {
                    "duration_ms": 4500,
                    "reason": "end_turn"
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::TurnEnd(t) => {
                    assert_eq!(t.duration_ms, Some(4500));
                    assert_eq!(t.reason, Some("end_turn".into()));
                }
                other => panic!("expected TurnEnd, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_token_usage() {
            let json = json!({
                "record_type": "token_usage",
                "payload": {
                    "input_tokens": 1000,
                    "output_tokens": 500,
                    "scope": "turn"
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::TokenUsage(t) => {
                    assert_eq!(t.input_tokens, Some(1000));
                    assert_eq!(t.output_tokens, Some(500));
                    assert_eq!(t.scope, TokenScope::Turn);
                }
                other => panic!("expected TokenUsage, got {:?}", other),
            }
        }

        #[test]
        fn it_should_deserialize_error_record() {
            let json = json!({
                "record_type": "error",
                "payload": {
                    "code": "rate_limit",
                    "message": "Too many requests"
                }
            });
            let body: RecordBody = serde_json::from_value(json).unwrap();
            match body {
                RecordBody::Error(e) => {
                    assert_eq!(e.code, "rate_limit");
                    assert_eq!(e.message, "Too many requests");
                }
                other => panic!("expected Error, got {:?}", other),
            }
        }
    }

    // describe("ToolCall.with_typed_input()")
    mod tool_call_typing {
        use super::*;
        use crate::tool_input::ToolInput;

        #[test]
        fn it_should_populate_typed_input_from_raw() {
            let mut tc = ToolCall {
                call_id: "toolu_1".into(),
                name: "Bash".into(),
                input: json!({"command": "ls"}),
                raw_input: json!({"command": "ls"}),
                typed_input: None,
                status: None,
            };
            tc.resolve_typed_input();
            match tc.typed_input.as_ref().unwrap() {
                ToolInput::Bash(b) => assert_eq!(b.command, "ls"),
                other => panic!("expected Bash, got {:?}", other),
            }
        }

        #[test]
        fn it_should_produce_unknown_for_mcp_tool() {
            let mut tc = ToolCall {
                call_id: "toolu_2".into(),
                name: "mcp__slack__post".into(),
                input: json!({"channel": "#dev"}),
                raw_input: json!({"channel": "#dev"}),
                typed_input: None,
                status: None,
            };
            tc.resolve_typed_input();
            match tc.typed_input.as_ref().unwrap() {
                ToolInput::Unknown { name, .. } => assert_eq!(name, "mcp__slack__post"),
                other => panic!("expected Unknown, got {:?}", other),
            }
        }

        #[test]
        fn it_should_keep_raw_input_alongside_typed() {
            let mut tc = ToolCall {
                call_id: "toolu_3".into(),
                name: "Edit".into(),
                input: json!({"file_path": "/f.rs", "old_string": "a", "new_string": "b"}),
                raw_input: json!({"file_path": "/f.rs", "old_string": "a", "new_string": "b"}),
                typed_input: None,
                status: None,
            };
            tc.resolve_typed_input();
            assert!(tc.typed_input.is_some());
            assert_eq!(tc.raw_input["file_path"], "/f.rs");
        }
    }
}
