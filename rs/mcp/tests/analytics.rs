//! Integration tests for the analytics tools wired in Commit E:
//! `token_usage`, `daily_token_usage`, `productivity`.

mod common;

use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::json;

fn fresh_server() -> Server<LoopbackSubscriber> {
    let (store, plan_store, tmp) = make_test_store();
    std::mem::forget(tmp);
    Server::new(LoopbackSubscriber::new(), store, plan_store)
}

// ── token_usage ────────────────────────────────────────────────

mod token_usage {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_zero_counts() {
        let response = call_tool(fresh_server(), "token_usage", json!({})).await;
        let result = unwrap_tool_result(&response).expect("token_usage must succeed");
        assert_eq!(result["session_count"], 0);
        // TokenUsage fields are flattened at the top level.
        assert_eq!(result["input_tokens"], 0);
        assert_eq!(result["output_tokens"], 0);
        // Cache fields exist even on empty store (Option<u64>, flattened
        // via #[serde(skip_serializing_if = "None")] → absent OR 0).
        assert!(result["sessions"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn model_arg_changes_cost_estimate_shape() {
        // Even on empty store, the cost estimate object is present and
        // varies by model. Hitting opus pricing path here just to
        // confirm the arg is plumbed through.
        let response = call_tool(
            fresh_server(),
            "token_usage",
            json!({"model": "opus", "days": 7}),
        )
        .await;
        let result = unwrap_tool_result(&response).expect("model arg must not break");
        assert!(
            result["cost"].is_object(),
            "result must include cost estimate"
        );
    }

    #[tokio::test]
    async fn session_id_scope_is_accepted() {
        let response = call_tool(
            fresh_server(),
            "token_usage",
            json!({"session_id": "sid-x"}),
        )
        .await;
        let _result = unwrap_tool_result(&response).expect("session scope must not break");
    }
}

// ── daily_token_usage ──────────────────────────────────────────

mod daily_token_usage {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let response = call_tool(fresh_server(), "daily_token_usage", json!({"days": 7})).await;
        let result = unwrap_tool_result(&response).expect("daily_token_usage must succeed");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn no_args_uses_default_window() {
        let response = call_tool(fresh_server(), "daily_token_usage", json!({})).await;
        let result = unwrap_tool_result(&response).expect("default window must work");
        assert!(result.as_array().is_some());
    }
}

// ── productivity ───────────────────────────────────────────────

mod productivity {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_empty_array() {
        let response = call_tool(fresh_server(), "productivity", json!({"days": 30})).await;
        let result = unwrap_tool_result(&response).expect("productivity must succeed");
        assert!(result.as_array().map(|a| a.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn no_args_uses_default_30_day_window() {
        let response = call_tool(fresh_server(), "productivity", json!({})).await;
        let _result = unwrap_tool_result(&response).expect("default window must work");
    }
}
