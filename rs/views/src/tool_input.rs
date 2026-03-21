// ToolInput: typed discriminated union for Claude Code tool inputs.
// Consumers can `match` on variant and access fields with compile-time guarantees.
// `Unknown` handles MCP tools, custom tools, and future additions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed tool input — exhaustive over known Claude Code tools.
/// `Unknown` handles MCP tools, custom tools, and future additions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolInput {
    // -- File operations --
    Read(ReadInput),
    Edit(EditInput),
    Write(WriteInput),
    Glob(GlobInput),
    Grep(GrepInput),
    NotebookEdit(NotebookEditInput),

    // -- Command execution --
    Bash(BashInput),

    // -- Search & fetch --
    WebFetch(WebFetchInput),
    WebSearch(WebSearchInput),

    // -- Agent / subagent --
    Agent(AgentInput),

    // -- Task management --
    TaskCreate(TaskCreateInput),
    TaskUpdate(TaskUpdateInput),
    TaskGet(TaskGetInput),
    TaskList,
    TaskOutput(TaskOutputInput),
    TaskStop(TaskStopInput),

    // -- Plan / context --
    EnterPlanMode,
    ExitPlanMode(ExitPlanModeInput),
    EnterWorktree(EnterWorktreeInput),
    Skill(SkillInput),
    AskUserQuestion(AskUserQuestionInput),
    Lsp(LspInput),
    ToolSearch(ToolSearchInput),
    CronCreate(CronCreateInput),
    CronDelete(CronDeleteInput),
    CronList,

    // -- Escape hatch --
    Unknown {
        name: String,
        raw: Value,
    },
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadInput {
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditInput {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replace_all: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteInput {
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobInput {
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepInput {
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_limit: Option<u64>,
    #[serde(rename = "-i", skip_serializing_if = "Option::is_none")]
    pub case_insensitive: Option<bool>,
    #[serde(rename = "-n", skip_serializing_if = "Option::is_none")]
    pub line_numbers: Option<bool>,
    #[serde(rename = "-A", skip_serializing_if = "Option::is_none")]
    pub after_context: Option<u64>,
    #[serde(rename = "-B", skip_serializing_if = "Option::is_none")]
    pub before_context: Option<u64>,
    #[serde(rename = "-C", skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookEditInput {
    pub notebook_path: String,
    pub new_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashInput {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_in_background: Option<bool>,
}

// ---------------------------------------------------------------------------
// Search & fetch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchInput {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchInput {
    pub query: String,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_in_background: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
}

// ---------------------------------------------------------------------------
// Task management
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreateInput {
    pub subject: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdateInput {
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetInput {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutputInput {
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStopInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Plan / context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitPlanModeInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterWorktreeInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInput {
    pub skill: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionInput {
    pub question: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspInput {
    #[serde(flatten)]
    pub fields: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSearchInput {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronCreateInput {
    pub schedule: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronDeleteInput {
    pub id: String,
}

// ---------------------------------------------------------------------------
// Parsing: (name, input: Value) → ToolInput
// ---------------------------------------------------------------------------

/// Parse a tool name and raw JSON input into a typed ToolInput.
/// Falls back to `Unknown` on unrecognized names or malformed input.
pub fn parse_tool_input(name: &str, input: Value) -> ToolInput {
    let try_parse = |input: Value, name: &str| -> ToolInput {
        macro_rules! try_tool {
            ($variant:ident, $ty:ty) => {
                match serde_json::from_value::<$ty>(input.clone()) {
                    Ok(v) => return ToolInput::$variant(v),
                    Err(_) => {
                        return ToolInput::Unknown {
                            name: name.to_string(),
                            raw: input,
                        }
                    }
                }
            };
        }

        macro_rules! unit_tool {
            ($variant:ident) => {
                return ToolInput::$variant
            };
        }

        match name {
            // File operations
            "Read" => try_tool!(Read, ReadInput),
            "Edit" => try_tool!(Edit, EditInput),
            "Write" => try_tool!(Write, WriteInput),
            "Glob" => try_tool!(Glob, GlobInput),
            "Grep" => try_tool!(Grep, GrepInput),
            "NotebookEdit" => try_tool!(NotebookEdit, NotebookEditInput),

            // Command execution
            "Bash" => try_tool!(Bash, BashInput),

            // Search & fetch
            "WebFetch" => try_tool!(WebFetch, WebFetchInput),
            "WebSearch" => try_tool!(WebSearch, WebSearchInput),

            // Agent
            "Agent" => try_tool!(Agent, AgentInput),

            // Task management
            "TaskCreate" => try_tool!(TaskCreate, TaskCreateInput),
            "TaskUpdate" => try_tool!(TaskUpdate, TaskUpdateInput),
            "TaskGet" => try_tool!(TaskGet, TaskGetInput),
            "TaskList" => unit_tool!(TaskList),
            "TaskOutput" => try_tool!(TaskOutput, TaskOutputInput),
            "TaskStop" => try_tool!(TaskStop, TaskStopInput),

            // Plan / context
            "EnterPlanMode" => unit_tool!(EnterPlanMode),
            "ExitPlanMode" => try_tool!(ExitPlanMode, ExitPlanModeInput),
            "EnterWorktree" => try_tool!(EnterWorktree, EnterWorktreeInput),
            "Skill" => try_tool!(Skill, SkillInput),
            "AskUserQuestion" => try_tool!(AskUserQuestion, AskUserQuestionInput),
            "LSP" => try_tool!(Lsp, LspInput),
            "ToolSearch" => try_tool!(ToolSearch, ToolSearchInput),
            "CronCreate" => try_tool!(CronCreate, CronCreateInput),
            "CronDelete" => try_tool!(CronDelete, CronDeleteInput),
            "CronList" => unit_tool!(CronList),

            // Unknown / MCP / future tools
            _ => ToolInput::Unknown {
                name: name.to_string(),
                raw: input,
            },
        }
    };

    try_parse(input, name)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    // -----------------------------------------------------------------------
    // describe("parse_tool_input")
    // -----------------------------------------------------------------------

    // describe("when tool is Read")
    mod read_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_file_path_as_required_field() {
            let input = json!({"file_path": "/src/main.rs"});
            let result = parse_tool_input("Read", input);
            match result {
                ToolInput::Read(r) => assert_eq!(r.file_path, "/src/main.rs"),
                other => panic!("expected Read, got {:?}", other),
            }
        }

        #[test]
        fn it_should_parse_optional_offset_and_limit() {
            let input = json!({"file_path": "/big.log", "offset": 100, "limit": 50});
            let result = parse_tool_input("Read", input);
            match result {
                ToolInput::Read(r) => {
                    assert_eq!(r.offset, Some(100));
                    assert_eq!(r.limit, Some(50));
                }
                other => panic!("expected Read, got {:?}", other),
            }
        }

        #[test]
        fn it_should_parse_optional_pages_for_pdfs() {
            let input = json!({"file_path": "/doc.pdf", "pages": "1-5"});
            let result = parse_tool_input("Read", input);
            match result {
                ToolInput::Read(r) => assert_eq!(r.pages, Some("1-5".into())),
                other => panic!("expected Read, got {:?}", other),
            }
        }

        #[test]
        fn it_should_fall_back_to_unknown_when_file_path_missing() {
            let input = json!({"offset": 10});
            let result = parse_tool_input("Read", input);
            match result {
                ToolInput::Unknown { name, .. } => assert_eq!(name, "Read"),
                other => panic!("expected Unknown, got {:?}", other),
            }
        }
    }

    // describe("when tool is Edit")
    mod edit_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_file_path_old_string_new_string() {
            let input = json!({
                "file_path": "/src/lib.rs",
                "old_string": "fn old()",
                "new_string": "fn new()"
            });
            let result = parse_tool_input("Edit", input);
            match result {
                ToolInput::Edit(e) => {
                    assert_eq!(e.file_path, "/src/lib.rs");
                    assert_eq!(e.old_string, "fn old()");
                    assert_eq!(e.new_string, "fn new()");
                }
                other => panic!("expected Edit, got {:?}", other),
            }
        }

        #[test]
        fn it_should_parse_optional_replace_all() {
            let input = json!({
                "file_path": "/f.rs",
                "old_string": "a",
                "new_string": "b",
                "replace_all": true
            });
            let result = parse_tool_input("Edit", input);
            match result {
                ToolInput::Edit(e) => assert_eq!(e.replace_all, Some(true)),
                other => panic!("expected Edit, got {:?}", other),
            }
        }

        #[test]
        fn it_should_fall_back_to_unknown_when_required_fields_missing() {
            let input = json!({"file_path": "/f.rs"});
            let result = parse_tool_input("Edit", input);
            match result {
                ToolInput::Unknown { name, .. } => assert_eq!(name, "Edit"),
                other => panic!("expected Unknown, got {:?}", other),
            }
        }
    }

    // describe("when tool is Write")
    mod write_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_file_path_and_content() {
            let input = json!({"file_path": "/new.rs", "content": "fn main() {}"});
            let result = parse_tool_input("Write", input);
            match result {
                ToolInput::Write(w) => {
                    assert_eq!(w.file_path, "/new.rs");
                    assert_eq!(w.content, "fn main() {}");
                }
                other => panic!("expected Write, got {:?}", other),
            }
        }
    }

    // describe("when tool is Bash")
    mod bash_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_command_as_required_field() {
            let input = json!({"command": "cargo test"});
            let result = parse_tool_input("Bash", input);
            match result {
                ToolInput::Bash(b) => assert_eq!(b.command, "cargo test"),
                other => panic!("expected Bash, got {:?}", other),
            }
        }

        #[test]
        fn it_should_parse_optional_description_timeout_background() {
            let input = json!({
                "command": "npm run build",
                "description": "Build the project",
                "timeout": 60000,
                "run_in_background": true
            });
            let result = parse_tool_input("Bash", input);
            match result {
                ToolInput::Bash(b) => {
                    assert_eq!(b.description, Some("Build the project".into()));
                    assert_eq!(b.timeout, Some(60000));
                    assert_eq!(b.run_in_background, Some(true));
                }
                other => panic!("expected Bash, got {:?}", other),
            }
        }

        #[test]
        fn it_should_fall_back_to_unknown_when_command_missing() {
            let input = json!({"description": "oops"});
            let result = parse_tool_input("Bash", input);
            match result {
                ToolInput::Unknown { name, .. } => assert_eq!(name, "Bash"),
                other => panic!("expected Unknown, got {:?}", other),
            }
        }
    }

    // describe("when tool is Glob")
    mod glob_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_pattern_and_optional_path() {
            let input = json!({"pattern": "**/*.rs", "path": "/src"});
            let result = parse_tool_input("Glob", input);
            match result {
                ToolInput::Glob(g) => {
                    assert_eq!(g.pattern, "**/*.rs");
                    assert_eq!(g.path, Some("/src".into()));
                }
                other => panic!("expected Glob, got {:?}", other),
            }
        }
    }

    // describe("when tool is Grep")
    mod grep_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_pattern_and_optional_fields() {
            let input = json!({
                "pattern": "fn main",
                "path": "/src",
                "glob": "*.rs",
                "output_mode": "content",
                "-i": true,
                "-A": 3
            });
            let result = parse_tool_input("Grep", input);
            match result {
                ToolInput::Grep(g) => {
                    assert_eq!(g.pattern, "fn main");
                    assert_eq!(g.path, Some("/src".into()));
                    assert_eq!(g.glob, Some("*.rs".into()));
                    assert_eq!(g.output_mode, Some("content".into()));
                    assert_eq!(g.case_insensitive, Some(true));
                    assert_eq!(g.after_context, Some(3));
                }
                other => panic!("expected Grep, got {:?}", other),
            }
        }
    }

    // describe("when tool is Agent")
    mod agent_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_prompt_and_optional_fields() {
            let input = json!({
                "prompt": "Find all TODO comments",
                "subagent_type": "Explore",
                "description": "search TODOs"
            });
            let result = parse_tool_input("Agent", input);
            match result {
                ToolInput::Agent(a) => {
                    assert_eq!(a.prompt, "Find all TODO comments");
                    assert_eq!(a.subagent_type, Some("Explore".into()));
                    assert_eq!(a.description, Some("search TODOs".into()));
                }
                other => panic!("expected Agent, got {:?}", other),
            }
        }
    }

    // describe("when tool is WebSearch")
    mod web_search_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_query() {
            let input = json!({"query": "rust serde tutorial"});
            let result = parse_tool_input("WebSearch", input);
            match result {
                ToolInput::WebSearch(w) => assert_eq!(w.query, "rust serde tutorial"),
                other => panic!("expected WebSearch, got {:?}", other),
            }
        }
    }

    // describe("when tool is WebFetch")
    mod web_fetch_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_url_and_optional_prompt() {
            let input = json!({"url": "https://example.com", "prompt": "summarize"});
            let result = parse_tool_input("WebFetch", input);
            match result {
                ToolInput::WebFetch(w) => {
                    assert_eq!(w.url, "https://example.com");
                    assert_eq!(w.prompt, Some("summarize".into()));
                }
                other => panic!("expected WebFetch, got {:?}", other),
            }
        }
    }

    // describe("when tool is Skill")
    mod skill_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_skill_and_optional_args() {
            let input = json!({"skill": "commit", "args": "-m 'fix'"});
            let result = parse_tool_input("Skill", input);
            match result {
                ToolInput::Skill(s) => {
                    assert_eq!(s.skill, "commit");
                    assert_eq!(s.args, Some("-m 'fix'".into()));
                }
                other => panic!("expected Skill, got {:?}", other),
            }
        }
    }

    // describe("when tool is unknown or MCP tool")
    mod unknown_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_produce_unknown_for_unrecognized_tool_name() {
            let input = json!({"server": "slack", "channel": "#general"});
            let result = parse_tool_input("mcp__slack__send_message", input);
            match result {
                ToolInput::Unknown { name, raw } => {
                    assert_eq!(name, "mcp__slack__send_message");
                    assert_eq!(raw["server"], "slack");
                }
                other => panic!("expected Unknown, got {:?}", other),
            }
        }

        #[test]
        fn it_should_preserve_raw_input_for_display() {
            let input = json!({"custom_field": 42});
            let result = parse_tool_input("FutureTool", input);
            match result {
                ToolInput::Unknown { raw, .. } => {
                    assert_eq!(raw["custom_field"], 42);
                }
                other => panic!("expected Unknown, got {:?}", other),
            }
        }
    }

    // describe("parameterless tools")
    mod parameterless_tools {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_task_list_with_no_fields() {
            let input = json!({});
            let result = parse_tool_input("TaskList", input);
            assert!(matches!(result, ToolInput::TaskList));
        }

        #[test]
        fn it_should_parse_enter_plan_mode_with_no_fields() {
            let input = json!({});
            let result = parse_tool_input("EnterPlanMode", input);
            assert!(matches!(result, ToolInput::EnterPlanMode));
        }
    }

    // describe("task management tools")
    mod task_tools {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_task_create() {
            let input = json!({
                "subject": "Fix bug",
                "description": "The login page crashes"
            });
            let result = parse_tool_input("TaskCreate", input);
            match result {
                ToolInput::TaskCreate(t) => {
                    assert_eq!(t.subject, "Fix bug");
                    assert_eq!(t.description, "The login page crashes");
                }
                other => panic!("expected TaskCreate, got {:?}", other),
            }
        }

        #[test]
        fn it_should_parse_task_get() {
            let input = json!({"task_id": "abc-123"});
            let result = parse_tool_input("TaskGet", input);
            match result {
                ToolInput::TaskGet(t) => assert_eq!(t.task_id, "abc-123"),
                other => panic!("expected TaskGet, got {:?}", other),
            }
        }
    }

    // describe("LSP tool")
    mod lsp_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_lsp_with_arbitrary_fields() {
            let input = json!({"method": "textDocument/definition", "uri": "file:///src/main.rs"});
            let result = parse_tool_input("LSP", input);
            match result {
                ToolInput::Lsp(l) => {
                    assert_eq!(l.fields["method"], "textDocument/definition");
                }
                other => panic!("expected Lsp, got {:?}", other),
            }
        }
    }

    // describe("NotebookEdit tool")
    mod notebook_edit_tool {
        use super::*;
        use crate::tool_input::{parse_tool_input, ToolInput};

        #[test]
        fn it_should_parse_notebook_path_and_new_source() {
            let input = json!({
                "notebook_path": "/notebook.ipynb",
                "new_source": "print('hello')",
                "cell_type": "code",
                "edit_mode": "replace"
            });
            let result = parse_tool_input("NotebookEdit", input);
            match result {
                ToolInput::NotebookEdit(n) => {
                    assert_eq!(n.notebook_path, "/notebook.ipynb");
                    assert_eq!(n.new_source, "print('hello')");
                    assert_eq!(n.cell_type, Some("code".into()));
                }
                other => panic!("expected NotebookEdit, got {:?}", other),
            }
        }
    }

    // describe("ToolInput serialization roundtrip")
    mod serialization {
        use crate::tool_input::{parse_tool_input, ToolInput};
        use super::*;

        #[test]
        fn it_should_roundtrip_through_json() {
            let input = json!({"command": "ls -la", "description": "list files"});
            let parsed = parse_tool_input("Bash", input);
            let json = serde_json::to_value(&parsed).unwrap();
            let deserialized: ToolInput = serde_json::from_value(json).unwrap();
            match deserialized {
                ToolInput::Bash(b) => assert_eq!(b.command, "ls -la"),
                other => panic!("expected Bash, got {:?}", other),
            }
        }
    }
}
