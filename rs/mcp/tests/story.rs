//! Integration tests for the native session_story tool (Commit F).

mod common;

use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::json;

fn fresh_server() -> Server<LoopbackSubscriber> {
    let (store, plan_store, tmp) = make_test_store();
    std::mem::forget(tmp);
    Server::new(LoopbackSubscriber::new(), store, plan_store)
}

mod session_story {
    use super::*;

    #[tokio::test]
    async fn empty_store_returns_zero_records_with_full_schema() {
        let response =
            call_tool(fresh_server(), "session_story", json!({"session_id": "sid-x"})).await;
        let result = unwrap_tool_result(&response).expect("session_story must succeed");

        // Top-level fields match the scripts/sessionstory.py SessionFacts schema.
        assert_eq!(result["session_id"], "sid-x");
        assert_eq!(result["total_records"], 0);
        assert_eq!(result["turn_count"], 0);
        assert_eq!(result["sidechain_count"], 0);
        assert_eq!(result["pattern_total"], 0);
        assert_eq!(result["error_recovery_count"], 0);
        assert_eq!(result["test_cycle_count"], 0);

        // Container fields default to empty.
        assert!(result["record_type_counts"].is_object());
        assert!(result["tool_call_counts"].is_object());
        assert!(result["pattern_type_counts"].is_object());
        assert!(result["turn_phase_counts"].is_object());
        assert!(result["sample_sentences"].as_array().map(|a| a.is_empty()).unwrap_or(false));
        assert!(result["prompt_timeline"].as_array().map(|a| a.is_empty()).unwrap_or(false));
        assert!(result["trailing_assistant"].as_array().map(|a| a.is_empty()).unwrap_or(false));
        // Optional prompts are null when no prompts.
        assert!(result["opening_prompt"].is_null());
        assert!(result["closing_prompt"].is_null());
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = call_tool(fresh_server(), "session_story", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }
}
