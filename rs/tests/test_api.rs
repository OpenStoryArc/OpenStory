//! Integration tests for GET /api/* endpoints.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, make_event, send_request, test_state};
use tempfile::TempDir;

use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use helpers::seed_and_ingest;

#[tokio::test]
async fn test_list_sessions_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["total"].as_u64(), Some(0));
    assert_eq!(body["sessions"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_list_sessions_with_data() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Pre-ingest events for 2 sessions
    {
        let mut s = state.write().await;
        let events_a = vec![
            make_event("io.arc.event", "session-a"),
            make_event("io.arc.event", "session-a"),
        ];
        seed_and_ingest(&mut s, "session-a", &events_a, None).await;

        let events_b = vec![
            make_event("io.arc.event", "session-b"),
        ];
        seed_and_ingest(&mut s, "session-b", &events_b, None).await;
    }

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["total"].as_u64(), Some(2));
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);

    let session_ids: Vec<&str> = sessions
        .iter()
        .map(|s| s["session_id"].as_str().unwrap())
        .collect();
    assert!(session_ids.contains(&"session-a"));
    assert!(session_ids.contains(&"session-b"));
}

#[tokio::test]
async fn test_get_events_existing_session() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events: Vec<_> = (0..5)
            .map(|_| make_event("io.arc.event", "sess-events"))
            .collect();
        seed_and_ingest(&mut s, "sess-events", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-events/events")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let events = body.as_array().unwrap();
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn test_get_events_unknown_session() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/events")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let events = body.as_array().unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_get_summary() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_event("io.arc.event", "sess-summary"),
            make_event("io.arc.event", "sess-summary"),
            make_event("io.arc.event", "sess-summary"),
        ];
        seed_and_ingest(&mut s, "sess-summary", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-summary/summary")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["session_id"], "sess-summary");
    assert_eq!(body["event_count"], 3);
}

#[tokio::test]
async fn test_get_tool_schemas() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/tool-schemas")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    // Should be a non-empty JSON value (object or array)
    assert!(!body.is_null());
}

#[tokio::test]
async fn test_list_sessions_includes_project_id() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "session-with-project")];
        seed_and_ingest(&mut s, "session-with-project", &events, Some("my-project")).await;

        let events2 = vec![make_event("io.arc.event", "session-no-project")];
        seed_and_ingest(&mut s, "session-no-project", &events2, None).await;
    }

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);

    for session in sessions {
        let sid = session["session_id"].as_str().unwrap();
        if sid == "session-with-project" {
            assert_eq!(session["project_id"].as_str(), Some("my-project"));
        } else {
            assert!(session["project_id"].is_null());
        }
    }
}

#[tokio::test]
async fn test_get_summary_includes_project_id() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-proj")];
        seed_and_ingest(&mut s, "sess-proj", &events, Some("open-story")).await;
    }

    let req = Request::get("/api/sessions/sess-proj/summary")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["project_id"].as_str(), Some("open-story"));
}

#[tokio::test]
async fn test_cors_allows_localhost_origin() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // CORS only returns allow-origin when request includes a matching Origin header
    let req = Request::get("/api/sessions")
        .header("Origin", "http://localhost:5173")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let cors = resp.headers().get("access-control-allow-origin");
    assert!(cors.is_some(), "CORS header should be present for localhost origin");
    assert_eq!(cors.unwrap(), "http://localhost:5173");
}

#[tokio::test]
async fn test_cors_rejects_unknown_origin() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Request from a non-localhost origin should not get CORS header
    let req = Request::get("/api/sessions")
        .header("Origin", "http://evil.example.com")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let cors = resp.headers().get("access-control-allow-origin");
    assert!(cors.is_none(), "CORS should not allow unknown origins");
}

// ── Activity endpoint ──────────────────────────────────────────────

fn make_rich_event(event_type: &str, session_id: &str, subtype: Option<&str>) -> open_story::cloud_event::CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("test".to_string());
    payload.tool = Some("Read".to_string());
    let data = EventData::with_payload(
        serde_json::json!({}),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    open_story::cloud_event::CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        event_type.to_string(),
        data,
        subtype.map(|s| s.to_string()),
        None, None, None, None, None,
    )
}

