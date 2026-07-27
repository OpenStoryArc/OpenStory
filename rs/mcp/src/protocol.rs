//! JSON-RPC 2.0 message types and pure-function dispatch.
//!
//! No I/O here. The transport layer (stdio) reads frames and hands
//! them to `handle_message`; the result is written back unchanged.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 error codes (subset we care about).
pub mod error_code {
    /// Invalid JSON was received by the server.
    pub const PARSE_ERROR: i32 = -32700;
    /// The JSON sent is not a valid Request object.
    pub const INVALID_REQUEST: i32 = -32600;
    /// The method does not exist / is not available.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameter(s).
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal JSON-RPC error.
    pub const INTERNAL_ERROR: i32 = -32603;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum JsonRpcResponse {
    Success {
        jsonrpc: &'static str,
        id: Value,
        result: Value,
    },
    Failure {
        jsonrpc: &'static str,
        id: Value,
        error: JsonRpcError,
    },
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Value {
        serde_json::to_value(Self::Success {
            jsonrpc: "2.0",
            id,
            result,
        })
        .unwrap()
    }

    pub fn failure(id: Value, code: i32, message: &str) -> Value {
        serde_json::to_value(Self::Failure {
            jsonrpc: "2.0",
            id,
            error: JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            },
        })
        .unwrap()
    }

    pub fn parse_error() -> Value {
        Self::failure(Value::Null, error_code::PARSE_ERROR, "Parse error")
    }
}

/// Server identity returned in `initialize` responses.
pub const SERVER_NAME: &str = "open-story-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Default MCP protocol version we speak when the client doesn't specify.
pub const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

/// Embedded agent-facing docs (no filesystem at runtime). Paths relative to
/// this source file (`rs/mcp/src/`).
pub const AGENT_IN_UI_DOC: &str = include_str!("../../../docs/agent-in-ui.md");
pub const HANDS_DOC: &str = include_str!("../agent-docs/hands.md");
pub const PHYSICS_DOC: &str = include_str!("../agent-docs/physics.md");
pub const EXAMPLE_PICKUP_DOC: &str = include_str!("../agent-docs/examples/pickup.md");
pub const EXAMPLE_FILE_LOCUS_DOC: &str = include_str!("../agent-docs/examples/file-locus.md");
pub const EXAMPLE_SHOW_HUMAN_DOC: &str = include_str!("../agent-docs/examples/show-human.md");

pub const AGENT_IN_UI_URI: &str = "openstory://docs/agent-in-ui";
pub const HANDS_URI: &str = "openstory://docs/hands";
pub const PHYSICS_URI: &str = "openstory://docs/physics";
pub const EXAMPLE_PICKUP_URI: &str = "openstory://examples/pickup";
pub const EXAMPLE_FILE_LOCUS_URI: &str = "openstory://examples/file-locus";
pub const EXAMPLE_SHOW_HUMAN_URI: &str = "openstory://examples/show-human";

/// Agent-facing self-documentation returned in `initialize`. History hands +
/// dashboard seam — no git repo required. Depth via resources / openstory_help.
pub const INSTRUCTIONS: &str = "\
OpenStory MCP — read your fleet's coding history (observe, never rewrite) and \
optionally drive the dashboard (ui.* only). You do not need the OpenStory repo; \
this protocol surface is the body schema.

LAW: Prefer these tools over memory for past work. Cite session_id / paths / \
event ids. Do not invent events. Sentences are SVO projections of acts, not \
intent labels. ui_control never mutates observed history.

MOTIONS (need → first tools):
  orient       list_sessions → session_synopsis | session_story
  what-touched file_impact | tool_journey | session_sentences
  find         search | agent_search  (then session_story on hits)
  cost         token_usage | daily_token_usage
  live         subscribe_session | subscribe_tokens
  show-human   where_is_user → navigate_to (prefer) | ui_control
  stuck        openstory_help { need | topic }

