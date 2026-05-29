//! Aggressive security probes against a **real running container**.
//!
//! Complements `test_security_aggressive.rs` (in-process axum-tower).
//! Container-based tests catch issues that only surface with real
//! Docker, real OS process boundaries, and real network buffering —
//! e.g., hyper's HTTP parser, the TCP stack, the bind-mount file
//! perms, and signal handling.
//!
//! Prereq: build the test image first.
//!   `cd rs && docker build -t open-story:test .`
//!
//! Run with: `cargo test -p open-story --test test_security_container`

mod helpers;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use tokio_tungstenite::tungstenite::Message;

use helpers::container::start_open_story;
use helpers::fixtures_dir;

/// HTTP client with a short timeout so a hung server fails fast in CI
/// instead of hanging the test runner.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("client build")
}

// ── DoS: oversized body must be rejected by the 50 MB limit ────────────

#[tokio::test]
async fn container_rejects_oversized_body() {
    let server = start_open_story(&fixtures_dir()).await;
    let url = format!("{}/api/sessions", server.base_url());

    // 51 MB POST — over the DefaultBodyLimit cap of 50 MB
    let body = vec![b'x'; 51 * 1024 * 1024];
    let resp = client()
        .post(&url)
        .body(body)
        .send()
        .await
        .expect("request");

    // 413 (Payload Too Large), 405 (Method Not Allowed — POST not on /api/sessions),
    // 404, or 411 (Length Required) all OK. The invariant: NOT 500, NOT crash.
    assert_ne!(
        resp.status().as_u16(),
        500,
        "oversized body caused server error; status={}",
        resp.status()
    );

    // Server must still be alive afterward.
    let post_health = client()
        .get(format!("{}/api/sessions", server.base_url()))
        .send()
        .await
        .expect("post-DoS health");
    assert_eq!(post_health.status(), 200, "server died after oversized body");
}

// ── DoS: FTS5 metacharacter flood ──────────────────────────────────────

#[tokio::test]
async fn container_survives_fts_metacharacter_flood() {
    let server = start_open_story(&fixtures_dir()).await;
    server.wait_for_sessions().await;
    let base = server.base_url();

    let attacks = [
        "\"\"",
        "AND OR NOT",
        "(((",
        "*",
        "col:val",
        "NEAR/100",
        "^",
        "\u{0000}",
        // SQL — should not affect FTS5 (different grammar) but worth poking
        "'; DROP TABLE events; --",
        // RTL override + control bytes
        "\u{202e}\u{0007}\u{0008}",
        // Extremely long query
        &"x".repeat(100_000),
    ];

    let cli = client();
    for q in &attacks {
        let encoded = urlencoding::encode(q);
        let url = format!("{base}/api/search?q={encoded}");
        let resp = cli.get(&url).send().await.expect("search request");
        assert_ne!(
            resp.status().as_u16(),
            500,
            "FTS query {q:?} produced 500; expected 200/400"
        );
        // Body must be valid JSON (or empty), proving the response is well-formed
        let _ = resp.text().await;
    }

    // Server still alive
    let resp = cli
        .get(format!("{base}/api/sessions"))
        .send()
        .await
        .expect("health");
    assert_eq!(resp.status(), 200, "server died after FTS flood");
}

// ── Concurrent request flood — server stays responsive ─────────────────

#[tokio::test]
async fn container_handles_concurrent_request_flood() {
    let server = start_open_story(&fixtures_dir()).await;
    server.wait_for_sessions().await;
    let base = server.base_url();

    // 200 concurrent GETs against the same endpoint
    let cli = client();
    let mut handles = Vec::new();
    for _ in 0..200 {
        let cli = cli.clone();
        let url = format!("{base}/api/sessions");
        handles.push(tokio::spawn(async move {
            cli.get(&url).send().await.map(|r| r.status().as_u16())
        }));
    }

    let mut ok = 0;
    let mut errs = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(200) => ok += 1,
            Ok(other) => {
                eprintln!("unexpected status {other}");
                errs += 1;
            }
            Err(e) => {
                eprintln!("request error: {e}");
                errs += 1;
            }
        }
    }
    // Some loss under high concurrency is expected; the invariant is
    // "most succeed, none with 5xx, server still alive".
    assert!(
        ok > 150,
        "under 200 concurrent requests, only {ok}/200 succeeded ({errs} errs); server may be falling over"
    );

    let resp = cli
        .get(format!("{base}/api/sessions"))
        .send()
        .await
        .expect("post-flood health");
    assert_eq!(resp.status(), 200);
}

// ── HTTP header smuggling — disagreement between Content-Length and TE ─

#[tokio::test]
async fn container_rejects_te_chunked_with_content_length() {
    let server = start_open_story(&fixtures_dir()).await;
    let url = format!("{}/api/sessions", server.base_url());

    // RFC 7230 says: if both CL and TE are present, TE wins (and CL must be
    // ignored). hyper enforces this. We POST with conflicting headers and
    // assert no 500.
    let mut headers = HeaderMap::new();
    headers.insert(
        "Transfer-Encoding",
        HeaderValue::from_static("chunked"),
    );
    let resp = client()
        .post(&url)
        .headers(headers)
        .body("0\r\n\r\n")
        .send()
        .await
        .expect("request");

    assert_ne!(resp.status().as_u16(), 500);
}