#[tokio::test]
async fn test_get_activity() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_rich_event("io.arc.event", "sess-act", Some("message.user.prompt")),
            make_rich_event("io.arc.event", "sess-act", Some("message.assistant.tool_use")),
            make_rich_event("io.arc.event", "sess-act", Some("message.assistant.text")),
        ];
        seed_and_ingest(&mut s, "sess-act", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-act/activity")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body["first_prompt"].is_string() || body["first_prompt"].is_null());
    assert!(body["tool_breakdown"].is_object());
    assert!(body["conversation_turns"].is_number());
    assert!(body["files_touched"].is_array());
    assert!(body["error_messages"].is_array());
}

#[tokio::test]
async fn test_get_activity_empty_session() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/activity")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["conversation_turns"], 0);
    assert_eq!(body["plan_count"], 0);
}

// ── Tools endpoint ─────────────────────────────────────────────────

#[tokio::test]
async fn test_get_tools() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_rich_event("io.arc.event", "sess-tools", Some("message.assistant.tool_use")),
            make_rich_event("io.arc.event", "sess-tools", Some("message.assistant.tool_use")),
        ];
        seed_and_ingest(&mut s, "sess-tools", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-tools/tools")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_object());
}

#[tokio::test]
async fn test_get_tools_empty_session() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/tools")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_object());
}

// ── Transcript endpoint ────────────────────────────────────────────

#[tokio::test]
async fn test_get_transcript_no_transcript_path() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-no-tr")];
        seed_and_ingest(&mut s, "sess-no-tr", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-no-tr/transcript")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    // Hermes/event-sourced sessions have no transcript_path — events are the
    // transcript. The endpoint reconstructs from events instead of erroring.
    assert_eq!(body["source"], "events");
    assert_eq!(body["entries"], serde_json::json!([]));
}

// ── Plans endpoints ────────────────────────────────────────────────

#[tokio::test]
async fn test_list_plans_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/plans")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body, serde_json::json!([]));
}

#[tokio::test]
async fn test_get_plan_not_found() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/plans/nonexistent-plan")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_get_session_plans_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/sess-1/plans")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body, serde_json::json!([]));
}

// ── Subagent plan attribution ────────────────────────────────────────

#[tokio::test]
async fn test_session_plans_includes_subagent_plans() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;

        // Ingest a normal event into the parent session
        let parent_event = helpers::make_user_prompt("parent-sess", "evt-parent-1");
        seed_and_ingest(&mut s, "parent-sess", &[parent_event], None).await;

        // Ingest an ExitPlanMode event into the subagent session.
        // The event's data.session_id = "parent-sess" (the parent),
        // but we're ingesting under "agent-sub" (the subagent).
        let mut plan_payload = ClaudeCodePayload::new();
        plan_payload.tool = Some("ExitPlanMode".to_string());
        plan_payload.args = Some(serde_json::json!({ "plan": "# Subagent Plan\n\nDo the thing." }));
        let plan_data = EventData::with_payload(
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "model": "claude-4",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_sub_plan",
                        "name": "ExitPlanMode",
                        "input": { "plan": "# Subagent Plan\n\nDo the thing." }
                    }]
                }
            }),
            1,
            "parent-sess".to_string(),
            AgentPayload::ClaudeCode(plan_payload),
        );
        let plan_event = open_story::cloud_event::CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            plan_data,
            Some("message.assistant.tool_use".to_string()),
            Some("evt-sub-plan-1".to_string()),
            Some("2025-01-17T00:00:00Z".to_string()),
            None,
            None,
            None,
        );
        seed_and_ingest(&mut s, "agent-sub", &[plan_event], None).await;
    }

    // Query plans for the PARENT session — should include the subagent's plan
    let req = Request::get("/api/sessions/parent-sess/plans")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let plans = body.as_array().unwrap();
    assert!(
        !plans.is_empty(),
        "parent session plans should include subagent plans"
    );
    assert!(
        plans.iter().any(|p| p["title"].as_str() == Some("Subagent Plan")),
        "should find the subagent's plan under the parent session"
    );
}

// ── Session list completeness (Phase 2 of Plan 069) ──────────────────

