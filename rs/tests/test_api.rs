//! Integration tests for GET /api/* endpoints.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, make_event, send_request, test_state};
use tempfile::TempDir;

use helpers::seed_and_ingest;
use open_story::event_data::{AgentPayload, ClaudeCodePayload, EventData};

#[tokio::test]
async fn test_list_sessions_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["total"].as_u64(), Some(0));
    assert_eq!(body["sessions"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_health_reports_node_status() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    // Ingest one session so the store and the projection are non-empty.
    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-health")];
        seed_and_ingest(&mut s, "sess-health", &events, None).await;
    }

    let req = Request::get("/api/health").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
    // store
    assert!(body["store"]["backend"].is_string(), "backend should be reported");
    assert_eq!(body["store"]["sessions"].as_u64(), Some(1));
    // bus — tests use NoopBus, so not connected
    assert_eq!(body["bus"]["connected"], false);
    // projection freshness — ingest populated the projection, so count covers
    // sessions and fresh is true. This is the field that goes false when a
    // restart leaves projections un-rehydrated (the token-0 divergence).
    assert_eq!(body["projections"]["sessions"].as_u64(), Some(1));
    assert!(body["projections"]["count"].as_u64().unwrap() >= 1);
    assert_eq!(body["projections"]["fresh"], true);
}

#[tokio::test]
async fn test_digests_endpoint_reports_per_session_convergence() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events_a = vec![
            make_event("io.arc.event", "sess-a"),
            make_event("io.arc.event", "sess-a"),
        ];
        seed_and_ingest(&mut s, "sess-a", &events_a, None).await;
        let events_b = vec![make_event("io.arc.event", "sess-b")];
        seed_and_ingest(&mut s, "sess-b", &events_b, None).await;
    }

    let req = Request::get("/api/digests").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 2, "one digest per session");
    for d in sessions {
        assert!(d["session_id"].is_string());
        assert!(d["count"].as_u64().unwrap() >= 1);
        // FNV-1a 64-bit rendered as 16 hex chars — the cross-node comparison key.
        let digest = d["digest"].as_str().expect("digest string");
        assert_eq!(digest.len(), 16, "digest is 16 hex chars, got {digest:?}");
    }
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

        let events_b = vec![make_event("io.arc.event", "session-b")];
        seed_and_ingest(&mut s, "session-b", &events_b, None).await;
    }

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();

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
async fn test_list_sessions_includes_plan_count() {
    // The sidebar plan badge reads plan_count from /api/sessions. It must
    // reflect the durable plan store, not whatever ExitPlanMode events happen
    // to be loaded — so a session with stored plans but no plan events still
    // reports its count.
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-plans")];
        seed_and_ingest(&mut s, "sess-plans", &events, None).await;
        // Two distinct plans (different timestamps) for this session.
        s.store.plan_store.save("sess-plans", "# Plan: First", "2026-01-01T00:00:00Z").unwrap();
        s.store.plan_store.save("sess-plans", "# Plan: Second", "2026-01-01T00:05:00Z").unwrap();
        // A plan for a different session must not bleed into this count.
        let other = vec![make_event("io.arc.event", "sess-other")];
        seed_and_ingest(&mut s, "sess-other", &other, None).await;
        s.store.plan_store.save("sess-other", "# Plan: Other", "2026-01-01T00:00:00Z").unwrap();
    }

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    let with_plans = sessions.iter().find(|s| s["session_id"] == "sess-plans").unwrap();
    assert_eq!(with_plans["plan_count"].as_u64(), Some(2));
    let other = sessions.iter().find(|s| s["session_id"] == "sess-other").unwrap();
    assert_eq!(other["plan_count"].as_u64(), Some(1));
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
}

// ── Invariant ① — catch-up respects share policy ─────────────────────────
//
// `share_policy[id] = private` means the session must not leak through
// the app-level catch-up path. Two endpoints carry the contract:
//   /api/digests           → omit private sessions entirely
//   /api/sessions/{id}/events → 404 (deny existence)
//
// Together these mean a peer running catch-up against this node sees the
// private session as "not on this device" — same shape as never having
// observed it, exactly what the federation transport's `filter_subject`
// would also produce.