SHOW-HUMAN (attention layer — steers the mirror only):
  navigate_to {kind, id, sessionId?, canvasMode?, details?} — ANY event / graph click
  ui_control: open_view | focus_event | present | query | toggle | set
  where_is_user / subscribe_ui_state — follow the human; drive in rests (tempo).

DEPTH (resources/read):
  openstory://docs/hands          — motions + default flows (start here)
  openstory://docs/physics        — events/turns/outcomes/sentences + soft holes
  openstory://docs/agent-in-ui    — full dashboard drive/follow map
  openstory://examples/pickup | file-locus | show-human
Or call tool openstory_help.";

/// Handle one incoming JSON-RPC message.
///
/// Returns `Some(response)` for requests (and for malformed input,
/// which gets a Parse error response with id=null per the spec).
/// Returns `None` for notifications.
pub fn handle_message(raw: &str) -> Option<Value> {
    let parsed: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Some(JsonRpcResponse::parse_error()),
    };

    let id = parsed.get("id").cloned();
    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = parsed.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no `id`) get no response.
    let id = id?;

    Some(match method {
        "initialize" => handle_initialize(id, params),
        "tools/list" => JsonRpcResponse::success(id, crate::tools::list_tools_result()),
        "resources/list" => JsonRpcResponse::success(id, resources_list_result()),
        "resources/read" => handle_resources_read(id, params),
        // `tools/call` is NOT routed here — it needs async access to
        // the store (for query tools) and the writer channel (for
        // streaming tools), both of which live in `stdio.rs`. The
        // stdio handler intercepts `tools/call` before reaching this
        // function; any tools/call that arrives here means the call
        // came in without the stdio layer wired up (test edge), which
        // is not a real use case.
        _ => JsonRpcResponse::failure(id, error_code::METHOD_NOT_FOUND, "Method not found"),
    })
}

fn handle_initialize(id: Value, params: Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_PROTOCOL_VERSION)
        .to_string();

    let result = serde_json::json!({
        "protocolVersion": protocol_version,
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION,
        },
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "instructions": INSTRUCTIONS,
    });

    JsonRpcResponse::success(id, result)
}

/// Catalog of embedded agent docs: (uri, name, description, body).
fn agent_resources() -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
    &[
        (
            HANDS_URI,
            "Hands — how to use this MCP",
            "Start here. Motions: orient, what-touched, find, cost, live, show-human. Read-only history; cite IDs.",
            HANDS_DOC,
        ),
        (
            PHYSICS_URI,
            "Physics — what is ground truth",
            "Events, turns, outcomes, sentences as projections; soft holes; citation path. No interpretation.",
            PHYSICS_DOC,
        ),
        (
            AGENT_IN_UI_URI,
            "Agent-in-UI seam",
            "How to drive, follow, and replay the OpenStory dashboard (ui.* only).",
            AGENT_IN_UI_DOC,
        ),
        (
            EXAMPLE_PICKUP_URI,
            "Example: pickup / resume",
            "Worked flow: list_sessions → session_story.",
            EXAMPLE_PICKUP_DOC,
        ),
        (
            EXAMPLE_FILE_LOCUS_URI,
            "Example: file locus",
            "Worked flow: file_impact / search → sentences.",
            EXAMPLE_FILE_LOCUS_DOC,
        ),
        (
            EXAMPLE_SHOW_HUMAN_URI,
            "Example: show the human",
            "Worked flow: where_is_user → ui_control (views, canvas, focus).",
            EXAMPLE_SHOW_HUMAN_DOC,
        ),
    ]
}

/// `resources/list` — agent curriculum readable without the git repo.
fn resources_list_result() -> Value {
    let resources: Vec<Value> = agent_resources()
        .iter()
        .map(|(uri, name, description, _)| {
            serde_json::json!({
                "uri": uri,
                "name": name,
                "description": description,
                "mimeType": "text/markdown",
            })
        })
        .collect();
    serde_json::json!({ "resources": resources })
}

