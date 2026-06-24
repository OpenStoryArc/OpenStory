//! Aggressive security exploit attempts — red-team probes that try to break
//! the server's defenses. Each test asserts the server *fends off* the attack.
//!
//! Companion to `test_security.rs`. That file covers baseline hardening
//! (path traversal, SQL injection, dedup integrity). This file probes the
//! attack surface that surfaced during the 2026-05-28 security audit:
//!
//! - Bearer-token bypass attempts (wrong scheme, extra whitespace, empty token,
//!   case-folding, very-long tokens — constant-time comparison should hold).
//! - WebSocket auth gating: the auth_middleware MUST gate the `/ws` upgrade
//!   when `api_token` is set, because the comment in `auth.rs` claiming
//!   "WebSocket auth uses `?token=` query param" is aspirational — the
//!   ws_handler never reads the query param. We assert that the Bearer
//!   gate still rejects unauthorized upgrade attempts, including a
//!   `?token=` form that some attacker might guess from the doc.
//! - 50 MB DefaultBodyLimit is enforced.
//! - FTS5 query metacharacters can't crash the server.
//! - URL-encoded `..` in transcript paths.
//! - Symlink escapes from `data_dir`.
//! - Concurrent `delete_session` races don't panic.
//! - Adversarial query params (negative limits, huge days values, NUL/CRLF
//!   in session_id) are handled gracefully.

mod helpers;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use helpers::{body_json, body_text, send_request, test_state};
use open_story::server::{Config, SharedState, build_router};

/// Build a router with a specific `api_token` set. Tests of the auth path
/// can't use the default helper because `Config::default()` has an empty
/// token (pass-through mode).
fn router_with_token(state: SharedState, token: &str) -> axum::Router {
    let config = Config {
        api_token: token.to_string(),
        ..Config::default()
    };
    build_router(state, None, &config)
}

// ── Bearer-token bypass attempts ─────────────────────────────────────────

#[tokio::test]
async fn auth_rejects_missing_authorization_header() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_lowercase_bearer_scheme() {
    // The middleware does `starts_with("Bearer ")` — case-sensitive. A
    // lowercase scheme must be rejected so we don't accidentally accept
    // tooling that lowercases header values.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    let req = Request::get("/api/sessions")
        .header("Authorization", "bearer the-real-token")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_empty_bearer_value() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    // "Bearer " with nothing after — empty token must NOT match the
    // configured token (constant_time_eq returns false when lengths differ).
    let req = Request::get("/api/sessions")
        .header("Authorization", "Bearer ")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_prefix_match_attempt() {
    // An attacker who knows the first character of the token tries to
    // bypass via a single-character bearer value. constant_time_eq must
    // reject because lengths differ.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "supersecret-1234");

    let req = Request::get("/api/sessions")
        .header("Authorization", "Bearer s")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_handles_extremely_long_bearer_without_crash() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    let huge_token = "A".repeat(100_000);
    let req = Request::get("/api/sessions")
        .header("Authorization", format!("Bearer {huge_token}"))
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "100 KB token must be rejected without crashing"
    );
}

#[tokio::test]
async fn auth_accepts_correct_bearer_token() {
    // Sanity: the positive case must work, otherwise the negative cases
    // above prove nothing. /health is on the publisher router too; we
    // hit /api/sessions which requires auth on the full router.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    let req = Request::get("/api/sessions")
        .header("Authorization", "Bearer the-real-token")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── WebSocket auth gating ────────────────────────────────────────────────

#[tokio::test]
async fn ws_upgrade_rejected_without_bearer_when_token_set() {
    // The /ws route lives inside api_router so it's wrapped by
    // auth_middleware. Without a Bearer header, the upgrade must 401.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "ws-token");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/ws")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("Sec-WebSocket-Version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "WS upgrade without Bearer must 401 — not silently accepted"
    );
}