#[tokio::test]
async fn test_digests_omit_private_sessions_invariant_one() {
    use open_story_store::event_store::SharePolicyMode;

    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events_a = vec![make_event("io.arc.event", "sess-shared")];
        seed_and_ingest(&mut s, "sess-shared", &events_a, None).await;
        let events_b = vec![make_event("io.arc.event", "sess-private")];
        seed_and_ingest(&mut s, "sess-private", &events_b, None).await;

        // Mark one private — the other stays at default (shared).
        s.store
            .event_store
            .set_share_policy("sess-private", SharePolicyMode::Private, None)
            .await
            .expect("set share policy");
    }

    let req = Request::get("/api/digests").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let ids: Vec<&str> = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["session_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"sess-shared"), "shared session must appear");
    assert!(
        !ids.contains(&"sess-private"),
        "private session must be omitted (invariant ①)"
    );
}

#[tokio::test]
async fn test_get_events_404s_on_private_session_invariant_one() {
    use open_story_store::event_store::SharePolicyMode;

    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-secret")];
        seed_and_ingest(&mut s, "sess-secret", &events, None).await;
        s.store
            .event_store
            .set_share_policy("sess-secret", SharePolicyMode::Private, None)
            .await
            .expect("set share policy");
    }

    let req = Request::get("/api/sessions/sess-secret/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(
        resp.status(),
        404,
        "private session must 404 — denying existence so peer catch-up stops asking"
    );
}

// ── Default posture — sharing is opt-IN, not opt-out ────────────────────
//
// Sovereignty: a session the operator has never touched must be PRIVATE.
// Nothing federates or becomes API-readable until the user explicitly
// shares it. (Was the inverse — default Shared — which meant any new
// session leaked outward the moment a hub existed.)
#[tokio::test]
async fn test_unconfigured_session_is_private_by_default_opt_in_share() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        // Insert directly (NOT seed_and_ingest, which marks sessions shared
        // as a test convenience) so the session has no share_policy row at
        // all — exercising the genuine default.
        let event = make_event("io.arc.event", "sess-fresh");
        let val = serde_json::to_value(&event).unwrap();
        s.store
            .event_store
            .insert_event("sess-fresh", &val)
            .await
            .expect("insert event");
    }

    // Per-session read denies existence (private → 404).
    let req = Request::get("/api/sessions/sess-fresh/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(std::sync::Arc::clone(&state), req).await;
    assert_eq!(
        resp.status(),
        404,
        "an unconfigured session must be private by default (opt-in sharing)"
    );

    // And it is omitted from the digest list.
    let req = Request::get("/api/digests").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let ids: Vec<&str> = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["session_id"].as_str())
        .collect();
    assert!(
        !ids.contains(&"sess-fresh"),
        "unconfigured (private) session must not appear in digests, got {ids:?}"
    );
}

// ── Invariant ① — full-text search must not leak private session content ──
//
// The per-session read endpoints carry the `RequirePublicSession` gate, but
// `/api/search` and `/api/agent/search` reach the FTS index directly. Without
// a filter they return event snippets + session_id for sessions the operator
// marked private — a back door around the gate. Both share-modes are set
// explicitly so the test is robust to the default-private posture.
#[tokio::test]
async fn test_search_omits_private_session_content_invariant_one() {
    use open_story_store::event_store::SharePolicyMode;

    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let shared = vec![make_event("io.arc.event", "sess-search-shared")];
        seed_and_ingest(&mut s, "sess-search-shared", &shared, None).await;
        let private = vec![make_event("io.arc.event", "sess-search-private")];
        seed_and_ingest(&mut s, "sess-search-private", &private, None).await;

        s.store
            .event_store
            .set_share_policy("sess-search-shared", SharePolicyMode::Shared, None)
            .await
            .expect("set shared");
        s.store
            .event_store
            .set_share_policy("sess-search-private", SharePolicyMode::Private, None)
            .await
            .expect("set private");

        // seed_and_ingest persists but does not touch FTS (that happens in
        // the broadcast consumer on the live bus path); index explicitly.
        s.store
            .event_store
            .index_fts(&shared[0].id, "sess-search-shared", "message.assistant.text", "test content")
            .await
            .expect("index shared");
        s.store
            .event_store
            .index_fts(&private[0].id, "sess-search-private", "message.assistant.text", "test content")
            .await
            .expect("index private");
    }

    // Both sessions index the text "test content"; search a common term.
    let req = Request::get("/api/search?q=content")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(std::sync::Arc::clone(&state), req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let sessions: Vec<&str> = body
        .as_array()
        .expect("/api/search returns an array")
        .iter()
        .filter_map(|r| r["session_id"].as_str())
        .collect();
    assert!(
        sessions.contains(&"sess-search-shared"),
        "shared session content must remain searchable, got {sessions:?}"
    );
    assert!(
        !sessions.contains(&"sess-search-private"),
        "private session content must NOT leak via /api/search, got {sessions:?}"
    );

    // Same guarantee for the session-grouped agentic search endpoint.
    let req = Request::get("/api/agent/search?q=content")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let blob = body.to_string();
    assert!(
        !blob.contains("sess-search-private"),
        "private session must NOT appear in /api/agent/search results, got {blob}"
    );
}

