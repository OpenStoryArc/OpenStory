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
            "tools": {}
        }
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
}