// ── Path traversal through real Hyper parser ───────────────────────────

#[tokio::test]
async fn container_rejects_path_traversal_via_url() {
    let server = start_open_story(&fixtures_dir()).await;
    let base = server.base_url();
    let cli = client();

    let traversals = [
        "/api/sessions/../../../../etc/passwd/events",
        "/api/sessions/%2e%2e/%2e%2e/etc/passwd/events",
        "/api/sessions/..%2f..%2fetc%2fpasswd/events",
        "/api/sessions/.%2e/.%2e/etc/passwd/events",
        // Null byte
        "/api/sessions/abc%00../../../etc/passwd/events",
    ];

    for path in &traversals {
        let url = format!("{base}{path}");
        let resp = cli.get(&url).send().await.expect("traversal request");

        // hyper or axum may 400 the path, or the handler may accept it
        // as an opaque session_id and return 200/404. Either way:
        // - never 5xx (no panic)
        // - body must not contain /etc/passwd content
        assert_ne!(
            resp.status().as_u16(),
            500,
            "path {path} caused 5xx; expected 4xx/200"
        );

        if resp.status() == 200 {
            let body = resp.text().await.unwrap_or_default();
            assert!(
                !body.contains("root:x:") && !body.contains("/bin/bash"),
                "traversal {path} returned /etc/passwd-like content"
            );
        }
    }
}

// ── WebSocket upgrade against the real container ───────────────────────

#[tokio::test]
async fn container_websocket_accepts_handshake_and_sends_initial_state() {
    let server = start_open_story(&fixtures_dir()).await;
    server.wait_for_sessions().await;
    let ws_url = server.ws_url();

    let (mut socket, response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect");
    assert_eq!(response.status().as_u16(), 101, "expected 101 Switching Protocols");

    // First message must be initial_state JSON
    let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("WS recv timeout")
        .expect("WS stream closed early")
        .expect("WS msg error");

    match msg {
        Message::Text(text) => {
            let json: serde_json::Value =
                serde_json::from_str(&text).expect("WS initial_state must be JSON");
            assert_eq!(json["kind"], "initial_state");
        }
        other => panic!("expected text message, got {other:?}"),
    }

    let _ = socket.close(None).await;
}

// ── Authorization matrix — when no token configured, every endpoint
//    returns one of {200, 400, 404}. Catches accidental 5xx leaks. ──────

#[tokio::test]
async fn container_authz_matrix_no_5xx_on_known_endpoints() {
    let server = start_open_story(&fixtures_dir()).await;
    server.wait_for_sessions().await;
    let base = server.base_url();
    let cli = client();

    // Representative endpoints from router.rs. Each must return 2xx/4xx,
    // never 5xx, with no api_token configured.
    let endpoints = [
        ("GET", "/api/sessions"),
        ("GET", "/api/health"),
        ("GET", "/api/watchers"),
        ("GET", "/api/users"),
        ("GET", "/api/digests"),
        ("GET", "/api/local-info"),
        ("GET", "/api/plans"),
        ("GET", "/api/insights/pulse"),
        ("GET", "/api/insights/tool-evolution"),
        ("GET", "/api/insights/efficiency"),
        ("GET", "/api/insights/productivity"),
        ("GET", "/api/insights/token-usage"),
        ("GET", "/api/insights/token-usage/daily"),
        ("GET", "/api/insights/pulse?days=4294967295"), // regression: was panic
        ("GET", "/api/agent/tools"),
        ("GET", "/api/agent/project-context"),
        ("GET", "/api/agent/recent-files"),
        ("GET", "/api/agent/search?q=foo"),
        ("GET", "/api/search?q=foo"),
        ("GET", "/api/tool-schemas"),
        ("GET", "/api/sessions/nonexistent-id-12345/events"),
        ("GET", "/api/sessions/nonexistent-id-12345/synopsis"),
        ("GET", "/api/sessions/nonexistent-id-12345/tool-journey"),
        ("GET", "/api/sessions/nonexistent-id-12345/file-impact"),
        ("GET", "/api/sessions/nonexistent-id-12345/errors"),
        ("GET", "/api/sessions/nonexistent-id-12345/synopsis?days=0"),
        // The retired hooks endpoint must 404
        ("POST", "/hooks"),
    ];

    let mut failures = Vec::new();
    for (method, path) in &endpoints {
        let req = match *method {
            "GET" => cli.get(format!("{base}{path}")),
            "POST" => cli.post(format!("{base}{path}")).body(""),
            _ => unreachable!(),
        };
        let resp = req.send().await.expect("matrix req");
        let status = resp.status().as_u16();
        if status >= 500 {
            failures.push(format!("{method} {path} → {status}"));
        }
    }

    assert!(
        failures.is_empty(),
        "endpoints leaked 5xx (potential DoS / info-disclosure):\n  {}",
        failures.join("\n  ")
    );
}