// ── Invariant ③ — revocation is stop-flow, not purge ────────────────────
//
// Naming the hard edge the research doc surfaces: marking a session
// private STOPS new visibility (the API filter from invariant ① kicks in
// immediately), but does NOT delete the events on disk. Local data
// remains intact — flipping back to `shared` restores API access without
// rebuilding anything. Peer-mirrored copies on OTHER devices are the
// unresolved part of revocation (the personhood Q1/Q9 problem); this
// test pins the *local* half of the contract.
#[tokio::test]
async fn test_revocation_is_stop_flow_not_purge_invariant_three() {
    use open_story_store::event_store::SharePolicyMode;

    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_event("io.arc.event", "sess-toggle"),
            make_event("io.arc.event", "sess-toggle"),
            make_event("io.arc.event", "sess-toggle"),
        ];
        seed_and_ingest(&mut s, "sess-toggle", &events, None).await;
    }

    // 1. While shared, three events are visible.
    let req = Request::get("/api/sessions/sess-toggle/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 3);

    // 2. Mark private — events on disk are NOT deleted, just gated.
    {
        let s = state.write().await;
        s.store
            .event_store
            .set_share_policy("sess-toggle", SharePolicyMode::Private, None)
            .await
            .expect("set private");
        // Sanity: store still has the events (count via direct read).
        let evs = s
            .store
            .event_store
            .session_events("sess-toggle")
            .await
            .unwrap();
        assert_eq!(evs.len(), 3, "events are NOT purged from storage");
    }
    let req = Request::get("/api/sessions/sess-toggle/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 404, "API gates while private");

    // 3. Flip back to shared — API access is restored from the same rows.
    {
        let s = state.write().await;
        s.store
            .event_store
            .set_share_policy("sess-toggle", SharePolicyMode::Shared, None)
            .await
            .expect("set shared");
    }
    let req = Request::get("/api/sessions/sess-toggle/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        body_json(resp).await.as_array().unwrap().len(),
        3,
        "events come back unchanged — stop-flow toggled, not destroyed"
    );
}

#[tokio::test]
async fn test_get_events_still_returns_shared_after_policy_table_exists() {
    // Defense-in-depth: confirm the new private-filter branch doesn't
    // accidentally suppress shared sessions when the policy table is
    // populated for OTHER sessions.
    use open_story_store::event_store::SharePolicyMode;

    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-keep")];
        seed_and_ingest(&mut s, "sess-keep", &events, None).await;
        // Some other session is private — shouldn't affect this one.
        s.store
            .event_store
            .set_share_policy("sess-other", SharePolicyMode::Private, None)
            .await
            .expect("set share policy");
    }

    let req = Request::get("/api/sessions/sess-keep/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 1);
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

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();

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
    assert!(
        cors.is_some(),
        "CORS header should be present for localhost origin"
    );
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

fn make_rich_event(
    event_type: &str,
    session_id: &str,
    subtype: Option<&str>,
) -> open_story::cloud_event::CloudEvent {
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
        None,
        None,
        None,
        None,
        None,
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
            make_rich_event(
                "io.arc.event",
                "sess-act",
                Some("message.assistant.tool_use"),
            ),
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
}

// ── Tools endpoint ─────────────────────────────────────────────────

#[tokio::test]
async fn test_get_tools() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_rich_event(
                "io.arc.event",
                "sess-tools",
                Some("message.assistant.tool_use"),
            ),
            make_rich_event(
                "io.arc.event",
                "sess-tools",
                Some("message.assistant.tool_use"),
            ),
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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

    let req = Request::get("/api/plans").body(Body::empty()).unwrap();

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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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
        plans
            .iter()
            .any(|p| p["title"].as_str() == Some("Subagent Plan")),
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

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();

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

    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    // Response should be { sessions: [...], total: N }
    assert!(
        body["sessions"].is_array(),
        "response should have 'sessions' array"
    );
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
        let mid: Vec<_> = (0..3)
            .map(|_| make_event("io.arc.event", "sess-mid"))
            .collect();
        seed_and_ingest(&mut s, "sess-mid", &mid, None).await;
        let big: Vec<_> = (0..7)
            .map(|_| make_event("io.arc.event", "sess-big"))
            .collect();
        seed_and_ingest(&mut s, "sess-big", &big, None).await;
        let small: Vec<_> = (0..1)
            .map(|_| make_event("io.arc.event", "sess-small"))
            .collect();
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
    let row = sessions
        .iter()
        .find(|s| s["session_id"] == "sess-has-host")
        .unwrap();
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
    let row = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == "sess-no-host")
        .unwrap();
    assert!(
        row["host"].is_null(),
        "pre-migration rows must report host: null, got {:?}",
        row["host"]
    );
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

    let req = Request::get("/api/sessions?host=Maxs-Air")
        .body(Body::empty())
        .unwrap();
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

    let req = Request::get("/api/search").body(Body::empty()).unwrap();

    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 400);

    let body = body_json(resp).await;
    assert_eq!(body["error"], "missing or empty 'q' parameter");
}

