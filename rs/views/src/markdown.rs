//! Markdown renderer for PairedConversation.
//!
//! Transforms a conversation into a readable markdown document.
//! Pure function: PairedConversation in, String out.

use crate::pair::{ConversationEntry, PairedConversation};
use crate::unified::{ContentBlock, MessageContent, RecordBody};

/// Render a PairedConversation as markdown.
///
/// Format:
/// - User messages as blockquotes
/// - Assistant messages as plain text
/// - Tool calls as code blocks with tool name header
/// - Tool results as collapsible details
/// - Reasoning as italic blockquotes
/// - Timestamps as subtle markers
pub fn conversation_to_markdown(conversation: &PairedConversation, session_id: &str) -> String {
    let mut out = String::new();

    out.push_str(&format!("# Session {}\n\n", &session_id[..12.min(session_id.len())]));

    for entry in &conversation.entries {
        match entry {
            ConversationEntry::UserMessage(r) => {
                out.push_str(&format!("---\n\n**User** _{}_\n\n", format_time(&r.timestamp)));
                if let RecordBody::UserMessage(msg) = &r.body {
                    let text = extract_message_content(&msg.content);
                    for line in text.lines() {
                        out.push_str(&format!("> {}\n", line));
                    }
                    out.push('\n');
                }
            }

            ConversationEntry::AssistantMessage(r) => {
                out.push_str(&format!("**Assistant** _{}_\n\n", format_time(&r.timestamp)));
                if let RecordBody::AssistantMessage(msg) = &r.body {
                    for block in &msg.content {
                        match block {
                            ContentBlock::Text { text } => {
                                out.push_str(text);
                                out.push_str("\n\n");
                            }
                            ContentBlock::CodeBlock { text, language } => {
                                let lang = language.as_deref().unwrap_or("");
                                out.push_str(&format!("```{}\n{}\n```\n\n", lang, text));
                            }
                            ContentBlock::Image { .. } => {
                                out.push_str("_(image)_\n\n");
                            }
                        }
                    }
                }
            }

            ConversationEntry::ToolRoundtrip { call, result } => {
                if let RecordBody::ToolCall(tc) = &call.body {
                    out.push_str(&format!("**{}** _{}_\n\n", tc.name, format_time(&call.timestamp)));

                    // Show relevant input based on tool type
                    let input_summary = summarize_tool_input(&tc.name, &tc.input);
                    if !input_summary.is_empty() {
                        out.push_str(&format!("```\n{}\n```\n\n", input_summary));
                    }

                    // Show result if present
                    if let Some(res) = result {
                        if let RecordBody::ToolResult(tr) = &res.body {
                            if let Some(output) = &tr.output {
                                let truncated = truncate(output, 500);
                                if tr.is_error {
                                    out.push_str(&format!("**Error:**\n```\n{}\n```\n\n", truncated));
                                } else {
                                    out.push_str("<details><summary>Output</summary>\n\n");
                                    out.push_str(&format!("```\n{}\n```\n\n", truncated));
                                    out.push_str("</details>\n\n");
                                }
                            }
                        }
                    }
                }
            }

            ConversationEntry::Reasoning(r) => {
                if let RecordBody::Reasoning(reasoning) = &r.body {
                    if !reasoning.summary.is_empty() {
                        out.push_str("_Thinking:_\n\n");
                        for line in &reasoning.summary {
                            out.push_str(&format!("> _{}_\n", line));
                        }
                        out.push('\n');
                    }
                }
            }

            ConversationEntry::TokenUsage(_) => {
                // Skip token usage in markdown view — noise
            }

            ConversationEntry::System(r) => {
                if let RecordBody::SystemEvent(evt) = &r.body {
                    let msg = evt.message.as_deref().unwrap_or(&evt.subtype);
                    out.push_str(&format!("_System: {}_\n\n", msg));
                }
            }
        }
    }

    out
}

/// Extract text content from a MessageContent enum.
fn extract_message_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => {
            blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Summarize tool input for display (not the full JSON blob).
fn summarize_tool_input(tool: &str, input: &serde_json::Value) -> String {
    match tool {
        "Read" => input.get("file_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "Edit" => {
            let file = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
            let old = input.get("old_string").and_then(|v| v.as_str()).map(|s| truncate(s, 80));
            let new = input.get("new_string").and_then(|v| v.as_str()).map(|s| truncate(s, 80));
            match (old, new) {
                (Some(o), Some(n)) => format!("{file}\n- {o}\n+ {n}"),
                _ => file.to_string(),
            }
        }
        "Write" => input.get("file_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "Bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "Glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() { pattern.to_string() } else { format!("{pattern} in {path}") }
        }
        "Agent" => input.get("prompt").and_then(|v| v.as_str()).map(|s| truncate(s, 120)).unwrap_or_default(),
        _ => {
            let s = serde_json::to_string_pretty(input).unwrap_or_default();
            truncate(&s, 200)
        }
    }
}

/// Truncate a string to max_len chars, adding "..." if truncated.
/// Uses char boundary to avoid panicking on multi-byte UTF-8.
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let end = s.char_indices()
            .nth(max_len)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
    }
}