#[tokio::test]
async fn test_list_sessions_includes_label_branch_and_tokens() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        // Use a user prompt event so the projection extracts a label
        let event = helpers::make_user_prompt("sess-fields", "evt-fields-1");
        seed_and_ingest(&mut s, "sess-fields", &[event], Some("my-project")).await;
    }

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);

    let session = &sessions[0];
    // Core fields
    assert_eq!(session["session_id"], "sess-fields");
    assert!(session["status"].is_string());
    assert!(session["event_count"].is_number());
    assert_eq!(session["project_id"].as_str(), Some("my-project"));
    assert!(session["project_name"].is_string());
    assert!(session["duration_ms"].is_number() || session["duration_ms"].is_null());
    assert!(session["first_prompt"].is_string() || session["first_prompt"].is_null());

    // New fields added for Explore sidebar
    assert!(
        session["label"].is_string(),
        "session list should include label from projection"
    );
    assert_eq!(session["label"].as_str().unwrap(), "test prompt");
    // branch may be null if no git_branch in events, but field must exist
    assert!(
        session.get("branch").is_some(),
        "session list should include branch field"
    );
    // Token counts should be present (may be 0)
    assert!(
        session.get("total_input_tokens").is_some(),
        "session list should include total_input_tokens"
    );
    assert!(
        session.get("total_output_tokens").is_some(),
        "session list should include total_output_tokens"
    );
}

// ── Session list response format + pagination ──────────────────────

#[tokio::test]
async fn test_list_sessions_returns_wrapped_format() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-fmt")];
        seed_and_ingest(&mut s, "sess-fmt", &events, None).await;
    }

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    // Response should be { sessions: [...], total: N }
    assert!(body["sessions"].is_array(), "response should have 'sessions' array");
    assert_eq!(body["total"].as_u64(), Some(1));
    assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(body["sessions"][0]["session_id"], "sess-fmt");
}

#[tokio::test]
async fn test_list_sessions_limit() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        for i in 0..5 {
            let sid = format!("sess-{}", i);
            let events = vec![make_event("io.arc.event", &sid)];
            seed_and_ingest(&mut s, &sid, &events, None).await;
        }
    }

    let req = Request::get("/api/sessions?limit=2")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(body["total"].as_u64(), Some(5));
}

#[tokio::test]
async fn test_list_sessions_offset() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        for i in 0..5 {
            let sid = format!("sess-{}", i);
            let events = vec![make_event("io.arc.event", &sid)];
            seed_and_ingest(&mut s, &sid, &events, None).await;
        }
    }

    let req = Request::get("/api/sessions?limit=2&offset=3")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(body["total"].as_u64(), Some(5));
}

// ── Sort modes (Latest / Most active / Most tokens) ────────────────

#[tokio::test]
async fn test_list_sessions_sort_active_orders_by_event_count_desc() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Three sessions with different event counts. Seed in an order that
    // differs from both event-count DESC and last_event DESC, so a passing
    // assertion proves the sort actually ran.
    {
        let mut s = state.write().await;
        let mid: Vec<_> = (0..3).map(|_| make_event("io.arc.event", "sess-mid")).collect();
        seed_and_ingest(&mut s, "sess-mid", &mid, None).await;
        let big: Vec<_> = (0..7).map(|_| make_event("io.arc.event", "sess-big")).collect();
        seed_and_ingest(&mut s, "sess-big", &big, None).await;
        let small: Vec<_> = (0..1).map(|_| make_event("io.arc.event", "sess-small")).collect();
        seed_and_ingest(&mut s, "sess-small", &small, None).await;
    }

    let req = Request::get("/api/sessions?sort=active")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let ids: Vec<&str> = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec!["sess-big", "sess-mid", "sess-small"],
        "sort=active must return sessions ordered by event_count DESC"
    );
}

// Default `sort=latest` (last_event DESC) is verified by the EventStore
// conformance helper `it_lists_sessions_ordered_by_last_event_desc` —
// re-asserting it here would just couple this test to fixture timestamps.

// ── Host origin filtering (Day 4) ───────────────────────────────────