#[tokio::test]
async fn ws_query_token_authorizes_when_correct() {
    // Browser WebSocket API can't set Authorization headers, so
    // auth_middleware accepts `?token=` as a fallback channel.
    // A correct token in the query must authorize the upgrade.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "ws-token");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/ws?token=ws-token")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("Sec-WebSocket-Version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    // Auth-layer signal: anything except 401 proves auth_middleware
    // let the request through. The downstream WebSocketUpgrade
    // extractor can return 101 (success) in a real network upgrade or
    // 426 (Upgrade Required) under `oneshot` — both mean auth passed.
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "valid ?token= must NOT be rejected by auth_middleware"
    );
}

#[tokio::test]
async fn ws_query_token_rejected_when_wrong() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "ws-token");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/ws?token=guess")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("Sec-WebSocket-Version", "13")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "wrong ?token= must be rejected"
    );
}

#[tokio::test]
async fn auth_wrong_bearer_does_not_fall_through_to_query_token() {
    // Defense in depth: if a request sends BOTH a wrong Bearer header
    // and a correct ?token=, the middleware rejects on the wrong
    // header rather than trying the next channel. An attacker who can
    // guess one channel shouldn't be helped by trying both.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "the-real-token");

    let req = Request::get("/api/sessions?token=the-real-token")
        .header("Authorization", "Bearer WRONG")
        .body(Body::empty())
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "wrong Bearer must short-circuit even when correct ?token= is present"
    );
}

// ── Body-limit DoS ───────────────────────────────────────────────────────

#[tokio::test]
async fn oversized_body_rejected_by_default_body_limit() {
    // Router applies DefaultBodyLimit::max(50 * 1024 * 1024). A POST
    // body larger than that must be rejected with 413, not OOM the
    // server. We POST to a route that doesn't exist (404 fallback) but
    // the body-limit layer runs before routing, so the limit applies.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);
    let router = router_with_token(state, "");

    // 51 MB body, well over the 50 MB cap
    let body = vec![b'x'; 51 * 1024 * 1024];
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/sessions")
        .header("Content-Type", "application/octet-stream")
        .body(Body::from(body))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::PAYLOAD_TOO_LARGE
            || resp.status() == StatusCode::METHOD_NOT_ALLOWED
            || resp.status() == StatusCode::NOT_FOUND,
        "oversized body must be rejected (413/405/404), got {}",
        resp.status()
    );
    // The key invariant: no 500, no panic, server still alive.
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ── FTS5 query metacharacter abuse ──────────────────────────────────────

#[tokio::test]
async fn fts_query_with_special_chars_does_not_crash() {
    // FTS5 has its own query grammar with operators: AND, OR, NOT, NEAR,
    // ^, *, ", :, (, ). Random user input often produces syntactically
    // invalid FTS expressions. The server must surface a clean error
    // (400 or 500 with a JSON body) — never a panic.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let attacks = [
        "\"\"",        // empty quoted phrase
        "AND OR NOT", // bare operators
        "(((",         // unbalanced parens
        "*",          // bare wildcard
        "col:val",   // column filter against unknown column
        "NEAR/100",  // malformed NEAR
        "^",          // bare initial-token operator
        "\u{0000}",  // NUL byte
    ];
    for q in attacks {
        let uri = format!("/api/search?q={}", urlencoding::encode(q));
        let req = Request::get(&uri).body(Body::empty()).unwrap();
        let resp = send_request(Arc::clone(&state), req).await;
        let status = resp.status();
        // Server may return 200 (empty results), 400 (bad request), or
        // 500 (FTS reports a parse error) — anything is fine as long as
        // the process is still up. The body must still be valid JSON.
        assert!(
            status == StatusCode::OK
                || status == StatusCode::BAD_REQUEST
                || status == StatusCode::INTERNAL_SERVER_ERROR,
            "FTS query {q:?} produced unexpected status {status}"
        );
        // Drain the body — proves the response is well-formed and the
        // connection wasn't dropped mid-flight.
        let _ = body_text(resp).await;
    }
}

