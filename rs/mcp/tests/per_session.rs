//! Integration tests for the per-session detail tools wired in
//! Commit C: `tool_journey`, `file_impact`, `session_errors`,
//! `session_patterns`, `session_sentences`.
//!
//! Empty-store baselines + missing-arg error paths. Seeded-event tests
//! land later — those exercise the full event-shape contract which
//! is already covered by the queries.rs unit tests in open-story-store;
//! the MCP tests' job is to verify the wrapper layer parses args
//! correctly and shapes the response.

mod common;

use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::{json, Value};

fn fresh_server() -> Server<LoopbackSubscriber> {
    let (store, plan_store, tmp) = make_test_store();
    std::mem::forget(tmp); // keep the temp dir alive for the test's duration
    Server::new(LoopbackSubscriber::new(), store, plan_store)
}

async fn empty_array_response(tool: &str) -> Value {
    let response = call_tool(fresh_server(), tool, json!({"session_id": "sid-x"})).await;
    unwrap_tool_result(&response).expect("tool must succeed on empty store")
}

async fn missing_arg_response(tool: &str) -> Value {
    call_tool(fresh_server(), tool, json!({})).await
}

// ── tool_journey ──────────────────────────────────────────────────

mod tool_journey {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let result = empty_array_response("tool_journey").await;
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("tool_journey").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── file_impact ───────────────────────────────────────────────────

mod file_impact {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let result = empty_array_response("file_impact").await;
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("file_impact").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_errors ───────────────────────────────────────────────

mod session_errors {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let result = empty_array_response("session_errors").await;
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_errors").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_patterns ─────────────────────────────────────────────

mod session_patterns {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let result = empty_array_response("session_patterns").await;
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn pattern_type_filter_is_accepted() {
        let response = call_tool(
            fresh_server(),
            "session_patterns",
            json!({"session_id": "sid-x", "pattern_type": "turn.sentence"}),
        )
        .await;
        let result =
            unwrap_tool_result(&response).expect("filter arg must not break the call");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_patterns").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_sentences ───────────────────────────────────────────

mod session_sentences {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_zero_count_with_empty_sentences() {
        let result = empty_array_response("session_sentences").await;
        assert_eq!(result["count"], 0);
        assert!(result["sentences"].as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_sentences").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_plans ───────────────────────────────────────────────

mod session_plans {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let result = empty_array_response("session_plans").await;
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_plans").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_transcript ─────────────────────────────────────────

mod session_transcript {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_entries_array() {
        let result = empty_array_response("session_transcript").await;
        assert!(result["entries"].as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn assistant_only_arg_is_accepted() {
        let response = call_tool(
            fresh_server(),
            "session_transcript",
            json!({"session_id": "sid-x", "assistant_only": true}),
        )
        .await;
        let result = unwrap_tool_result(&response).expect("assistant_only must not break");
        assert!(result["entries"].as_array().is_some());
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_transcript").await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── session_activity ──────────────────────────────────────────

mod session_activity {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_zero_counts_and_empty_lists() {
        let result = empty_array_response("session_activity").await;
        // ActivitySummary serialized — check the expected keys exist.
        assert!(result["first_prompt"].is_null() || result["first_prompt"].is_string());
        assert!(result["files_touched"].as_array().is_some());
        assert!(result["tool_breakdown"].is_object());
        assert!(result["error_messages"].as_array().is_some());
        assert_eq!(result["conversation_turns"], 0);
        assert_eq!(result["plan_count"], 0);
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = missing_arg_response("session_activity").await;
        assert_eq!(response["result"]["isError"], true);
    }
}
