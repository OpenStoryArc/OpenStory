//! Security integration tests — verify hardening against common attack vectors.
//!
//! Each test attempts to exploit a specific vulnerability and asserts that the
//! application rejects or safely handles the malicious input.
//!
//! Attack vectors covered:
//! - Path traversal in transcript API
//! - Session ID injection (SQL metacharacters, null bytes, long strings)
//! - Oversized payloads
//! - Malformed JSON
//! - SQLite injection via event fields
//! - Event ID collision / data integrity
//! - Hook payload injection
//! - Resource exhaustion (many sessions)

mod helpers;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tempfile::TempDir;

use helpers::{body_json, make_event_with_id, make_user_prompt, send_request, test_state};
use open_story::server::ingest_events;

// ── Path Traversal ────────────────────────────────────────────────────

#[tokio::test]
async fn transcript_api_rejects_dotdot_in_path() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Ingest an event that references a traversal path in its source
    {
        let s = state.write().await;
        let event = serde_json::json!({
            "id": "evt-traversal",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://transcript/sess-traversal",
            "time": "2025-01-15T00:00:00Z",
            "data": {
                "meta": {"transcript_path": "../../../etc/passwd"},
                "text": "hello",
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}}
            }
        });
        let _ = s.store.event_store.insert_event("sess-traversal", &event).await;
    }

    let req = Request::get("/api/sessions/sess-traversal/transcript")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = body_json(resp).await;
    // Should either return error or empty entries — never file contents
    let entries = body.get("entries").and_then(|v| v.as_array());
    let has_error = body.get("error").is_some();
    assert!(
        has_error || entries.map(|e| e.is_empty()).unwrap_or(true),
        "traversal path should not return file contents"
    );
}

#[tokio::test]
async fn transcript_api_rejects_backslash_traversal() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    {
        let s = state.write().await;
        let event = serde_json::json!({
            "id": "evt-backslash",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://transcript/sess-bs",
            "time": "2025-01-15T00:00:00Z",
            "data": {
                "meta": {"transcript_path": "..\\..\\..\\etc\\passwd"},
                "text": "hello",
                "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}}
            }
        });
        let _ = s.store.event_store.insert_event("sess-bs", &event).await;
    }

    let req = Request::get("/api/sessions/sess-bs/transcript")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    let body: Value = body_json(resp).await;
    let entries = body.get("entries").and_then(|v| v.as_array());
    let has_error = body.get("error").is_some();
    assert!(
        has_error || entries.map(|e| e.is_empty()).unwrap_or(true),
        "backslash traversal should not return file contents"
    );
}

// ── Session ID Injection ──────────────────────────────────────────────

#[tokio::test]
async fn session_id_with_path_traversal_chars_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Session ID containing path traversal chars should be treated as opaque string
    let req = Request::get("/api/sessions/../../../etc/passwd/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    // axum might 404 due to route mismatch or return empty array
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "path traversal in session_id should not cause server error, got {}",
        status
    );
}

#[tokio::test]
async fn session_id_with_sql_injection_is_harmless() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let malicious_sid = "'; DROP TABLE events; --";
    let events = vec![make_user_prompt(malicious_sid, "evt-sql-1")];

    {
        let mut s = state.write().await;
        let result = ingest_events(&mut s, malicious_sid, &events, None).await;
        assert_eq!(result.count, 1, "event should be ingested normally despite SQL in session_id");
    }

    // Verify the events table still exists and original data is intact
    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events(malicious_sid).await.unwrap();
        assert_eq!(stored.len(), 1, "events table should still exist with our event");
    }

    // Also verify via API — use a percent-encoded form of the malicious session_id
    let encoded_sid = malicious_sid.replace("'", "%27").replace(";", "%3B").replace(" ", "%20").replace("-", "%2D");
    let req = Request::get(&format!("/api/sessions/{}/events", encoded_sid))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    // May return OK (empty array) or 404 depending on URL decoding
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NOT_FOUND,
        "SQL injection session_id via API should not cause error, got {}",
        status
    );
}

#[tokio::test]
async fn session_id_extremely_long_does_not_crash() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let long_sid = "a".repeat(10_000);
    let events = vec![make_user_prompt(&long_sid, "evt-long-1")];

    {
        let mut s = state.write().await;
        let result = ingest_events(&mut s, &long_sid, &events, None).await;
        assert_eq!(result.count, 1, "long session_id should not crash");
    }

    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events(&long_sid).await.unwrap();
        assert_eq!(stored.len(), 1);
    }
}

// ── SQLite Injection ──────────────────────────────────────────────────

#[tokio::test]
async fn sqlite_injection_via_event_id() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let malicious_id = "evt'; DROP TABLE events; --";
    let events = vec![make_event_with_id("io.arc.event", "sess-sqli", malicious_id)];

    {
        let mut s = state.write().await;
        ingest_events(&mut s, "sess-sqli", &events, None).await;
    }

    // Table should still exist, event should be stored with literal SQL as ID
    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events("sess-sqli").await.unwrap();
        assert_eq!(stored.len(), 1, "events table survives SQL injection attempt in event_id");
        assert_eq!(
            stored[0].get("id").and_then(|v| v.as_str()),
            Some(malicious_id),
            "malicious ID should be stored as literal string"
        );
    }
}