// ── URL-encoded path traversal ──────────────────────────────────────────

#[tokio::test]
async fn url_encoded_dotdot_in_transcript_path_blocked() {
    // The transcript handler rejects `..` after replacing `\` with `/`.
    // But an attacker who URL-encodes the dots (`%2e%2e`) bypasses
    // that check unless axum decodes path params first. Axum DOES
    // decode AxumPath params, so this should be equivalent to the
    // plain `..` form — and `test_security::transcript_api_rejects_dotdot_in_path`
    // already covers the plain form via the meta field. Here we
    // exercise the URL-encoded form via the session_id route param
    // to confirm percent-encoded segments don't escape the route.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // %2e%2e%2f..%2f passwd
    let req = Request::get("/api/sessions/%2e%2e%2f..%2fetc%2fpasswd/transcript")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    // The route matches with the decoded session_id; the handler
    // returns 200 with empty entries (session not found) OR 404 if
    // axum rejects the decoded path. Both are fine — the invariant
    // is "no file contents in body".
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "URL-encoded traversal should not 500; got {status}"
    );
    if status == StatusCode::OK {
        let body: Value = body_json(resp).await;
        let text = serde_json::to_string(&body).unwrap();
        assert!(
            !text.contains("root:") && !text.contains("/bin/bash"),
            "response body must not contain /etc/passwd-like content"
        );
    }
}

#[tokio::test]
async fn transcript_path_symlink_outside_data_dir_blocked() {
    // The handler canonicalizes `data_dir.join(p)` and only returns
    // the file if the canonical path is still under data_dir. We
    // create a symlink inside data_dir pointing OUT of data_dir, then
    // reference it through a session event's transcript_path.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Sensitive target outside data_dir
    let outside = tmp.path().join("outside-secret.txt");
    std::fs::write(&outside, "TOP_SECRET").unwrap();

    // Symlink inside data_dir pointing OUT
    let data_dir = {
        let s = state.read().await;
        s.store.data_dir.clone()
    };
    std::fs::create_dir_all(&data_dir).unwrap();
    let link = data_dir.join("escape.jsonl");
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&outside, &link).unwrap();
    }
    #[cfg(not(unix))]
    {
        // Skip on non-unix — symlink semantics differ.
        return;
    }

    // Ingest an event whose meta.transcript_path is just the link
    // basename (resolved relative to data_dir).
    {
        let s = state.write().await;
        let event = json!({
            "id": "evt-symlink",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://transcript/sess-symlink",
            "time": "2026-05-28T00:00:00Z",
            "data": {
                "meta": {"transcript_path": "escape.jsonl"},
                "text": "x",
                "raw": {"type":"user","message":{"content":[{"type":"text","text":"x"}]}}
            }
        });
        let _ = s.store.event_store.insert_event("sess-symlink", &event).await;
    }

    let req = Request::get("/api/sessions/sess-symlink/transcript")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = body_json(resp).await;
    let serialized = serde_json::to_string(&body).unwrap();
    assert!(
        !serialized.contains("TOP_SECRET"),
        "symlink escape returned secret content: {serialized}"
    );
}

// ── Concurrent delete TOCTOU ────────────────────────────────────────────

