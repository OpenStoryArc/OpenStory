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
    use open_story_core::reader::read_new_lines;
    use open_story_core::translate::TranscriptState;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn empty_store_returns_zero_records_with_full_schema() {
        let response = call_tool(
            fresh_server(),
            "session_story",
            json!({"session_id": "sid-x"}),
        )
        .await;
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
        assert!(result["sample_sentences"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
        assert!(result["prompt_timeline"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
        assert!(result["trailing_assistant"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false));
        // Optional prompts are null when no prompts.
        assert!(result["opening_prompt"].is_null());
        assert!(result["closing_prompt"].is_null());
    }

    #[tokio::test]
    async fn missing_session_id_returns_is_error_true() {
        let response = call_tool(fresh_server(), "session_story", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn codex_rollout_events_flow_into_prompt_timeline_and_tool_counts() {
        let session_id = "codex-story-session";
        let mut file = NamedTempFile::new().expect("create codex fixture");
        writeln!(
            file,
            r#"{{"timestamp":"2026-05-23T12:11:57.908Z","type":"event_msg","payload":{{"type":"user_message","message":"Can you use the codex session data as a foundation to TDD the Codex adapter?","images":[],"local_images":[],"text_elements":[]}}}}"#
        )
        .expect("write codex prompt");
        writeln!(
            file,
            r#"{{"timestamp":"2026-05-23T12:12:00.552Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"pwd\",\"workdir\":\"/Users/maxglassie/projects/OpenStory\",\"yield_time_ms\":1000,\"max_output_tokens\":2000}}","call_id":"call_pwd"}}}}"#
        )
        .expect("write codex tool call");
        writeln!(
            file,
            r#"{{"timestamp":"2026-05-23T12:12:00.614Z","type":"response_item","payload":{{"type":"function_call_output","call_id":"call_pwd","output":"Output:\n/Users/maxglassie/projects/OpenStory\n"}}}}"#
        )
        .expect("write codex tool result");
        writeln!(
            file,
            r#"{{"timestamp":"2026-05-23T12:12:01.896Z","type":"event_msg","payload":{{"type":"agent_message","message":"Yes. The next slice is the MCP story projection.","phase":"final_answer","memory_citation":null}}}}"#
        )
        .expect("write codex assistant message");
        file.flush().expect("flush codex fixture");

        let mut state = TranscriptState::new(session_id.to_string());
        let events = read_new_lines(file.path(), &mut state).expect("translate codex fixture");

        let (store, plan_store, tmp) = make_test_store();
        let values: Vec<_> = events
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .expect("serialize cloud events");
        assert_eq!(
            store
                .insert_batch(session_id, &values)
                .await
                .expect("insert events"),
            4
        );

        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);
        let response = call_tool(server, "session_story", json!({"session_id": session_id})).await;
        drop(tmp);

        let result = unwrap_tool_result(&response).expect("session_story must succeed");
        assert_eq!(result["total_records"], 4);
        assert_eq!(result["record_type_counts"]["user_message"], 1);
        assert_eq!(result["record_type_counts"]["tool_call"], 1);
        assert_eq!(result["record_type_counts"]["tool_result"], 1);
        assert_eq!(result["record_type_counts"]["assistant_message"], 1);
        assert_eq!(result["tool_call_counts"]["exec_command"], 1);
        assert_eq!(
            result["opening_prompt"]["content"],
            "Can you use the codex session data as a foundation to TDD the Codex adapter?"
        );
        assert_eq!(
            result["trailing_assistant"][0]["content"],
            "Yes. The next slice is the MCP story projection."
        );
    }
}
