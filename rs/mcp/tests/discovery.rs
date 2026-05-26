//! Integration tests for the search + project-scoped tools wired in
//! Commit D: `search`, `agent_search`, `project_context`, `recent_files`.

mod common;

use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::{json, Value};

fn fresh_server() -> Server<LoopbackSubscriber> {
    let (store, plan_store, tmp) = make_test_store();
    std::mem::forget(tmp);
    Server::new(LoopbackSubscriber::new(), store, plan_store)
}

// ── search ────────────────────────────────────────────────────────

mod search {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_results() {
        let response = call_tool(fresh_server(), "search", json!({"query": "anything"})).await;
        let result = unwrap_tool_result(&response).expect("search must succeed");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_query_arg_returns_is_error_true() {
        let response = call_tool(fresh_server(), "search", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn empty_query_string_returns_is_error_true() {
        let response = call_tool(fresh_server(), "search", json!({"query": ""})).await;
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn limit_arg_is_accepted() {
        let response = call_tool(
            fresh_server(),
            "search",
            json!({"query": "test", "limit": 5}),
        )
        .await;
        let _result = unwrap_tool_result(&response).expect("limit must not break");
    }
}

// ── agent_search ─────────────────────────────────────────────────

mod agent_search {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_query_with_empty_results_array() {
        let response =
            call_tool(fresh_server(), "agent_search", json!({"query": "anything"})).await;
        let result: Value = unwrap_tool_result(&response).expect("agent_search must succeed");
        assert_eq!(result["query"], "anything");
        assert!(result["results"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_query_returns_is_error_true() {
        let response = call_tool(fresh_server(), "agent_search", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn project_filter_is_accepted() {
        let response = call_tool(
            fresh_server(),
            "agent_search",
            json!({"query": "anything", "project": "openstory"}),
        )
        .await;
        let _result = unwrap_tool_result(&response).expect("project filter must not break");
    }
}

// ── project_context ──────────────────────────────────────────────

mod project_context {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let response = call_tool(
            fresh_server(),
            "project_context",
            json!({"project": "some-project"}),
        )
        .await;
        let result = unwrap_tool_result(&response).expect("project_context must succeed");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_project_arg_returns_is_error_true() {
        let response = call_tool(fresh_server(), "project_context", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── recent_files ────────────────────────────────────────────────

mod recent_files {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let response = call_tool(
            fresh_server(),
            "recent_files",
            json!({"project": "some-project"}),
        )
        .await;
        let result = unwrap_tool_result(&response).expect("recent_files must succeed");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn missing_project_arg_returns_is_error_true() {
        let response = call_tool(fresh_server(), "recent_files", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }
}
