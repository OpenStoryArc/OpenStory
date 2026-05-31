//! Phase 4.1 RED — share-policy enforcement across ALL per-session endpoints.
//!
//! PR #58 shipped Invariant ① on only two endpoints (`/api/digests` and
//! `/api/sessions/{id}/events`). The other per-session reads continue to
//! serve a "private" session's full content. This test pins the contract
//! the UI promises: marking a session `private` makes it 404 on every
//! per-session read.
//!
//! This is a RED test on purpose — it should fail on all endpoints
//! except `/events` until Phase 4.2 ships the `RequirePublicSession`
//! extractor and threads it through the 18 handlers.

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{make_event, seed_and_ingest, send_request, test_state};
use open_story_store::event_store::SharePolicyMode;
use tempfile::TempDir;

/// All per-session GET endpoints from `rs/server/src/router.rs`. Each
/// must return 404 when the session is marked `private` (Invariant ①
/// applied uniformly). The path placeholder `{event_id}` is substituted
/// with a dummy id for the `/events/{event_id}/content` endpoint — the
/// `private` gate must fire BEFORE the event-id lookup, so the dummy id
/// is fine.
const PRIVATE_SESSION_ENDPOINTS: &[&str] = &[
    "/api/sessions/sess-secret/events",
    "/api/sessions/sess-secret/summary",
    "/api/sessions/sess-secret/activity",
    "/api/sessions/sess-secret/tools",
    "/api/sessions/sess-secret/transcript",
    "/api/sessions/sess-secret/view-records",
    "/api/sessions/sess-secret/conversation",
    "/api/sessions/sess-secret/file-changes",
    "/api/sessions/sess-secret/patterns",
    "/api/sessions/sess-secret/turns",
    "/api/sessions/sess-secret/plans",
    "/api/sessions/sess-secret/meta",
    "/api/sessions/sess-secret/events/evt-dummy/content",
    "/api/sessions/sess-secret/records",
    "/api/sessions/sess-secret/export",
    "/api/sessions/sess-secret/synopsis",
    "/api/sessions/sess-secret/tool-journey",
    "/api/sessions/sess-secret/file-impact",
    "/api/sessions/sess-secret/errors",
];

/// RED: marking a session private must 404 every per-session read.
///
/// Today this test fails on 18 of 19 endpoints — only `/events` was
/// gated by Invariant ① in PR #58. Phase 4.2's `RequirePublicSession`
/// axum `FromRequestParts` extractor will close the gap in one place
/// (the handler signatures pick it up; the body never runs for a private
/// session); Phase 4.3/4.4 hardens the policy read against store errors
/// (fail closed).
#[tokio::test]
async fn private_session_returns_404_on_all_per_session_endpoints() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let events = vec![
            make_event("io.arc.event", "sess-secret"),
            make_event("io.arc.event", "sess-secret"),
        ];
        seed_and_ingest(&mut s, "sess-secret", &events, None).await;
        s.store
            .event_store
            .set_share_policy("sess-secret", SharePolicyMode::Private, None)
            .await
            .expect("set share policy");
    }

    let mut failing: Vec<(&str, u16)> = Vec::new();
    for &path in PRIVATE_SESSION_ENDPOINTS {
        let req = Request::get(path).body(Body::empty()).unwrap();
        let resp = send_request(state.clone(), req).await;
        let status = resp.status().as_u16();
        if status != 404 {
            failing.push((path, status));
        }
    }
    assert!(
        failing.is_empty(),
        "private session must 404 on every per-session read; \
         the UI promises sovereignty, the API must keep it. \
         endpoints that did NOT 404: {failing:?}"
    );
}

/// Defense-in-depth: marking a different session private must NOT affect
/// reads on a shared session sharing the same store. Catches a future
/// regression where a global gate accidentally hides everything.
#[tokio::test]
async fn shared_session_still_returns_data_with_other_private_in_store() {
    let data_dir = TempDir::new().unwrap();
    let state = test_state(&data_dir);

    {
        let mut s = state.write().await;
        let public_events = vec![make_event("io.arc.event", "sess-public")];
        seed_and_ingest(&mut s, "sess-public", &public_events, None).await;
        s.store
            .event_store
            .set_share_policy("sess-other", SharePolicyMode::Private, None)
            .await
            .expect("set share policy");
    }

    let req = Request::get("/api/sessions/sess-public/summary")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state, req).await;
    assert_eq!(
        resp.status(),
        200,
        "sess-public must still serve normally when an unrelated \
         session is private"
    );
}
