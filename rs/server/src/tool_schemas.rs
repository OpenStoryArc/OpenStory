//! Static tool schema registry for dashboard rendering.
//!
//! Port of Python tool_schemas.py.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct FieldMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub type_label: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSchemaMeta {
    pub tool_name: String,
    pub fields: Vec<FieldMeta>,
    pub display_fields: Vec<String>,
    pub display_formatter: String,
}

fn f(name: &str, type_label: &str, required: bool) -> FieldMeta {
    FieldMeta {
        name: name.to_string(),
        type_label: type_label.to_string(),
        required,
    }
}

fn fr(name: &str) -> FieldMeta {
    f(name, "str", true)
}

fn fo(name: &str) -> FieldMeta {
    f(name, "str", false)
}

/// Build the full tool schema registry as a JSON Value.
pub fn schemas_to_json() -> Value {
    let schemas: Vec<ToolSchemaMeta> = vec![
        ToolSchemaMeta {
            tool_name: "Read".into(),
            fields: vec![fr("file_path"), f("offset", "int", false), f("limit", "int", false), fo("pages")],
            display_fields: vec!["file_path".into()],
            display_formatter: "file_basename".into(),
        },
        ToolSchemaMeta {
            tool_name: "Edit".into(),
            fields: vec![fr("file_path"), fr("old_string"), fr("new_string"), f("replace_all", "bool", false)],
            display_fields: vec!["file_path".into()],
            display_formatter: "file_basename".into(),
        },
        ToolSchemaMeta {
            tool_name: "Write".into(),
            fields: vec![fr("file_path"), fr("content")],
            display_fields: vec!["file_path".into()],
            display_formatter: "file_basename".into(),
        },
        ToolSchemaMeta {
            tool_name: "Bash".into(),
            fields: vec![fr("command"), fo("description"), f("timeout", "int", false), f("run_in_background", "bool", false)],
            display_fields: vec!["command".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "Grep".into(),
            fields: vec![fr("pattern"), fo("path"), fo("glob"), fo("type"), fo("output_mode"), f("head_limit", "int", false)],
            display_fields: vec!["pattern".into(), "path".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "Glob".into(),
            fields: vec![fr("pattern"), fo("path")],
            display_fields: vec!["pattern".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "Agent".into(),
            fields: vec![fr("prompt"), fr("subagent_type"), fr("description"), fo("model"), fo("isolation")],
            display_fields: vec!["subagent_type".into(), "description".into()],
            display_formatter: "subagent".into(),
        },
        ToolSchemaMeta {
            tool_name: "WebFetch".into(),
            fields: vec![fr("url"), fr("prompt")],
            display_fields: vec!["url".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "WebSearch".into(),
            fields: vec![fr("query")],
            display_fields: vec!["query".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskCreate".into(),
            fields: vec![fr("subject"), fr("description"), fo("activeForm")],
            display_fields: vec!["subject".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskUpdate".into(),
            fields: vec![fr("taskId"), fo("status"), fo("subject")],
            display_fields: vec!["taskId".into(), "status".into()],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskGet".into(),
            fields: vec![fr("taskId")],
            display_fields: vec!["taskId".into()],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskList".into(),
            fields: vec![],
            display_fields: vec![],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "NotebookEdit".into(),
            fields: vec![fr("notebook_path"), fr("new_source"), fo("cell_id"), fo("cell_type"), fo("edit_mode")],
            display_fields: vec!["notebook_path".into()],
            display_formatter: "file_basename".into(),
        },
        ToolSchemaMeta {
            tool_name: "Skill".into(),
            fields: vec![fr("skill"), fo("args")],
            display_fields: vec!["skill".into()],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "AskUserQuestion".into(),
            fields: vec![f("questions", "list[dict]", true)],
            display_fields: vec!["questions".into()],
            display_formatter: "truncate".into(),
        },
        ToolSchemaMeta {
            tool_name: "EnterPlanMode".into(),
            fields: vec![],
            display_fields: vec![],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "ExitPlanMode".into(),
            fields: vec![fo("plan")],
            display_fields: vec!["plan".into()],
            display_formatter: "plan_title".into(),
        },
        ToolSchemaMeta {
            tool_name: "EnterWorktree".into(),
            fields: vec![fo("name")],
            display_fields: vec!["name".into()],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskOutput".into(),
            fields: vec![fr("task_id"), f("block", "bool", false), f("timeout", "int", false)],
            display_fields: vec!["task_id".into()],
            display_formatter: "literal".into(),
        },
        ToolSchemaMeta {
            tool_name: "TaskStop".into(),
            fields: vec![fo("task_id")],
            display_fields: vec!["task_id".into()],
            display_formatter: "literal".into(),
        },
    ];

    let mut map = serde_json::Map::new();
    for schema in schemas {
        map.insert(
            schema.tool_name.clone(),
            json!({
                "tool_name": schema.tool_name,
                "fields": schema.fields.iter().map(|f| json!({
                    "name": f.name,
                    "type": f.type_label,
                    "required": f.required,
                })).collect::<Vec<_>>(),
                "display_fields": schema.display_fields,
                "display_formatter": schema.display_formatter,
            }),
        );
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schemas_to_json_has_all_tools() {
        let json = schemas_to_json();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("Read"));
        assert!(obj.contains_key("Edit"));
        assert!(obj.contains_key("Bash"));
        assert!(obj.contains_key("Agent"));
        assert!(obj.contains_key("ExitPlanMode"));
        assert!(obj.len() >= 20);
    }

    #[test]
    fn test_schema_structure() {
        let json = schemas_to_json();
        let read = &json["Read"];
        assert_eq!(read["tool_name"], "Read");
        assert_eq!(read["display_formatter"], "file_basename");
        let fields = read["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["name"] == "file_path" && f["required"] == true));
    }
}
