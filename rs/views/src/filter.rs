// Typed filter functions over ViewRecord collections.

use crate::tool_input::ToolInput;
use crate::unified::RecordBody;
use crate::view_record::ViewRecord;

/// Filter records to just file-modifying tool calls (Edit, Write).
pub fn file_edits(records: &[ViewRecord]) -> Vec<&ViewRecord> {
    records
        .iter()
        .filter(|r| {
            if let RecordBody::ToolCall(tc) = &r.body {
                matches!(
                    tc.typed_input.as_ref(),
                    Some(ToolInput::Edit(_)) | Some(ToolInput::Write(_))
                )
            } else {
                false
            }
        })
        .collect()
}

/// Filter to bash commands that are git operations.
pub fn git_commands(records: &[ViewRecord]) -> Vec<&ViewRecord> {
    records
        .iter()
        .filter(|r| {
            if let RecordBody::ToolCall(tc) = &r.body {
                if let Some(ToolInput::Bash(b)) = tc.typed_input.as_ref() {
                    return is_git_command(&b.command);
                }
            }
            false
        })
        .collect()
}

/// Extract all unique file paths touched by file operations (Read, Edit, Write).
pub fn files_touched(records: &[ViewRecord]) -> Vec<String> {
    let mut paths = Vec::new();
    for r in records {
        if let RecordBody::ToolCall(tc) = &r.body {
            let path = match tc.typed_input.as_ref() {
                Some(ToolInput::Read(r)) => Some(r.file_path.clone()),
                Some(ToolInput::Edit(e)) => Some(e.file_path.clone()),
                Some(ToolInput::Write(w)) => Some(w.file_path.clone()),
                _ => None,
            };
            if let Some(p) = path {
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }
    }
    paths
}

fn is_git_command(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed.starts_with("git ")
        || trimmed.starts_with("git\t")
        || trimmed == "git"
        || trimmed.contains("&& git ")
        || trimmed.contains("; git ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use crate::filter::*;
    use crate::view_record::ViewRecord;
    use crate::unified::*;
    use crate::tool_input::{self, ToolInput};

    fn make_tool_call_record(seq: u64, name: &str, input: serde_json::Value) -> ViewRecord {
        let typed = tool_input::parse_tool_input(name, input.clone());
        ViewRecord {
            id: format!("evt-{}", seq),
            seq,
            session_id: "sess-1".into(),
            timestamp: "2025-01-09T10:00:00Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolCall(Box::new(ToolCall {
                call_id: format!("call_{}", seq),
                name: name.into(),
                input: input.clone(),
                raw_input: input,
                typed_input: Some(typed),
                status: None,
            })),
        }
    }

    fn make_user_record(seq: u64) -> ViewRecord {
        ViewRecord {
            id: format!("evt-{}", seq),
            seq,
            session_id: "sess-1".into(),
            timestamp: "2025-01-09T10:00:00Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text("hello".into()),
                images: vec![],
            }),
        }
    }

    // describe("file_edits filter")
    mod file_edits_filter {
        use super::*;

        #[test]
        fn it_should_return_only_edit_and_write_tool_calls() {
            let records = vec![
                make_tool_call_record(1, "Edit", json!({"file_path": "/a.rs", "old_string": "x", "new_string": "y"})),
                make_tool_call_record(2, "Write", json!({"file_path": "/b.rs", "content": "new file"})),
                make_tool_call_record(3, "Read", json!({"file_path": "/c.rs"})),
                make_tool_call_record(4, "Bash", json!({"command": "ls"})),
                make_user_record(5),
            ];
            let edits = file_edits(&records);
            assert_eq!(edits.len(), 2);
            assert_eq!(edits[0].seq, 1);
            assert_eq!(edits[1].seq, 2);
        }

        #[test]
        fn it_should_allow_access_to_edit_input_fields_without_casting() {
            let records = vec![
                make_tool_call_record(1, "Edit", json!({
                    "file_path": "/src/main.rs",
                    "old_string": "println!(\"old\")",
                    "new_string": "println!(\"new\")"
                })),
            ];
            let edits = file_edits(&records);
            let tc = match &edits[0].body {
                RecordBody::ToolCall(tc) => tc,
                _ => panic!("expected ToolCall"),
            };
            match tc.typed_input.as_ref().unwrap() {
                ToolInput::Edit(e) => {
                    assert_eq!(e.file_path, "/src/main.rs");
                    assert_eq!(e.old_string, "println!(\"old\")");
                    assert_eq!(e.new_string, "println!(\"new\")");
                }
                other => panic!("expected Edit, got {:?}", other),
            }
        }
    }

    // describe("git_commands filter")
    mod git_commands_filter {
        use super::*;

        #[test]
        fn it_should_return_only_bash_calls_containing_git_commands() {
            let records = vec![
                make_tool_call_record(1, "Bash", json!({"command": "git status"})),
                make_tool_call_record(2, "Bash", json!({"command": "cargo test"})),
                make_tool_call_record(3, "Bash", json!({"command": "git diff HEAD"})),
                make_tool_call_record(4, "Read", json!({"file_path": "/f.rs"})),
            ];
            let gits = git_commands(&records);
            assert_eq!(gits.len(), 2);
            assert_eq!(gits[0].seq, 1);
            assert_eq!(gits[1].seq, 3);
        }

        #[test]
        fn it_should_allow_access_to_bash_command_without_casting() {
            let records = vec![
                make_tool_call_record(1, "Bash", json!({"command": "git push origin main"})),
            ];
            let gits = git_commands(&records);
            let tc = match &gits[0].body {
                RecordBody::ToolCall(tc) => tc,
                _ => panic!("expected ToolCall"),
            };
            match tc.typed_input.as_ref().unwrap() {
                ToolInput::Bash(b) => assert_eq!(b.command, "git push origin main"),
                other => panic!("expected Bash, got {:?}", other),
            }
        }
    }

    // describe("files_touched filter")
    mod files_touched_filter {
        use super::*;

        #[test]
        fn it_should_extract_unique_file_paths_from_file_operations() {
            let records = vec![
                make_tool_call_record(1, "Read", json!({"file_path": "/src/main.rs"})),
                make_tool_call_record(2, "Edit", json!({"file_path": "/src/lib.rs", "old_string": "a", "new_string": "b"})),
                make_tool_call_record(3, "Write", json!({"file_path": "/src/new.rs", "content": "// new"})),
                make_tool_call_record(4, "Read", json!({"file_path": "/src/main.rs"})), // duplicate
                make_tool_call_record(5, "Bash", json!({"command": "ls"})),
            ];
            let files = files_touched(&records);
            assert_eq!(files.len(), 3);
            assert!(files.contains(&"/src/main.rs".to_string()));
            assert!(files.contains(&"/src/lib.rs".to_string()));
            assert!(files.contains(&"/src/new.rs".to_string()));
        }
    }
}