/// `resources/read` — return an embedded doc's content by URI.
fn handle_resources_read(id: Value, params: Value) -> Value {
    let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    let Some((_, _, _, body)) = agent_resources().iter().find(|(u, _, _, _)| *u == uri) else {
        return JsonRpcResponse::failure(id, error_code::INVALID_PARAMS, "Unknown resource uri");
    };
    let result = serde_json::json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "text/markdown",
                "text": body,
            }
        ]
    });
    JsonRpcResponse::success(id, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adversarial JSON-RPC frames thrown at `handle_message`. The MCP
    /// transport is stdio — a local trust boundary — but the parser must
    /// still never panic or stack-overflow on hostile input, and must keep
    /// its contract: invalid JSON → Parse error; notification (no id) →
    /// None; request → Some(response). (audit-master-2026-06, MCP surface.)
    fn fuzz_corpus() -> Vec<String> {
        let mut corpus = vec![
            // ── Malformed / non-JSON ──
            String::new(),
            " ".into(),
            "\n".into(),
            "not json at all".into(),
            "{".into(),
            "}".into(),
            "[".into(),
            "{\"method\":".into(),         // truncated
            "{\"method\":\"x\",}".into(),   // trailing comma
            "\u{0}".into(),                  // bare null byte
            "{\"a\":\"\u{0}\u{1}\u{2}\"}".into(), // control chars in string
            "nan".into(),
            "Infinity".into(),
            "12345678901234567890123456789".into(), // bignum overflow
            // ── Valid JSON, hostile shapes ──
            "null".into(),
            "true".into(),
            "42".into(),
            "\"just a string\"".into(),
            "[]".into(),
            "{}".into(),                                    // no method, no id
            "{\"method\":123}".into(),                       // method wrong type
            "{\"method\":null,\"id\":1}".into(),
            "{\"id\":{\"nested\":\"object\"},\"method\":\"initialize\"}".into(),
            "{\"id\":[1,2,3],\"method\":\"tools/list\"}".into(),
            "{\"method\":\"x\"}".into(),                     // notification → None
            "{\"id\":null,\"method\":\"initialize\"}".into(),// explicit null id
            "{\"id\":1,\"method\":\"tools/call\"}".into(),   // not routed here → method-not-found
            "{\"id\":1,\"method\":\"initialize\",\"params\":\"not-an-object\"}".into(),
            "{\"id\":1,\"method\":\"initialize\",\"params\":[1,2,3]}".into(),
            "{\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":999}}".into(),
            // ── Unicode / size ──
            "{\"id\":1,\"method\":\"🦀💥\\u0000\"}".into(),
            format!("{{\"id\":1,\"method\":\"{}\"}}", "A".repeat(1_000_000)), // 1MB method
        ];
        // Deep nesting must hit serde_json's recursion limit and return a
        // Parse error, NOT overflow the stack.
        corpus.push(format!("{}{}", "[".repeat(20_000), "]".repeat(20_000)));
        corpus.push(format!(
            "{{\"id\":1,\"method\":\"initialize\",\"params\":{}{}}}",
            "{\"a\":".repeat(20_000),
            "1".to_string() + &"}".repeat(20_000)
        ));
        corpus
    }

    #[test]
    fn handle_message_never_panics_on_adversarial_input() {
        for (i, raw) in fuzz_corpus().iter().enumerate() {
            // The assertion is simply that this returns — a panic or stack
            // overflow here fails the test.
            let _ = handle_message(raw);
            // Sanity: any response that IS produced must be serializable
            // back to a string (the transport does exactly this next).
            if let Some(resp) = handle_message(raw) {
                assert!(
                    serde_json::to_string(&resp).is_ok(),
                    "frame #{i} produced an unserializable response"
                );
            }
        }
    }

    #[test]
    fn contract_invalid_json_is_parse_error() {
        let resp = handle_message("{ not json").expect("malformed input gets a response");
        assert_eq!(resp["error"]["code"], error_code::PARSE_ERROR);
        assert_eq!(resp["id"], Value::Null, "parse error carries null id");
    }

    #[test]
    fn contract_notification_without_id_gets_no_response() {
        assert!(
            handle_message("{\"method\":\"initialize\"}").is_none(),
            "a notification (no id) must produce no response"
        );
    }

    #[test]
    fn contract_deeply_nested_json_is_parse_error_not_overflow() {
        let bomb = format!("{}{}", "[".repeat(20_000), "]".repeat(20_000));
        let resp = handle_message(&bomb).expect("depth-limited input still answers");
        assert_eq!(
            resp["error"]["code"],
            error_code::PARSE_ERROR,
            "serde recursion limit must reject the bomb as a parse error"
        );
    }

    #[test]
    fn contract_unknown_method_is_method_not_found_not_panic() {
        let resp = handle_message("{\"id\":7,\"method\":\"tools/call\"}").unwrap();
        assert_eq!(resp["error"]["code"], error_code::METHOD_NOT_FOUND);
        assert_eq!(resp["id"], 7);
    }

    // ── MCP self-documentation (agent hands + UI seam) ──
    // A connecting agent learns body schema from the protocol — no git repo.

    #[test]
    fn initialize_carries_agent_facing_instructions() {
        let resp = handle_message("{\"id\":1,\"method\":\"initialize\",\"params\":{}}").unwrap();
        let instr = resp["result"]["instructions"].as_str().expect("instructions present");
        assert!(instr.contains("open_view"), "mentions a control verb");
        assert!(instr.contains("where_is_user"), "mentions the point-read tool");
        assert!(instr.contains("session_story"), "mentions history orient tool");
        assert!(instr.contains("openstory://docs/hands"), "points at hands curriculum");
        assert!(instr.contains("openstory_help"), "points at help tool");
        assert!(
            instr.contains("Do not invent") || instr.contains("do not invent"),
            "states scientific law"
        );
    }

    #[test]
    fn initialize_advertises_the_resources_capability() {
        let resp = handle_message("{\"id\":1,\"method\":\"initialize\",\"params\":{}}").unwrap();
        assert!(
            resp["result"]["capabilities"]["resources"].is_object(),
            "resources capability advertised so clients try resources/list"
        );
    }

    #[test]
    fn resources_list_includes_hands_physics_and_ui() {
        let resp = handle_message("{\"id\":2,\"method\":\"resources/list\"}").unwrap();
        let list = resp["result"]["resources"].as_array().expect("resources array");
        for uri in [HANDS_URI, PHYSICS_URI, AGENT_IN_UI_URI, EXAMPLE_PICKUP_URI] {
            let doc = list
                .iter()
                .find(|r| r["uri"] == uri)
                .unwrap_or_else(|| panic!("missing resource {uri}"));
            assert_eq!(doc["mimeType"], "text/markdown");
            assert!(doc["name"].is_string());
        }
    }

    #[test]
    fn resources_read_returns_hands_and_ui_docs() {
        for (uri, needle) in [
            (HANDS_URI, "Motions"),
            (PHYSICS_URI, "Soft holes"),
            (AGENT_IN_UI_URI, "agent-in-UI"),
            (EXAMPLE_SHOW_HUMAN_URI, "ui_control"),
        ] {
            let req = format!(
                "{{\"id\":3,\"method\":\"resources/read\",\"params\":{{\"uri\":\"{uri}\"}}}}"
            );
            let resp = handle_message(&req).unwrap();
            let contents = resp["result"]["contents"].as_array().expect("contents array");
            let first = &contents[0];
            assert_eq!(first["uri"], uri);
            let text = first["text"].as_str().expect("doc text");
            assert!(
                text.contains(needle) || text.len() > 200,
                "uri {uri} should contain {needle:?} or be substantial"
            );
        }
    }

    #[test]
    fn resources_read_unknown_uri_is_invalid_params() {
        let resp = handle_message(
            "{\"id\":4,\"method\":\"resources/read\",\"params\":{\"uri\":\"openstory://nope\"}}",
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], error_code::INVALID_PARAMS);
    }
}