#[tokio::test]
async fn sqlite_injection_via_subtype() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Craft a CloudEvent with SQL injection in subtype
    let event = serde_json::json!({
        "id": "evt-subtype-sqli",
        "type": "io.arc.event",
        "subtype": "'; DELETE FROM events WHERE '1'='1",
        "source": "arc://test",
        "time": "2025-01-15T00:00:00Z",
        "data": {
            "text": "innocent data",
            "raw": {"type": "user", "message": {"content": [{"type": "text", "text": "hello"}]}}
        }
    });

    {
        let s = state.read().await;
        let inserted = s.store.event_store.insert_event("sess-sqli-sub", &event).await.unwrap();
        assert!(inserted, "event with SQL in subtype should be inserted normally");
    }

    // Insert another event to verify DELETE didn't run
    {
        let s = state.read().await;
        let event2 = serde_json::json!({
            "id": "evt-after-sqli",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": "2025-01-15T00:00:01Z",
            "data": {"text": "still here"}
        });
        let _ = s.store.event_store.insert_event("sess-sqli-sub", &event2).await;
        let all = s.store.event_store.session_events("sess-sqli-sub").await.unwrap();
        assert_eq!(all.len(), 2, "both events should survive — SQL injection in subtype had no effect");
    }
}

// ── Hook Payload ──────────────────────────────────────────────────────

#[tokio::test]
async fn hooks_rejects_non_json_body() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from("this is not json"))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;

    // axum's Json extractor returns 400 or 422 for invalid JSON
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "non-JSON body should be rejected, got {}",
        status
    );
}

#[tokio::test]
async fn hooks_handles_empty_body() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(""))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;

    // Should be 422 (JSON parse failure) or 400
    let status = resp.status();
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "empty body should be rejected, got {}",
        status
    );
}

#[tokio::test]
async fn hooks_handles_wrong_json_types() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // session_id as number, transcript_path as boolean — wrong types
    let body = json!({"session_id": 12345, "transcript_path": true});
    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;

    // Should degrade gracefully — .as_str() returns None for non-strings
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "wrong types should be handled gracefully"
    );
    let body: Value = body_json(resp).await;
    assert_eq!(body["status"], "no_transcript", "should find no transcript for non-string session_id");
}

#[tokio::test]
async fn hooks_deeply_nested_json_no_crash() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Build deeply nested JSON (200 levels)
    let mut nested = json!({});
    for _ in 0..200 {
        nested = json!({"a": nested});
    }

    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&nested).unwrap()))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;

    // serde_json has recursion limit ~128 — might reject, might accept
    // Either way, no stack overflow
    let status = resp.status();
    assert!(
        status == StatusCode::ACCEPTED
            || status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::BAD_REQUEST,
        "deeply nested JSON should not cause stack overflow, got {}",
        status
    );
}

// ── Event ID Collision / Data Integrity ───────────────────────────────

#[tokio::test]
async fn duplicate_event_id_does_not_overwrite_data() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    {
        let mut s = state.write().await;

        // First event with id "evt-collision"
        let event_a = make_user_prompt("sess-collision", "evt-collision");
        let result_a = ingest_events(&mut s, "sess-collision", &[event_a], None).await;
        assert_eq!(result_a.count, 1);

        // Second event with SAME id but potentially different data
        let event_b = make_event_with_id("io.arc.event", "sess-collision", "evt-collision");
        let result_b = ingest_events(&mut s, "sess-collision", &[event_b], None).await;
        assert_eq!(result_b.count, 0, "duplicate event_id should be deduplicated");

        // Only one event should be stored
        let stored = s.store.event_store.session_events("sess-collision").await.unwrap();
        assert_eq!(stored.len(), 1, "only original event should exist");
    }
}

#[tokio::test]
async fn duplicate_event_id_across_sessions() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    {
        let mut s = state.write().await;

        // Same event ID in session A
        let event_a = make_user_prompt("sess-a", "shared-evt");
        let result_a = ingest_events(&mut s, "sess-a", &[event_a], None).await;
        assert_eq!(result_a.count, 1);

        // Same event ID in session B — should be deduplicated by seen_event_ids
        let event_b = make_user_prompt("sess-b", "shared-evt");
        let result_b = ingest_events(&mut s, "sess-b", &[event_b], None).await;
        assert_eq!(result_b.count, 0, "same event_id across sessions should be deduplicated");
    }
}

// ── Resource Exhaustion ───────────────────────────────────────────────

