//! POST /api/memory and its reads (memory hands, D-02): where a host's
//! judgment lands. Validated against the kind's schema and, for readings,
//! against the arc's exchanges; stored; published best-effort.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use support::{app_with, golden_expected, TestApp};
use tower::ServiceExt;

async fn post(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let req = Request::post("/api/memory")
        .header("authorization", format!("Bearer {}", app.api_token))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let req = Request::get(path)
        .header("authorization", format!("Bearer {}", app.api_token))
        .body(Body::empty())
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn author() -> Value {
    json!({ "host": "claude-code", "model": "claude-fable-5-1" })
}

mod when_a_memory_record_is_posted {
    use super::*;

    #[tokio::test]
    async fn without_an_author_it_is_rejected() {
        let app = app_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let (status, body) = post(&app, json!({
            "kind": "enrichment", "handle": arc["handle"], "session_id": "golden-two_arcs_gap",
            "payload": { "handle": arc["handle"], "title": "t", "question": "q", "resolution": "r", "summary": "s" }
        })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].as_str().unwrap().contains("author"), "{body}");
        assert!(
            app.bus.subjects().is_empty(),
            "nothing published on a rejected write"
        );
    }

    #[tokio::test]
    async fn a_reading_naming_an_unknown_handle_is_rejected() {
        let app = app_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let mut exchanges: Vec<Value> = arc["exchanges"].as_array().unwrap().clone();
        exchanges.push(json!("ex0000000000dead"));
        let (status, body) = post(&app, json!({
            "kind": "reading", "handle": arc["handle"], "session_id": "golden-two_arcs_gap", "author": author(),
            "payload": { "handle": arc["handle"], "standing": "final",
                "paragraphs": [ { "exchanges": exchanges, "intent": "everything" } ], "author": author() }
        })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("ex0000000000dead"),
            "names the stranger: {body}"
        );
    }

    #[tokio::test]
    async fn a_valid_enrichment_is_stored_published_and_readable() {
        let app = app_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let sid = "golden-two_arcs_gap";
        let (status, body) = post(&app, json!({
            "kind": "enrichment", "handle": arc["handle"], "session_id": sid, "author": author(),
            "payload": { "handle": arc["handle"], "title": "Plan, then build", "question": "q", "resolution": "r", "summary": "s",
                "slots": { "decisions": ["d1"] }, "author": author() }
        })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["kind"], "enrichment");
        assert_eq!(body["handle"], arc["handle"]);
        assert_eq!(body["author"]["host"], "claude-code");
        assert!(
            body["created_at"].as_str().unwrap().starts_with("20"),
            "server stamps created_at"
        );
        assert_eq!(
            body["id"],
            json!(format!(
                "enrichment:{}:-:claude-code:claude-fable-5-1",
                arc["handle"].as_str().unwrap()
            ))
        );

        let (s, by_session) = get(&app, &format!("/api/sessions/{sid}/memory")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(by_session["memory"].as_array().unwrap().len(), 1);
        assert_eq!(
            by_session["memory"][0]["payload"]["title"],
            "Plan, then build"
        );
        let (s, by_handle) = get(
            &app,
            &format!("/api/memory/{}", arc["handle"].as_str().unwrap()),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(by_handle["memory"][0]["id"], body["id"]);

        let subjects = app.bus.subjects();
        assert_eq!(
            subjects,
            vec![format!("memory.enrichment.{sid}")],
            "published on memory.{{kind}}.{{session}}"
        );
        assert_eq!(
            app.bus.payloads()[0]["id"],
            body["id"],
            "the stored record is what rides the bus"
        );
    }

    #[tokio::test]
    async fn a_valid_final_reading_is_accepted() {
        let app = app_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let (status, body) = post(&app, json!({
            "kind": "reading", "handle": arc["handle"], "session_id": "golden-two_arcs_gap", "author": author(),
            "payload": { "handle": arc["handle"], "standing": "final",
                "paragraphs": [ { "exchanges": arc["exchanges"], "intent": "one paragraph" } ], "author": author() }
        })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["standing"], "final");
        assert!(body["id"].as_str().unwrap().contains(":final:"));
    }
}

mod when_a_memory_record_is_stored {
    use super::*;
    use open_story_server::broadcast::BroadcastMessage;

    #[tokio::test]
    async fn the_dashboard_is_told() {
        let mut app = app_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let (status, body) = post(&app, json!({
            "kind": "enrichment", "handle": arc["handle"], "session_id": "golden-two_arcs_gap", "author": author(),
            "payload": { "handle": arc["handle"], "title": "t", "question": "q", "resolution": "r", "summary": "s", "author": author() }
        })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let msg = app.broadcast_rx.try_recv().expect("one broadcast message");
        match msg {
            BroadcastMessage::Memory { record } => {
                assert_eq!(record.id, body["id"].as_str().unwrap());
                assert_eq!(record.session_id, "golden-two_arcs_gap");
            }
            other => panic!("expected a Memory broadcast, got {other:?}"),
        }
        let wire = serde_json::to_value(BroadcastMessage::Memory {
            record: serde_json::from_value(body.clone()).unwrap(),
        })
        .unwrap();
        assert_eq!(wire["kind"], "memory", "wire tag the UI switches on");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Store-wide search over story patterns and memory (B-11)
// ═══════════════════════════════════════════════════════════════════

mod when_story_and_memory_are_searched {
    use super::*;

    #[tokio::test]
    async fn it_finds_patterns_and_records_across_sessions() {
        let app = app_with(&["two_arcs_gap", "single_arc_plain"]).await;
        let (s, hits) = get(&app, "/api/story/search?q=golden%20prompt%200&limit=50").await;
        assert_eq!(s, StatusCode::OK, "{hits}");
        let sessions: std::collections::BTreeSet<&str> = hits["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["session_id"].as_str())
            .collect();
        assert_eq!(sessions.len(), 2, "both seeded sessions answer: {hits}");

        let arc = &golden_expected("two_arcs_gap")["arcs"][0];
        let sid = "golden-two_arcs_gap";
        let body = json!({
            "kind": "enrichment", "handle": arc["handle"], "session_id": sid,
            "author": { "host": "claude-code", "model": "m" },
            "payload": { "handle": arc["handle"], "title": "Kestrel migration lands",
                         "question": "q", "resolution": "r", "summary": "s" }
        });
        let (s, _) = post(&app, body).await;
        assert_eq!(s, StatusCode::OK);

        let (s, found) = get(&app, "/api/memory/search?q=kestrel").await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(found["memory"].as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found["memory"][0]["handle"], arc["handle"]);
    }
}