#[tokio::test]
async fn test_search_with_empty_q_returns_400() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/search?q=").body(Body::empty()).unwrap();

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
    assert!(search_tool.is_some(), "agent tools should include search");
    let tool = search_tool.unwrap();
    assert_eq!(tool["endpoint"], "/api/agent/search");
    assert!(tool["parameters"]["required"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("q")));
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
        None,
        None,
        None,
        None,
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
    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    let ids: Vec<&str> = sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str())
        .collect();
    assert!(
        !ids.contains(&"sess-del"),
        "deleted session should not appear in list"
    );
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
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each JSONL line should be valid JSON");
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
        body.get("session_id").is_some()
            || body.get("event_count").is_some()
            || body.get("tool_count").is_some(),
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
    assert!(
        !journey.is_empty(),
        "tool journey should contain entries for ingested tool events"
    );
}

#[tokio::test]
async fn test_tool_journey_empty_for_unknown() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/sessions/nonexistent/tool-journey")
        .body(Body::empty())
        .unwrap();

    let resp = send_request(state, req).await;
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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
    assert!(
        body.is_array() || body.is_object(),
        "project-context should return data"
    );
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
    assert!(
        body.is_array() || body.is_object(),
        "recent-files should return data"
    );
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
    // Opt-in sharing: an unknown/unshared session denies existence (404),
    // indistinguishable from a private one — no existence oracle.
    assert_eq!(resp.status(), 404);
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
    assert!(
        body["event_count"].is_number(),
        "meta should include event_count"
    );
    assert!(body["event_count"].as_u64().unwrap() >= 3);
    assert!(
        body["filter_counts"].is_object(),
        "meta should include filter_counts"
    );
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

// ── /api/local-info — what OPEN_STORY_HOST/USER resolved to ────────────

#[tokio::test]
async fn test_local_info_returns_host_and_user_strings() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    let req = Request::get("/api/local-info").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    // Both fields are always strings — the resolver falls back to
    // "unknown" rather than returning null.
    assert!(body["host"].is_string(), "host must be a string");
    assert!(body["user"].is_string(), "user must be a string");
    assert!(!body["host"].as_str().unwrap().is_empty());
    assert!(!body["user"].as_str().unwrap().is_empty());
}

// ── /api/users — per-user activity surface (Users tab v0.1) ────────────