#[tokio::test]
async fn test_list_sessions_includes_host_field_in_response() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let event = make_event("io.arc.event", "sess-has-host").with_host("Maxs-Air");
        seed_and_ingest(&mut s, "sess-has-host", &[event], None).await;
    }

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    let row = sessions.iter().find(|s| s["session_id"] == "sess-has-host").unwrap();
    assert_eq!(row["host"], "Maxs-Air");
}

#[tokio::test]
async fn test_list_sessions_host_is_null_for_pre_migration_events() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        // No .with_host() — simulates pre-migration data.
        let event = make_event("io.arc.event", "sess-no-host");
        seed_and_ingest(&mut s, "sess-no-host", &[event], None).await;
    }

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let row = body["sessions"].as_array().unwrap().iter()
        .find(|s| s["session_id"] == "sess-no-host").unwrap();
    assert!(row["host"].is_null(), "pre-migration rows must report host: null, got {:?}", row["host"]);
}

#[tokio::test]
async fn test_list_sessions_filters_by_host() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let e_max = make_event("io.arc.event", "sess-on-mac").with_host("Maxs-Air");
        seed_and_ingest(&mut s, "sess-on-mac", &[e_max], None).await;

        let e_bobby = make_event("io.arc.event", "sess-on-vps").with_host("debian-16gb-ash-1");
        seed_and_ingest(&mut s, "sess-on-vps", &[e_bobby], None).await;

        let e_bobby2 = make_event("io.arc.event", "sess-on-vps-2").with_host("debian-16gb-ash-1");
        seed_and_ingest(&mut s, "sess-on-vps-2", &[e_bobby2], None).await;
    }

    // ?host=debian-16gb-ash-1 narrows to Bobby's two sessions.
    let req = Request::get("/api/sessions?host=debian-16gb-ash-1")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(body["total"].as_u64(), Some(2));
    for s in sessions {
        assert_eq!(s["host"], "debian-16gb-ash-1");
    }

    // ?host=Maxs-Air narrows to the one Mac session.
    let req = Request::get("/api/sessions?host=Maxs-Air")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    let body = body_json(resp).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(body["sessions"][0]["session_id"], "sess-on-mac");

    // No filter → all three.
    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn test_list_sessions_host_filter_excludes_none_hosts() {
    // ?host=X must NOT match sessions whose host is None.
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let stamped = make_event("io.arc.event", "sess-stamped").with_host("Maxs-Air");
        seed_and_ingest(&mut s, "sess-stamped", &[stamped], None).await;

        let legacy = make_event("io.arc.event", "sess-legacy"); // no host
        seed_and_ingest(&mut s, "sess-legacy", &[legacy], None).await;
    }

    let req = Request::get("/api/sessions?host=Maxs-Air").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], "sess-stamped");
}

// ── FTS5 search endpoint tests ──────────────────────────────────────

#[tokio::test]
async fn test_search_without_q_param_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/search")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);

    let body = body_json(resp).await;
    assert_eq!(body["error"], "missing or empty 'q' parameter");
}

#[tokio::test]
async fn test_search_with_empty_q_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/search?q=")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_search_with_whitespace_only_q_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/search?q=%20%20")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_search_with_empty_store_returns_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/search?q=test")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.as_array().unwrap().is_empty());
}

// ── Agentic search endpoint tests ───────────────────────────────────

#[tokio::test]
async fn test_agent_search_without_q_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/search")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);

    let body = body_json(resp).await;
    assert_eq!(body["error"], "missing or empty 'q' parameter");
}

#[tokio::test]
async fn test_agent_search_with_empty_q_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/search?q=")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_agent_search_with_empty_store_returns_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/search?q=fix+auth+bug")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_agent_search_accepts_project_and_days_params() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/search?q=test&project=my-project&days=7&limit=3")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200, "FTS5 search should always be available");
}

#[tokio::test]
async fn test_agent_tools_includes_search() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/tools")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let tools = body.as_array().unwrap();
    let search_tool = tools.iter().find(|t| t["name"] == "search");
    assert!(
        search_tool.is_some(),
        "agent tools should include search"
    );
    let tool = search_tool.unwrap();
    assert_eq!(tool["endpoint"], "/api/agent/search");
    assert!(tool["parameters"]["required"].as_array().unwrap().contains(&serde_json::json!("q")));
}

