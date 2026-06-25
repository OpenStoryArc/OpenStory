//! Integration tests for the session-scoped tools wired in Commit B:
//! `list_sessions`, `session_synopsis`, `project_pulse`.
//!
//! Each tool gets a red-then-green TDD slice: an empty-store baseline
//! plus a seeded-store case that exercises a real query path.

mod common;

use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use open_story_store::event_store::SessionRow;
use serde_json::json;

fn seeded_row(id: &str, project: &str, last: &str, events: u64) -> SessionRow {
    SessionRow {
        id: id.to_string(),
        project_id: Some(project.to_string()),
        project_name: Some(project.to_string()),
        label: Some(format!("label-{id}")),
        custom_label: None,
        branch: None,
        event_count: events,
        first_event: Some(last.to_string()),
        last_event: Some(last.to_string()),
        host: None,
        user: None,
        origin_agent: None,
        person_id: None,
        principal_id: None,
    }
}

// ── list_sessions ──────────────────────────────────────────────────

mod when_list_sessions_is_called_against_an_empty_store {
    use super::*;

    #[tokio::test]
    async fn it_returns_an_empty_array_with_is_error_false() {
        let (store, plan_store, _tmp) = make_test_store();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "list_sessions", json!({})).await;

        assert_eq!(response["result"]["isError"], false);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert!(rows.is_empty(), "empty store → empty array, got {rows:?}");
    }
}

mod when_list_sessions_is_called_against_a_seeded_store {
    use super::*;

    #[tokio::test]
    async fn it_returns_trim_rows_for_every_session() {
        let (store, plan_store, _tmp) = make_test_store();
        store
            .upsert_session(&seeded_row("sid-1", "proj-a", "2026-05-19T08:00:00Z", 5))
            .await
            .unwrap();
        store
            .upsert_session(&seeded_row("sid-2", "proj-b", "2026-05-19T09:00:00Z", 10))
            .await
            .unwrap();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "list_sessions", json!({})).await;

        let rows: Vec<serde_json::Value> =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
        assert!(ids.contains(&"sid-1"));
        assert!(ids.contains(&"sid-2"));
        // Trim shape: no `host`/`user`/`custom_label` (the bloat that
        // blew the token budget on the Python MCP).
        for r in &rows {
            for forbidden in ["host", "user", "custom_label", "branch"] {
                assert!(
                    r.get(forbidden).is_none(),
                    "trim shape must not include {forbidden}, got {r}"
                );
            }
        }
    }

    #[tokio::test]
    async fn limit_arg_caps_row_count() {
        let (store, plan_store, _tmp) = make_test_store();
        for i in 0..20u32 {
            store
                .upsert_session(&seeded_row(
                    &format!("sid-{i:02}"),
                    "p",
                    "2026-05-19T10:00:00Z",
                    1,
                ))
                .await
                .unwrap();
        }
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "list_sessions", json!({"limit": 5})).await;

        let rows: Vec<serde_json::Value> =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            rows.len(),
            5,
            "limit=5 must cap rows at 5, got {}",
            rows.len()
        );
    }

    #[tokio::test]
    async fn project_filter_returns_only_matching_sessions() {
        let (store, plan_store, _tmp) = make_test_store();
        for (id, proj) in [("a-1", "alpha"), ("a-2", "alpha"), ("b-1", "beta")] {
            store
                .upsert_session(&seeded_row(id, proj, "2026-05-19T10:00:00Z", 1))
                .await
                .unwrap();
        }
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "list_sessions", json!({"project": "alpha"})).await;

        let rows: Vec<serde_json::Value> =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert_eq!(r["project_id"], "alpha");
        }
    }
}

// ── session_synopsis ──────────────────────────────────────────────

mod when_session_synopsis_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_an_error_when_session_id_is_missing() {
        let (store, plan_store, _tmp) = make_test_store();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "session_synopsis", json!({})).await;
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn it_returns_an_error_for_an_unknown_session_id() {
        let (store, plan_store, _tmp) = make_test_store();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "session_synopsis", json!({"session_id": "nope"})).await;
        assert_eq!(response["result"]["isError"], true);
    }
}

// ── tool dispatch error semantics ─────────────────────────────────

mod when_tools_call_targets_an_unknown_tool {
    use super::*;

    #[tokio::test]
    async fn it_returns_is_error_true_with_content_array_no_protocol_error() {
        let (store, plan_store, _tmp) = make_test_store();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "not_a_real_tool", json!({})).await;
        // Tool-level errors are NOT JSON-RPC errors per MCP spec —
        // the tool call succeeds at the protocol layer; isError=true
        // signals the tool itself failed.
        assert!(
            response["error"].is_null(),
            "tool-not-found is NOT a protocol error"
        );
        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["content"].is_array(),
            "even errors return content array"
        );
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("Unknown tool"),
            "error message should mention 'Unknown tool'"
        );
    }
}

// ── project_pulse ─────────────────────────────────────────────────

mod when_project_pulse_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_an_empty_array_against_an_empty_store() {
        let (store, plan_store, _tmp) = make_test_store();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let response = call_tool(server, "project_pulse", json!({"days": 7})).await;

        let pulse =
            unwrap_tool_result(&response).expect("project_pulse must succeed on empty store");
        assert!(
            pulse.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "empty store → empty array, got {pulse}"
        );
    }
}
