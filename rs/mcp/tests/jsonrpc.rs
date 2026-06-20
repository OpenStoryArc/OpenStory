//! Protocol-layer integration tests for open-story-mcp.
//!
//! Each `describe / it` from `docs/research/streaming-mcp/TESTS.md`
//! corresponds to one `#[test]` here. Red first.

use open_story_mcp::protocol;
use serde_json::json;

// ── A.0 Protocol framing ───────────────────────────────────────────

mod when_an_mcp_client_sends_a_malformed_json_rpc_request {
    use super::*;

    #[test]
    fn it_returns_a_parse_error_response_with_id_null() {
        let raw = "{not valid json";
        let response =
            protocol::handle_message(raw).expect("malformed input should still produce a response");

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], json!(null));
        assert_eq!(response["error"]["code"], -32700);
        assert_eq!(response["error"]["message"], "Parse error");
    }
}

mod when_an_mcp_client_sends_a_request_with_an_unknown_method {
    use super::*;

    #[test]
    fn it_returns_method_not_found_preserving_the_id() {
        let raw = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "this_method_does_not_exist",
        })
        .to_string();
        let response = protocol::handle_message(&raw).expect("requests always produce a response");

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 42);
        assert_eq!(response["error"]["code"], -32601);
    }
}

// ── A.1 Initialize handshake ───────────────────────────────────────

mod when_an_mcp_client_sends_initialize {
    use super::*;

    #[test]
    fn it_responds_with_server_info_and_tools_capability() {
        let raw = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {"name": "test-client", "version": "0.0.0"},
                "capabilities": {}
            }
        })
        .to_string();
        let response =
            protocol::handle_message(&raw).expect("initialize is a request — must respond");

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);

        let result = &response["result"];
        assert!(
            result.is_object(),
            "initialize result must be an object, got: {result}"
        );
        assert_eq!(result["serverInfo"]["name"], "open-story-mcp");
        assert!(
            result["serverInfo"]["version"].is_string(),
            "serverInfo.version must be a string"
        );
        assert!(
            result["capabilities"]["tools"].is_object(),
            "capabilities.tools must be an object (even if empty)"
        );
        // Server echoes the client's protocolVersion when supported.
        assert_eq!(result["protocolVersion"], "2024-11-05");
    }
}

mod when_an_mcp_client_sends_notifications_initialized {
    use super::*;

    #[test]
    fn it_emits_no_response() {
        let raw = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })
        .to_string();
        let response = protocol::handle_message(&raw);
        assert!(
            response.is_none(),
            "notifications never get a response, got: {response:?}"
        );
    }
}

// `tools/call` dispatch tests moved to `tests/sessions.rs` (and
// `tests/streaming.rs` for subscribe_*) because dispatch now needs
// the async runtime + a Server context. `protocol::handle_message`
// no longer routes tools/call directly — it requires the stdio layer.

// ── A.2 Tool registry — tools/list ────────────────────────────────

mod when_an_mcp_client_calls_tools_list {
    use super::*;

    fn call_tools_list() -> serde_json::Value {
        let raw = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/list"
        })
        .to_string();
        protocol::handle_message(&raw).expect("tools/list is a request")
    }

    #[test]
    fn it_returns_the_seed_tool_set() {
        let response = call_tools_list();
        assert_eq!(response["id"], 7);
        let tools = response["result"]["tools"]
            .as_array()
            .expect("result.tools must be an array");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

        // Stage A seed surface — these are the tools we ship first.
        for required in ["list_sessions", "session_synopsis", "project_pulse"] {
            assert!(
                names.contains(&required),
                "tools/list must include {required}, got: {names:?}"
            );
        }
    }

    #[test]
    fn each_tool_has_name_description_and_input_schema() {
        let response = call_tools_list();
        let tools = response["result"]["tools"].as_array().unwrap();
        for tool in tools {
            let name = tool["name"].as_str().expect("tool.name must be a string");
            assert!(!name.is_empty(), "tool name must not be empty");
            assert!(
                tool["description"]
                    .as_str()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false),
                "tool {name} must have a non-empty description"
            );
            // MCP requires inputSchema to be a JSON Schema object.
            assert!(
                tool["inputSchema"].is_object(),
                "tool {name}: inputSchema must be a JSON Schema object"
            );
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "tool {name}: inputSchema.type must be 'object'"
            );
        }
    }
}