// ── Session lifecycle endpoints ─────────────────────────────────────

fn make_error_event(session_id: &str, id: &str) -> open_story::cloud_event::CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("something failed".to_string());
    let data = EventData::with_payload(
        serde_json::json!({}),
        0,
        session_id.to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    open_story::cloud_event::CloudEvent::new(
        format!("arc://transcript/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some("system.error".to_string()),
        Some(id.to_string()),
        None, None, None, None,
    )
}

#[tokio::test]
async fn test_delete_session_unknown_returns_404() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::builder()
        .method("DELETE")
        .uri("/api/sessions/nonexistent")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_session_removes_session() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Ingest events for "sess-del"
    {
        let mut s = state.write().await;
        let events = vec![
            make_event("io.arc.event", "sess-del"),
            make_event("io.arc.event", "sess-del"),
            make_event("io.arc.event", "sess-del"),
        ];
        seed_and_ingest(&mut s, "sess-del", &events, None).await;
    }

    // DELETE it
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/sessions/sess-del")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "deleted");
    assert_eq!(body["session_id"], "sess-del");
    assert!(body["events_deleted"].as_u64().unwrap() >= 3);

    // Verify session is gone from list
    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    let ids: Vec<&str> = sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str())
        .collect();
    assert!(!ids.contains(&"sess-del"), "deleted session should not appear in list");
}

#[tokio::test]
async fn test_export_session_unknown_returns_404() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/export")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_export_session_returns_jsonl() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_event("io.arc.event", "sess-exp"),
            make_event("io.arc.event", "sess-exp"),
            make_event("io.arc.event", "sess-exp"),
        ];
        seed_and_ingest(&mut s, "sess-exp", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-exp/export")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(content_type, "application/x-ndjson");

    let body = helpers::body_text(resp).await;
    let lines: Vec<&str> = body.split('\n').filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3, "export should have 3 JSONL lines");

    // Each line should be valid JSON
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .expect("each JSONL line should be valid JSON");
        assert!(parsed.is_object());
    }
}

// ── Query endpoints ─────────────────────────────────────────────────

#[tokio::test]
async fn test_synopsis_returns_data() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_user_prompt("sess-syn", "evt-syn-1"),
            helpers::make_tool_use("sess-syn", "evt-syn-2", None, "Read", "cat foo.txt"),
            helpers::make_assistant_text("sess-syn", "evt-syn-3", None, "Here is the file content"),
        ];
        seed_and_ingest(&mut s, "sess-syn", &events, Some("my-proj")).await;
    }

    let req = Request::get("/api/sessions/sess-syn/synopsis")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_object(), "synopsis should return an object");
    assert!(
        body.get("session_id").is_some() || body.get("event_count").is_some() || body.get("tool_count").is_some(),
        "synopsis should contain session data fields"
    );
}

#[tokio::test]
async fn test_synopsis_unknown_returns_404() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/synopsis")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_tool_journey_returns_sequence() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_tool_use("sess-tj", "evt-tj-1", None, "Read", "cat foo.txt"),
            helpers::make_tool_use("sess-tj", "evt-tj-2", None, "Edit", "edit bar.rs"),
            helpers::make_tool_use("sess-tj", "evt-tj-3", None, "Bash", "cargo test"),
        ];
        seed_and_ingest(&mut s, "sess-tj", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-tj/tool-journey")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let journey = body.as_array().unwrap();
    assert!(!journey.is_empty(), "tool journey should contain entries for ingested tool events");
}

#[tokio::test]
async fn test_tool_journey_empty_for_unknown() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/tool-journey")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let journey = body.as_array().unwrap();
    assert!(journey.is_empty());
}

#[tokio::test]
async fn test_file_impact_returns_data() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_tool_use("sess-fi", "evt-fi-1", None, "Read", "/src/main.rs"),
            helpers::make_tool_use("sess-fi", "evt-fi-2", None, "Edit", "/src/lib.rs"),
        ];
        seed_and_ingest(&mut s, "sess-fi", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-fi/file-impact")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "file-impact should return an array");
}