/// Format an ISO timestamp to a short time string.
fn format_time(ts: &str) -> String {
    // "2025-01-19T23:15:58.293Z" → "23:15"
    if ts.len() >= 16 {
        ts[11..16].to_string()
    } else {
        ts.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pair::*;
    use crate::unified::*;
    use crate::view_record::ViewRecord;
    use serde_json::json;

    fn user_record(seq: u64, text: &str) -> ViewRecord {
        ViewRecord {
            id: format!("u-{seq}"),
            seq,
            session_id: "test-session-123".into(),
            timestamp: format!("2025-01-19T10:{:02}:00Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text(text.into()),
                images: vec![],
            }),
        }
    }

    fn assistant_record(seq: u64, text: &str) -> ViewRecord {
        ViewRecord {
            id: format!("a-{seq}"),
            seq,
            session_id: "test-session-123".into(),
            timestamp: format!("2025-01-19T10:{:02}:00Z", seq),
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

    fn tool_call_record(seq: u64, name: &str, input: serde_json::Value) -> ViewRecord {
        ViewRecord {
            id: format!("tc-{seq}"),
            seq,
            session_id: "test-session-123".into(),
            timestamp: format!("2025-01-19T10:{:02}:00Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolCall(Box::new(ToolCall {
                call_id: format!("call_{seq}"),
                name: name.into(),
                input: input.clone(),
                raw_input: input,
                typed_input: None,
                status: None,
            })),
        }
    }

    fn tool_result_record(seq: u64, call_id: &str, output: &str) -> ViewRecord {
        ViewRecord {
            id: format!("tr-{seq}"),
            seq,
            session_id: "test-session-123".into(),
            timestamp: format!("2025-01-19T10:{:02}:00Z", seq),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolResult(ToolResult {
                call_id: call_id.into(),
                output: Some(output.into()),
                is_error: false,
                tool_outcome: None,
            }),
        }
    }

    #[test]
    fn empty_conversation_produces_header_only() {
        let conv = PairedConversation { entries: vec![] };
        let md = conversation_to_markdown(&conv, "abc-123-def");
        assert!(md.starts_with("# Session abc-123-def"));
        // No entries beyond header
        assert_eq!(md.lines().count(), 2); // header + blank line
    }

    #[test]
    fn user_message_rendered_as_blockquote() {
        let conv = PairedConversation {
            entries: vec![ConversationEntry::UserMessage(user_record(1, "Fix the bug"))],
        };
        let md = conversation_to_markdown(&conv, "test-session");
        assert!(md.contains("> Fix the bug"));
        assert!(md.contains("**User**"));
    }

    #[test]
    fn assistant_message_rendered_as_text() {
        let conv = PairedConversation {
            entries: vec![ConversationEntry::AssistantMessage(assistant_record(2, "I'll fix it now."))],
        };
        let md = conversation_to_markdown(&conv, "test-session");
        assert!(md.contains("I'll fix it now."));
        assert!(md.contains("**Assistant**"));
    }

    #[test]
    fn tool_roundtrip_shows_tool_name_and_input() {
        let call = tool_call_record(3, "Bash", json!({"command": "cargo test"}));
        let result = tool_result_record(4, "call_3", "test passed");

        let conv = PairedConversation {
            entries: vec![ConversationEntry::ToolRoundtrip {
                call: Box::new(call),
                result: Some(Box::new(result)),
            }],
        };
        let md = conversation_to_markdown(&conv, "test-session");
        assert!(md.contains("**Bash**"));
        assert!(md.contains("cargo test"));
        assert!(md.contains("test passed"));
    }

    #[test]
    fn read_tool_shows_file_path() {
        let call = tool_call_record(3, "Read", json!({"file_path": "/src/main.rs"}));
        let conv = PairedConversation {
            entries: vec![ConversationEntry::ToolRoundtrip {
                call: Box::new(call),
                result: None,
            }],
        };
        let md = conversation_to_markdown(&conv, "test-session");
        assert!(md.contains("/src/main.rs"));
    }

    #[test]
    fn timestamps_formatted_as_short_time() {
        assert_eq!(format_time("2025-01-19T23:15:58.293Z"), "23:15");
        assert_eq!(format_time("short"), "short");
    }

    #[test]
    fn truncate_long_strings() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_multibyte_utf8() {
        // ─ is 3 bytes in UTF-8 — must not panic
        let s = "// ── Token Usage ──────────────────────────────────────────────────────────────────────";
        let result = truncate(s, 20);
        assert!(result.ends_with("..."));
        assert!(result.len() < s.len());
    }

    #[test]
    fn full_conversation_renders_in_order() {
        let conv = PairedConversation {
            entries: vec![
                ConversationEntry::UserMessage(user_record(1, "Fix the bug")),
                ConversationEntry::AssistantMessage(assistant_record(2, "Looking at the code")),
                ConversationEntry::ToolRoundtrip {
                    call: Box::new(tool_call_record(3, "Read", json!({"file_path": "src/lib.rs"}))),
                    result: Some(Box::new(tool_result_record(4, "call_3", "fn main() {}"))),
                },
                ConversationEntry::AssistantMessage(assistant_record(5, "Found and fixed the issue.")),
            ],
        };
        let md = conversation_to_markdown(&conv, "test-session-123");

        // Verify order
        let user_pos = md.find("Fix the bug").unwrap();
        let looking_pos = md.find("Looking at the code").unwrap();
        let read_pos = md.find("**Read**").unwrap();
        let fixed_pos = md.find("Found and fixed").unwrap();
        assert!(user_pos < looking_pos);
        assert!(looking_pos < read_pos);
        assert!(read_pos < fixed_pos);
    }
}