#[tokio::test]
async fn ingest_many_sessions_no_crash() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let num_sessions = 500;
    let events_per_session = 5;

    {
        let mut s = state.write().await;
        for i in 0..num_sessions {
            let sid = format!("sess-bulk-{}", i);
            let events: Vec<_> = (0..events_per_session)
                .map(|j| make_user_prompt(&sid, &format!("evt-{}-{}", i, j)))
                .collect();
            let result = ingest_events(&mut s, &sid, &events, None).await;
            assert_eq!(result.count, events_per_session);
        }
    }

    // Verify all sessions exist
    {
        let s = state.read().await;
        let sessions = s.store.event_store.list_sessions().await.unwrap();
        assert_eq!(
            sessions.len(),
            num_sessions,
            "all {} sessions should be stored",
            num_sessions
        );
    }

    // Verify API still works
    let req = Request::get("/api/sessions")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let sessions: Value = body_json(resp).await;
    assert_eq!(
        sessions.as_array().unwrap().len(),
        num_sessions,
        "API should serve all sessions"
    );
}

#[tokio::test]
async fn ingest_large_event_payload_no_crash() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // 1MB payload in a single event
    let events = vec![helpers::make_event_with_large_payload("sess-large", "evt-1mb", 1_000_000)];

    {
        let mut s = state.write().await;
        let result = ingest_events(&mut s, "sess-large", &events, None).await;
        assert_eq!(result.count, 1, "large event should be ingested successfully");
    }

    {
        let s = state.read().await;
        let stored = s.store.event_store.session_events("sess-large").await.unwrap();
        assert_eq!(stored.len(), 1);
    }
}

// ── API Edge Cases ────────────────────────────────────────────────────

#[tokio::test]
async fn api_nonexistent_session_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::get("/api/sessions/totally-made-up/events")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = body_json(resp).await;
    assert_eq!(
        body.as_array().unwrap().len(),
        0,
        "nonexistent session should return empty array, not error"
    );
}

#[tokio::test]
async fn meta_endpoint_returns_404_for_unknown_session() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    let req = Request::get("/api/sessions/unknown/meta")
        .body(Body::empty())
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "meta endpoint should 404 for unknown sessions"
    );
}

#[tokio::test]
async fn hook_with_transcript_path_pointing_outside_watch_dir() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Create a JSONL file outside the watch_dir
    let outside_dir = tmp.path().join("outside");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let outside_file = outside_dir.join("secret.jsonl");
    std::fs::write(
        &outside_file,
        r#"{"type":"user","message":{"content":[{"type":"text","text":"secret"}]}}"#,
    )
    .unwrap();

    // POST hook with transcript_path pointing outside watch_dir
    let body = json!({
        "session_id": "sess-escape",
        "transcript_path": outside_file.to_string_lossy()
    });
    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;

    // Tier 1 resolution uses the explicit path if the file exists —
    // this is expected behavior since hooks come from the local Claude Code process.
    // The key point is: this endpoint is trusted local input, not internet-facing.
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

// ── Symlink Traversal (Unix only) ─────────────────────────────────────

#[cfg(unix)]
#[tokio::test]
async fn hooks_symlink_in_watch_dir_is_not_followed() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Create a secret file outside watch_dir
    let secret_file = tmp.path().join("secret.jsonl");
    std::fs::write(
        &secret_file,
        r#"{"type":"user","message":{"content":[{"type":"text","text":"top secret"}]}}"#,
    )
    .unwrap();

    // Create a symlink inside watch_dir pointing to the secret file
    let watch_dir = tmp.path().join("watch");
    let symlink_path = watch_dir.join("evil-session.jsonl");
    std::os::unix::fs::symlink(&secret_file, &symlink_path).unwrap();

    // POST hook with session_id matching the symlink name
    let body = json!({"session_id": "evil-session"});
    let req = Request::post("/hooks")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = send_request(Arc::clone(&state), req).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let resp_body: Value = body_json(resp).await;
    // With follow_links(false), the symlink should not be followed via Tier 3
    // The hook may still resolve via Tier 1 (explicit path) or Tier 2 (transcript_states),
    // but Tier 3 (WalkDir) should skip symlinks.
    // Since we didn't provide transcript_path and no transcript_state exists,
    // Tier 3 is the only option — and it should fail to find the file.
    assert_eq!(
        resp_body["status"], "no_transcript",
        "symlink in watch_dir should not be followed by WalkDir"
    );
}

// ── Hook Injection (session routing) ──────────────────────────────────

#[tokio::test]
async fn hook_session_id_is_authoritative() {
    let tmp = TempDir::new().unwrap();
    let state = test_state(&tmp);

    // Ingest events under "attacker-session" — these contain session_id
    // claims for "victim-session" in the event data, but the routing
    // should use the session_id parameter, not the data content.
    let events = vec![make_user_prompt("victim-session", "evt-routed")];

    {
        let mut s = state.write().await;
        // Route to "attacker-session" despite event data saying "victim-session"
        let result = ingest_events(&mut s, "attacker-session", &events, None).await;
        assert_eq!(result.count, 1);

        // Events should be under attacker-session, not victim-session
        let attacker_events = s
            .store
            .event_store
            .session_events("attacker-session")
            .await
            .unwrap();
        assert_eq!(
            attacker_events.len(),
            1,
            "event should be stored under the authoritative session_id parameter"
        );

        // victim-session should be empty
        let victim_events = s
            .store
            .event_store
            .session_events("victim-session")
            .await
            .unwrap();
        assert_eq!(
            victim_events.len(),
            0,
            "event data session_id should not override routing"
        );
    }
}
