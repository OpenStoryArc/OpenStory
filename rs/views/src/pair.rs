// Tool pairing: match ToolCall + ToolResult by call_id into ToolRoundtrip.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::unified::RecordBody;
use crate::view_record::ViewRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedConversation {
    pub entries: Vec<ConversationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "entry_type", rename_all = "snake_case")]
pub enum ConversationEntry {
    UserMessage(ViewRecord),
    AssistantMessage(ViewRecord),
    Reasoning(ViewRecord),
    ToolRoundtrip {
        call: Box<ViewRecord>,
        result: Option<Box<ViewRecord>>,
    },
    TokenUsage(ViewRecord),
    System(ViewRecord),
}

/// Pair ToolCall and ToolResult records by call_id.
/// Non-tool records pass through as their own entry type.
pub fn pair_records(records: &[ViewRecord]) -> PairedConversation {
    // Index: call_id → ToolResult ViewRecord
    let mut results_by_id: HashMap<String, ViewRecord> = HashMap::new();
    for r in records {
        if let RecordBody::ToolResult(tr) = &r.body {
            results_by_id.insert(tr.call_id.clone(), r.clone());
        }
    }

    let mut entries = Vec::new();
    for r in records {
        match &r.body {
            RecordBody::ToolCall(tc) => {
                let result = results_by_id.remove(&tc.call_id);
                entries.push(ConversationEntry::ToolRoundtrip {
                    call: Box::new(r.clone()),
                    result: result.map(Box::new),
                });
            }
            RecordBody::ToolResult(_) => {
                // Consumed by pairing above; skip standalone results
            }
            RecordBody::UserMessage(_) => {
                entries.push(ConversationEntry::UserMessage(r.clone()));
            }
            RecordBody::AssistantMessage(_) => {
                entries.push(ConversationEntry::AssistantMessage(r.clone()));
            }
            RecordBody::Reasoning(_) => {
                entries.push(ConversationEntry::Reasoning(r.clone()));
            }
            RecordBody::TokenUsage(_) => {
                entries.push(ConversationEntry::TokenUsage(r.clone()));
            }
            _ => {
                entries.push(ConversationEntry::System(r.clone()));
            }
        }
    }

    PairedConversation { entries }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::pair::*;
    use crate::view_record::ViewRecord;
    use crate::unified::*;

    fn make_tool_call(id: &str, call_id: &str, seq: u64, name: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq,
            session_id: "sess-1".into(),
            timestamp: format!("2025-01-09T10:00:{:02}Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolCall(Box::new(ToolCall {
                call_id: call_id.into(),
                name: name.into(),
                input: json!({"command": "test"}),
                raw_input: json!({"command": "test"}),
                typed_input: None,
                status: None,
            })),
        }
    }

    fn make_tool_result(id: &str, call_id: &str, seq: u64, output: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq,
            session_id: "sess-1".into(),
            timestamp: format!("2025-01-09T10:00:{:02}Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolResult(ToolResult {
                call_id: call_id.into(),
                output: Some(output.into()),
                is_error: false,
            }),
        }
    }

    fn make_user_msg(id: &str, seq: u64, text: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq,
            session_id: "sess-1".into(),
            timestamp: format!("2025-01-09T10:00:{:02}Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text(text.into()),
                images: vec![],
            }),
        }
    }

    fn make_assistant_msg(id: &str, seq: u64, text: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq,
            session_id: "sess-1".into(),
            timestamp: format!("2025-01-09T10:00:{:02}Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::AssistantMessage(Box::new(AssistantMessage {
                model: "claude-sonnet-4-20250514".into(),
                content: vec![ContentBlock::Text { text: text.into() }],
                stop_reason: None,
                end_turn: None,
                phase: None,
            })),
        }
    }

    // describe("pair_tools")
    mod pair_tools {
        use super::*;

        #[test]
        fn it_should_pair_tool_call_and_result_by_call_id() {
            let records = vec![
                make_tool_call("e1", "call_1", 1, "Bash"),
                make_tool_result("e2", "call_1", 2, "ok"),
            ];
            let paired = pair_records(&records);
            assert_eq!(paired.entries.len(), 1);
            match &paired.entries[0] {
                ConversationEntry::ToolRoundtrip { call, result, .. } => {
                    assert_eq!(call.id, "e1");
                    assert!(result.is_some());
                    assert_eq!(result.as_ref().unwrap().id, "e2");
                }
                other => panic!("expected ToolRoundtrip, got {:?}", other),
            }
        }

        #[test]
        fn it_should_leave_unpaired_calls_with_result_none() {
            let records = vec![
                make_tool_call("e1", "call_1", 1, "Bash"),
            ];
            let paired = pair_records(&records);
            assert_eq!(paired.entries.len(), 1);
            match &paired.entries[0] {
                ConversationEntry::ToolRoundtrip { result, .. } => {
                    assert!(result.is_none());
                }
                other => panic!("expected ToolRoundtrip, got {:?}", other),
            }
        }

        #[test]
        fn it_should_handle_interleaved_calls_and_results() {
            let records = vec![
                make_tool_call("e1", "call_a", 1, "Read"),
                make_tool_call("e2", "call_b", 2, "Grep"),
                make_tool_result("e3", "call_b", 3, "found it"),
                make_tool_result("e4", "call_a", 4, "file contents"),
            ];
            let paired = pair_records(&records);
            assert_eq!(paired.entries.len(), 2);
            // First entry is call_a (appeared first)
            match &paired.entries[0] {
                ConversationEntry::ToolRoundtrip { call, result, .. } => {
                    assert_eq!(call.id, "e1");
                    assert_eq!(result.as_ref().unwrap().id, "e4");
                }
                other => panic!("expected ToolRoundtrip, got {:?}", other),
            }
            // Second entry is call_b
            match &paired.entries[1] {
                ConversationEntry::ToolRoundtrip { call, result, .. } => {
                    assert_eq!(call.id, "e2");
                    assert_eq!(result.as_ref().unwrap().id, "e3");
                }
                other => panic!("expected ToolRoundtrip, got {:?}", other),
            }
        }

        #[test]
        fn it_should_pass_through_non_tool_records() {
            let records = vec![
                make_user_msg("e1", 1, "fix the bug"),
                make_assistant_msg("e2", 2, "I'll fix it"),
                make_tool_call("e3", "call_1", 3, "Edit"),
                make_tool_result("e4", "call_1", 4, "done"),
            ];
            let paired = pair_records(&records);
            assert_eq!(paired.entries.len(), 3);
            assert!(matches!(&paired.entries[0], ConversationEntry::UserMessage(_)));
            assert!(matches!(&paired.entries[1], ConversationEntry::AssistantMessage(_)));
            assert!(matches!(&paired.entries[2], ConversationEntry::ToolRoundtrip { .. }));
        }

        #[test]
        fn it_should_handle_empty_input() {
            let paired = pair_records(&[]);
            assert!(paired.entries.is_empty());
        }
    }
}