#[tokio::test]
async fn test_list_users_empty() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    assert_eq!(body["total"].as_u64(), Some(0));
    assert_eq!(body["users"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_list_users_aggregates_by_user_field() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        // 2 sessions for katie, 1 for max, 1 with no user (legacy).
        let katie_a = vec![
            make_event("io.arc.event", "sess-katie-a")
                .with_host("Katies-Mac-mini")
                .with_user("katie"),
            make_event("io.arc.event", "sess-katie-a")
                .with_host("Katies-Mac-mini")
                .with_user("katie"),
        ];
        seed_and_ingest(&mut s, "sess-katie-a", &katie_a, Some("proj-A")).await;

        let katie_b = vec![make_event("io.arc.event", "sess-katie-b")
            .with_host("Katies-MacBook-Pro")
            .with_user("katie")];
        seed_and_ingest(&mut s, "sess-katie-b", &katie_b, Some("proj-B")).await;

        let max = vec![make_event("io.arc.event", "sess-max")
            .with_host("Maxs-MacBook-Pro")
            .with_user("max")];
        seed_and_ingest(&mut s, "sess-max", &max, Some("proj-A")).await;

        // Legacy session with no user stamp — must NOT appear in /api/users.
        let legacy = vec![make_event("io.arc.event", "sess-legacy")];
        seed_and_ingest(&mut s, "sess-legacy", &legacy, Some("proj-legacy")).await;
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(resp.status(), 200);

    let body = body_json(resp).await;
    let users = body["users"].as_array().unwrap();
    // Only katie + max — sess-legacy's null user is excluded.
    assert_eq!(users.len(), 2);

    let katie = users
        .iter()
        .find(|u| u["user"] == "katie")
        .expect("katie should appear");
    assert_eq!(katie["session_count"].as_u64(), Some(2));
    let katie_hosts: Vec<&str> = katie["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(katie_hosts.contains(&"Katies-Mac-mini"));
    assert!(katie_hosts.contains(&"Katies-MacBook-Pro"));

    let max = users
        .iter()
        .find(|u| u["user"] == "max")
        .expect("max should appear");
    assert_eq!(max["session_count"].as_u64(), Some(1));

    // total = all SessionRow rows (including legacy), so the UI can
    // render "stamped 3 / 4 sessions" if it wants.
    assert_eq!(body["total"].as_u64(), Some(4));
}

/// Build an event whose `time` is `minutes_ago` minutes before `now()`.
/// Used to anchor activity_24h math tests against a known clock offset
/// without injecting a clock through every layer.
fn event_at(session_id: &str, minutes_ago: i64) -> open_story::cloud_event::CloudEvent {
    let mut ev = make_event("io.arc.event", session_id)
        .with_host("Katies-Mac-mini")
        .with_user("katie");
    let t = chrono::Utc::now() - chrono::Duration::minutes(minutes_ago);
    ev.time = t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    ev
}

#[tokio::test]
async fn test_list_users_activity_24h_session_in_window_distributes_to_correct_buckets() {
    // Session of two events: one 90 min ago (first), one 30 min ago (last).
    // event_count = 2; span = 60 min. Should land entirely in the two
    // most-recent hour buckets ([22], [23]). Older buckets should be 0.
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    {
        let mut s = state.write().await;
        let events = vec![event_at("sess-recent", 90), event_at("sess-recent", 30)];
        seed_and_ingest(&mut s, "sess-recent", &events, Some("proj-A")).await;
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let katie = &body["users"][0];
    let buckets: Vec<u64> = katie["activity_24h"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();

    // Sum across the 24h window equals event_count when the entire
    // session span falls inside the window. Allow ±1 for rounding —
    // proportional distribution + per-hour rounding can shed at most 1.
    let total: u64 = buckets.iter().sum();
    assert!(
        (1..=2).contains(&total),
        "events should sum to ~event_count (2), got {} (buckets: {:?})",
        total,
        buckets,
    );
    // Older buckets ([0..21]) should be empty.
    for (i, &b) in buckets.iter().enumerate().take(21) {
        assert_eq!(
            b, 0,
            "bucket[{}] should be 0 for events all in last 90 min",
            i
        );
    }
    // At least one of the two most-recent hour buckets must have data.
    assert!(
        buckets[22] + buckets[23] >= 1,
        "the two most-recent buckets together should hold the session's events; got {:?}",
        &buckets[22..],
    );
}

#[tokio::test]
async fn test_list_users_activity_24h_session_before_window_contributes_nothing() {
    // Session ended 25 hours ago — entirely outside the 24h window.
    // Every bucket must be 0 for this user.
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    {
        let mut s = state.write().await;
        let events = vec![
            event_at("sess-old", 25 * 60 + 30), // first: 25.5h ago
            event_at("sess-old", 25 * 60),      // last:  25h ago
        ];
        seed_and_ingest(&mut s, "sess-old", &events, Some("proj-A")).await;
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let katie = &body["users"][0];
    let buckets: Vec<u64> = katie["activity_24h"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();

    let total: u64 = buckets.iter().sum();
    assert_eq!(
        total, 0,
        "session entirely before the 24h window must contribute 0 events to activity_24h, got {:?}",
        buckets,
    );
}

#[tokio::test]
async fn test_list_users_activity_24h_session_spanning_window_edge_clips_correctly() {
    // Session that started 25h ago and ended 20h ago — half in, half out.
    // Only the in-window portion (the last ~3.something hours of the
    // session) should contribute. Asserts the clip + proportional logic
    // is symmetric: events outside the window are not double-counted into
    // the in-window slice.
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    {
        let mut s = state.write().await;
        let events = vec![
            event_at("sess-spans", 25 * 60), // first: 25h ago
            event_at("sess-spans", 20 * 60), // last:  20h ago
        ];
        seed_and_ingest(&mut s, "sess-spans", &events, Some("proj-A")).await;
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let katie = &body["users"][0];
    let buckets: Vec<u64> = katie["activity_24h"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();

    // Strict: the most-recent buckets ([15..]) must be empty (session
    // ended 20h ago).
    for (i, &b) in buckets.iter().enumerate().skip(15) {
        assert_eq!(
            b, 0,
            "bucket[{}] should be 0 for a session that ended 20h ago",
            i,
        );
    }

    // The in-window portion is roughly hours 0..4 (24h ago to 20h ago);
    // total contribution ≤ event_count.
    let total: u64 = buckets.iter().sum();
    assert!(
        total <= 2,
        "in-window contribution capped at event_count, got {}",
        total
    );
}

#[tokio::test]
async fn test_list_users_activity_24h_shape() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);
    {
        let mut s = state.write().await;
        let events = vec![make_event("io.arc.event", "sess-katie")
            .with_host("Katies-Mac-mini")
            .with_user("katie")];
        seed_and_ingest(&mut s, "sess-katie", &events, Some("proj-A")).await;
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;
    let katie = &body["users"][0];

    let buckets = katie["activity_24h"].as_array().unwrap();
    assert_eq!(
        buckets.len(),
        24,
        "activity_24h should have exactly 24 hourly buckets"
    );
    for b in buckets {
        assert!(b.is_number(), "each bucket should be a number, got {:?}", b);
        assert!(b.as_u64().is_some(), "bucket should fit in u64");
    }
}

#[tokio::test]
async fn test_list_users_recent_sessions_capped_and_sorted() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        for i in 0..7 {
            let sid = format!("sess-katie-{i}");
            let events = vec![make_event("io.arc.event", &sid)
                .with_host("Katies-Mac-mini")
                .with_user("katie")];
            seed_and_ingest(&mut s, &sid, &events, Some("proj-A")).await;
        }
    }

    let req = Request::get("/api/users").body(Body::empty()).unwrap();
    let resp = send_request(state, req).await;
    let body = body_json(resp).await;

    let katie = &body["users"][0];
    assert_eq!(katie["user"], "katie");
    assert_eq!(katie["session_count"].as_u64(), Some(7));

    let recent = katie["recent_sessions"].as_array().unwrap();
    assert_eq!(
        recent.len(),
        5,
        "recent_sessions capped at 5 even when session_count is higher"
    );

    // last_event DESC: each entry should be >= the next.
    let timestamps: Vec<&str> = recent
        .iter()
        .filter_map(|s| s["last_event"].as_str())
        .collect();
    for window in timestamps.windows(2) {
        assert!(
            window[0] >= window[1],
            "recent_sessions should be sorted by last_event DESC"
        );
    }
}
