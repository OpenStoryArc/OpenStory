//! L-06: an in-process log ring served at `GET /api/logs`, so an agent (or
//! the MCP's `node_logs` hand) reads a node's recent lines through the API
//! without shell access. Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story_server::logging::{build_subscriber, LogFormat};

async fn get_logs(state: &open_story::server::SharedState, query: &str) -> serde_json::Value {
    let req = Request::get(format!("/api/logs{query}"))
        .body(Body::empty())
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    assert_eq!(resp.status(), 200, "GET /api/logs{query}");
    body_json(resp).await
}

mod when_logs_are_requested_since_seq {
    use super::*;

    #[tokio::test]
    async fn it_returns_only_newer_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let sub = build_subscriber(LogFormat::Json, "info", std::io::sink);
        let _guard = tracing::subscriber::set_default(sub);

        let before = get_logs(&state, "?limit=1").await["next"]
            .as_u64()
            .expect("next is the newest seq, 0 when empty");

        {
            let span = tracing::info_span!("consumer", actor = "persist");
            let _g = span.enter();
            tracing::info!(event = "batch_persisted", session_id = "sess-9", "one");
        }
        tracing::warn!(event = "stream_near_cap", stream = "events", "two");

        let body = get_logs(&state, &format!("?since={before}")).await;
        let lines = body["lines"].as_array().expect("lines array");
        assert_eq!(lines.len(), 2, "{body}");
        assert_eq!(lines[0]["event"], "batch_persisted");
        assert_eq!(lines[0]["actor"], "persist");
        assert_eq!(lines[0]["session_id"], "sess-9");
        assert!(lines[0]["seq"].as_u64().unwrap() > before);
        assert_eq!(lines[1]["level"], "WARN");
        assert_eq!(lines[1]["stream"], "events");
        assert_eq!(body["next"], lines[1]["seq"], "next is the last seq returned");

        let by_actor = get_logs(&state, &format!("?since={before}&actor=persist")).await;
        assert_eq!(by_actor["lines"].as_array().unwrap().len(), 1);
        assert_eq!(by_actor["lines"][0]["event"], "batch_persisted");

        let by_level = get_logs(&state, &format!("?since={before}&level=WARN")).await;
        assert_eq!(by_level["lines"].as_array().unwrap().len(), 1);
        assert_eq!(by_level["lines"][0]["event"], "stream_near_cap");

        let limited = get_logs(&state, &format!("?since={before}&limit=1")).await;
        assert_eq!(limited["lines"].as_array().unwrap().len(), 1, "limit caps the page");
        assert_eq!(limited["lines"][0]["event"], "batch_persisted", "oldest first");
    }
}

mod when_the_ring_is_bounded {
    use open_story_server::logging::LogRing;

    #[test]
    fn it_keeps_the_newest_lines_within_count_and_bytes() {
        let ring = LogRing::with_limits(3, 10_000);
        for i in 0..5u64 {
            ring.push(serde_json::json!({"event": format!("e{i}")}));
        }
        let (lines, next) = ring.read(0, None, None, 100);
        let events: Vec<_> = lines.iter().map(|l| l["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(events, vec!["e2", "e3", "e4"], "count bound keeps the newest three");
        assert_eq!(next, 5);

        let small = LogRing::with_limits(100, 60);
        small.push(serde_json::json!({"event": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}));
        small.push(serde_json::json!({"event": "b"}));
        let (lines, _) = small.read(0, None, None, 100);
        assert_eq!(lines.len(), 1, "byte bound evicted the oldest");
        assert_eq!(lines[0]["event"], "b");
    }
}