#[tokio::test]
async fn test_file_impact_empty_for_unknown() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/file-impact")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let impact = body.as_array().unwrap();
    assert!(impact.is_empty());
}

#[tokio::test]
async fn test_errors_returns_error_events() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_error_event("sess-err", "evt-err-1"),
            make_error_event("sess-err", "evt-err-2"),
            helpers::make_user_prompt("sess-err", "evt-err-3"),
        ];
        seed_and_ingest(&mut s, "sess-err", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-err/errors")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "errors endpoint should return an array");
}

#[tokio::test]
async fn test_errors_empty_when_none() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/errors")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let errors = body.as_array().unwrap();
    assert!(errors.is_empty());
}

// ── Insights endpoints ──────────────────────────────────────────────

#[tokio::test]
async fn test_pulse_returns_aggregation() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_user_prompt("sess-pulse", "evt-pulse-1"),
            helpers::make_tool_use("sess-pulse", "evt-pulse-2", None, "Read", "cat foo.txt"),
        ];
        seed_and_ingest(&mut s, "sess-pulse", &events, Some("my-proj")).await;
    }

    let req = Request::get("/api/insights/pulse")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "pulse should return an array");
}

#[tokio::test]
async fn test_tool_evolution_returns_data() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/insights/tool-evolution")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "tool-evolution should return an array");
}

#[tokio::test]
async fn test_efficiency_returns_metrics() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/insights/efficiency")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "efficiency should return an array");
}

#[tokio::test]
async fn test_productivity_returns_hourly_buckets() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/insights/productivity")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array(), "productivity should return an array");
}

// ── Agent context endpoints ─────────────────────────────────────────

#[tokio::test]
async fn test_project_context_returns_sessions() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_user_prompt("sess-ctx", "evt-ctx-1"),
            helpers::make_tool_use("sess-ctx", "evt-ctx-2", None, "Read", "cat main.rs"),
        ];
        seed_and_ingest(&mut s, "sess-ctx", &events, Some("my-proj")).await;
    }

    let req = Request::get("/api/agent/project-context?project=my-proj")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array() || body.is_object(), "project-context should return data");
}

#[tokio::test]
async fn test_project_context_requires_project_param() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/project-context")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    // Axum returns 422 for failed Query deserialization (missing required field)
    let status = resp.status().as_u16();
    assert!(
        status == 400 || status == 422,
        "missing project param should return 400 or 422, got {status}"
    );
}

#[tokio::test]
async fn test_recent_files_returns_modified_files() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_tool_use("sess-rf", "evt-rf-1", None, "Edit", "/src/main.rs"),
            helpers::make_tool_use("sess-rf", "evt-rf-2", None, "Write", "/src/lib.rs"),
        ];
        seed_and_ingest(&mut s, "sess-rf", &events, Some("my-proj")).await;
    }

    let req = Request::get("/api/agent/recent-files?project=my-proj")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body.is_array() || body.is_object(), "recent-files should return data");
}

#[tokio::test]
async fn test_recent_files_requires_project_param() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/agent/recent-files")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    let status = resp.status().as_u16();
    assert!(
        status == 400 || status == 422,
        "missing project param should return 400 or 422, got {status}"
    );
}

// ── Patterns + meta endpoints ───────────────────────────────────────

#[tokio::test]
async fn test_patterns_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/sess-pat/patterns")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["patterns"], serde_json::json!([]));
}

#[tokio::test]
async fn test_meta_returns_projection_data() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            helpers::make_user_prompt("sess-meta", "evt-meta-1"),
            helpers::make_tool_use("sess-meta", "evt-meta-2", None, "Read", "cat foo.txt"),
            helpers::make_assistant_text("sess-meta", "evt-meta-3", None, "done"),
        ];
        seed_and_ingest(&mut s, "sess-meta", &events, None).await;
    }

    let req = Request::get("/api/sessions/sess-meta/meta")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert!(body["event_count"].is_number(), "meta should include event_count");
    assert!(body["event_count"].as_u64().unwrap() >= 3);
    assert!(body["filter_counts"].is_object(), "meta should include filter_counts");
}

#[tokio::test]
async fn test_meta_unknown_session_returns_404() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/meta")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 404);
}