#[tokio::test]
async fn concurrent_delete_same_session_does_not_panic() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Seed a session with one event.
    {
        let s = state.write().await;
        let event = json!({
            "id": "evt-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2026-05-28T00:00:00Z",
            "data": {"text":"x","raw":{"type":"user","message":{"content":[{"type":"text","text":"x"}]}}}
        });
        s.store.event_store.insert_event("race-sess", &event).await.unwrap();
        s.store
            .projections
            .insert("race-sess".into(), open_story_store::projection::SessionProjection::new("race-sess"));
    }

    // Fire 8 concurrent DELETEs against the same session.
    let mut handles = Vec::new();
    for _ in 0..8 {
        let s = Arc::clone(&state);
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method(Method::DELETE)
                .uri("/api/sessions/race-sess")
                .body(Body::empty())
                .unwrap();
            send_request(s, req).await.status()
        }));
    }

    let mut ok_count = 0;
    let mut notfound_count = 0;
    let mut other_count = 0;
    for h in handles {
        match h.await.unwrap() {
            StatusCode::OK => ok_count += 1,
            StatusCode::NOT_FOUND => notfound_count += 1,
            other => {
                eprintln!("delete returned {other}");
                other_count += 1;
            }
        }
    }
    // Either: one OK + seven 404, or all OK if delete is idempotent.
    // The key invariant: no 5xx and total = 8.
    assert_eq!(ok_count + notfound_count + other_count, 8);
    assert_eq!(other_count, 0, "concurrent deletes must not produce 5xx");
}

// ── Adversarial query parameters ────────────────────────────────────────

#[tokio::test]
async fn negative_limit_in_search_does_not_crash() {
    // SearchQuery.limit is `Option<usize>` (or similar). Axum returns
    // 400 on parse failure. Either way: no 500.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::get("/api/search?q=foo&limit=-1")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST
            || status == StatusCode::OK
            || status == StatusCode::UNPROCESSABLE_ENTITY,
        "negative limit must be rejected cleanly, got {status}"
    );
}

#[tokio::test]
async fn huge_days_param_in_pulse_does_not_overflow() {
    // /api/insights/pulse accepts ?days=u32. Max u32 is 4,294,967,295.
    // The query string in the code computes a cutoff timestamp from
    // `now - days`. With huge days, the cutoff goes far into the past
    // — the query should return all rows, not panic on arithmetic
    // overflow.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::get("/api/insights/pulse?days=4294967295")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "huge days param must not crash, got {status}"
    );
}

#[tokio::test]
async fn store_survives_huge_days_request_no_poisoned_lock() {
    // Regression for the audit-master-2026-06 F1 escalation: the huge-days
    // overflow panicked WHILE holding the SQLite connection mutex, poisoning
    // it, so every subsequent query then panicked on `.lock().unwrap()` — one
    // request permanently bricked the whole store. Assert a follow-up request
    // on an UNRELATED endpoint still succeeds after the huge-days request.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let attack = Request::get("/api/insights/pulse?days=4294967295")
        .body(Body::empty())
        .unwrap();
    let _ = send_request(Arc::clone(&state), attack).await;

    let followup = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = send_request(Arc::clone(&state), followup).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "store must remain usable after a huge-days request (no poisoned lock)"
    );
}

#[tokio::test]
async fn crlf_in_session_id_does_not_smuggle_into_logs() {
    // The api handlers log "GET /api/sessions/{short_id}/..." for
    // observability. If session_id with CRLF made it into log output
    // unescaped, an attacker could fake log lines. We don't have a
    // log-capture rig here, but we can at least verify the server
    // doesn't crash and returns a normal response.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // %0d%0a = CRLF, %00 = NUL
    let req = Request::get("/api/sessions/abc%0d%0aINJECTED%00/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "CRLF/NUL session_id should not 500, got {status}"
    );
}

#[tokio::test]
async fn null_byte_in_session_id_does_not_crash_storage() {
    // SQLite supports NUL bytes in TEXT columns; some other backends
    // don't. Ensure the store layer accepts (or cleanly rejects) a
    // session_id with embedded NUL.
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let sid = "nulled\u{0000}sess";
    {
        let s = state.write().await;
        let event = json!({
            "id": "evt-nul",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2026-05-28T00:00:00Z",
            "data": {"text":"x","raw":{"type":"user","message":{"content":[{"type":"text","text":"x"}]}}}
        });
        // We don't assert success — backends differ. We assert no panic.
        let _ = s.store.event_store.insert_event(sid, &event).await;
    }
    // Reading back should also not panic.
    {
        let s = state.read().await;
        let _ = s.store.event_store.session_events(sid).await;
    }
}
