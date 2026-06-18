//! bash-shape — the shell dialect of a session.
//!
//! Ported from `scripts/build_bash_shapes.py`. For every `Bash` tool call,
//! tokenize the command into program / subcommand / flags / args, plus
//! pipeline/redirect flags detected by a quote-aware scan. Single row per event.

use open_story_core::cloud_event::CloudEvent;
use serde_json::json;

use crate::{ShapeExtractor, ShapeRow};

pub const SHAPE_TYPE: &str = "bash-shape";

/// Programs whose first non-flag argument is a meaningful subcommand.
const SUBCOMMAND_PROGRAMS: &[&str] = &[
    "git", "cargo", "npm", "just", "gh", "docker", "kubectl", "brew", "uv", "pip",
    "pip3", "poetry", "rustup", "nats", "rg",
];

pub struct BashShape;

impl ShapeExtractor for BashShape {
    fn shape_type(&self) -> &str {
        SHAPE_TYPE
    }

    fn extract(&self, event: &CloudEvent, session_id: &str) -> Vec<ShapeRow> {
        let ap = match event.data.agent_payload.as_ref() {
            Some(ap) => ap,
            None => return vec![],
        };
        if ap.tool() != Some("Bash") {
            return vec![];
        }
        let command = match ap.args().and_then(|a| a.get("command")).and_then(|c| c.as_str()) {
            Some(c) if !c.trim().is_empty() => c,
            _ => return vec![],
        };

        let d = decompose(command);
        vec![ShapeRow::new(event, session_id, SHAPE_TYPE, 0, d)]
    }
}

/// Tokenize a shell command. Mirrors the Python `decompose()` exactly.
pub fn decompose(command: &str) -> serde_json::Value {
    // shlex.split(posix=True), falling back to whitespace split on parse error.
    let tokens: Vec<String> =
        shlex::split(command).unwrap_or_else(|| command.split_whitespace().map(String::from).collect());

    let mut program = tokens.first().cloned().unwrap_or_default();
    // strip absolute-path prefix: /usr/bin/git -> git
    if let Some(idx) = program.rfind('/') {
        program = program[idx + 1..].to_string();
    }
    let rest: Vec<&String> = tokens.iter().skip(1).collect();

    let mut subcommand = String::new();
    if SUBCOMMAND_PROGRAMS.contains(&program.as_str()) {
        for t in &rest {
            if !t.starts_with('-') {
                subcommand = (*t).clone();
                break;
            }
        }
    }

    let flags: Vec<String> = rest.iter().filter(|t| t.starts_with('-')).map(|t| (*t).clone()).collect();
    let args: Vec<String> = rest
        .iter()
        .filter(|t| !t.starts_with('-') && ***t != subcommand)
        .map(|t| (*t).clone())
        .collect();

    json!({
        "program": program,
        "subcommand": subcommand,
        "flags": flags,
        "args": args,
        "is_pipeline": if outside_quotes(command, '|') { 1 } else { 0 },
        "is_redirect": if outside_quotes(command, '>') { 1 } else { 0 },
        "raw_command": command,
    })
}

/// True if `ch` appears in `command` outside single/double quotes.
/// Mirrors the Python `_outside_quotes()` scanner (handles backslash escapes).
fn outside_quotes(command: &str, ch: char) -> bool {
    let chars: Vec<char> = command.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        } else if c == ch && !in_single && !in_double {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures lifted from `scripts/_shape_spec_fixtures.py` (the spec).
    #[test]
    fn matches_python_decompose_fixtures() {
        assert_eq!(
            decompose("git commit -m 'init'"),
            json!({"program":"git","subcommand":"commit","flags":["-m"],"args":["init"],
                   "is_pipeline":0,"is_redirect":0,"raw_command":"git commit -m 'init'"})
        );
        assert_eq!(
            decompose("cargo test -p open-story-shapes"),
            json!({"program":"cargo","subcommand":"test","flags":["-p"],"args":["open-story-shapes"],
                   "is_pipeline":0,"is_redirect":0,"raw_command":"cargo test -p open-story-shapes"})
        );
        // absolute-path program is stripped to basename
        let d = decompose("/usr/bin/git status");
        assert_eq!(d["program"], "git");
        assert_eq!(d["subcommand"], "status");
        // pipe + redirect detected; | and > become args under shlex
        let d = decompose("cat foo.txt | grep bar > out.txt");
        assert_eq!(d["program"], "cat");
        assert_eq!(d["is_pipeline"], 1);
        assert_eq!(d["is_redirect"], 1);
        // quoted pipe is NOT a pipeline; && is an ordinary token
        let d = decompose("echo \"a | b\" && npm run build");
        assert_eq!(d["program"], "echo");
        assert_eq!(d["is_pipeline"], 0);
        assert_eq!(d["args"], json!(["a | b", "&&", "npm", "run", "build"]));
    }

    fn tool_event(event_id: &str, seq: u64, tool: &str, args: serde_json::Value) -> CloudEvent {
        use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
        let mut p = ClaudeCodePayload::new();
        p.tool = Some(tool.to_string());
        p.args = Some(args);
        let data = EventData::with_payload(json!({}), seq, "parent-sess".to_string(),
                                           AgentPayload::ClaudeCode(p));
        CloudEvent::new(
            "test".to_string(),
            "io.arc.event".to_string(),
            data,
            Some("message.assistant.tool_use".to_string()),
            Some(event_id.to_string()),
            Some("2026-01-01T00:00:00Z".to_string()),
            None,
            None,
            Some("claude-code".to_string()),
        )
    }

    fn bash_event(command: &str) -> CloudEvent {
        tool_event("evt-1", 7, "Bash", json!({"command": command}))
    }

    #[test]
    fn extract_stamps_passed_session_id_not_event_session() {
        let ev = bash_event("git status");
        let rows = BashShape.extract(&ev, "own-sess");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "own-sess"); // batch id, not "parent-sess"
        assert_eq!(rows[0].shape_type, "bash-shape");
        assert_eq!(rows[0].id, "evt-1:bash-shape:0");
        assert_eq!(rows[0].seq, 7);
        assert_eq!(rows[0].data["program"], "git");
    }

    #[test]
    fn non_bash_and_empty_yield_nothing() {
        let read_ev = tool_event("e", 1, "Read", json!({"file_path": "/x"}));
        assert!(BashShape.extract(&read_ev, "s").is_empty());
        assert!(BashShape.extract(&bash_event("   "), "s").is_empty());
    }
}
