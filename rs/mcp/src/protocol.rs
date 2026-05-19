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
    let Some(id) = id else {
        return None;
    };

    Some(match method {
        "initialize" => handle_initialize(id, params),
        "tools/list" => JsonRpcResponse::success(id, crate::tools::list_tools_result()),
        "tools/call" => handle_tools_call(id, params),
        _ => JsonRpcResponse::failure(id, error_code::METHOD_NOT_FOUND, "Method not found"),
    })
}

fn handle_tools_call(id: Value, params: Value) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let result = crate::tools::dispatch_tool_call(name, args);
    JsonRpcResponse::success(id, result)
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
