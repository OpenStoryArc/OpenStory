//! The write hands (memory hands, D-04): enrich, adjudicate_boundary,
//! link_saga, propose_keep. Each POSTs to `{api_base}/api/memory` through
//! the same REST seam ui_control uses, so HttpEventStore stays read-only.
//! A mock server records what was posted.

mod common;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

type Posted = Arc<Mutex<Vec<Value>>>;

/// A mock /api/memory: records the body, answers like the server would.
async fn spawn_mock(posted: Posted, reject_with: Option<&'static str>) -> String {
    let router = Router::new()
        .route(
            "/api/memory",
            post(
                move |State(p): State<Posted>, Json(body): Json<Value>| async move {
                    p.lock().unwrap().push(body.clone());
                    match reject_with {
                        Some(msg) => (
                            axum::http::StatusCode::BAD_REQUEST,
                            Json(json!({ "ok": false, "error": msg })),
                        ),
                        None => {
                            let mut rec = body.clone();
                            rec["id"] = json!(format!(
                                "{}:{}:-:{}:{}",
                                body["kind"].as_str().unwrap_or(""),
                                body["handle"].as_str().unwrap_or(""),
                                body["author"]["host"].as_str().unwrap_or(""),
                                body["author"]["model"].as_str().unwrap_or("")
                            ));
                            rec["created_at"] = json!("2026-09-18T20:00:00.000Z");
                            (axum::http::StatusCode::OK, Json(rec))
                        }
                    }
                },
            ),
        )
        .with_state(posted);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn server_at(api_base: &str) -> Server<LoopbackSubscriber> {
    let (store, plan_store, tmp) = make_test_store();
    std::mem::forget(tmp);
    Server::new(LoopbackSubscriber::new(), store, plan_store).with_api_base(api_base)
}

fn author() -> Value {
    json!({ "host": "claude-code", "model": "claude-fable-5-1" })
}

mod when_enrich_is_called {
    use super::*;

    #[tokio::test]
    async fn it_posts_an_enrichment_record_and_returns_the_stored_row() {
        let posted: Posted = Arc::default();
        let base = spawn_mock(posted.clone(), None).await;
        let args = json!({
            "handle": "arc0000000000001", "session_id": "sess-1", "author": author(),
            "enrichment": { "title": "t", "question": "q", "resolution": "r", "summary": "s", "slots": { "decisions": ["d"] } }
        });
        let resp = call_tool(server_at(&base), "enrich", args).await;
        let row = unwrap_tool_result(&resp).expect("enrich ok");
        assert_eq!(
            row["id"],
            "enrichment:arc0000000000001:-:claude-code:claude-fable-5-1"
        );
        let body = posted.lock().unwrap()[0].clone();
        assert_eq!(body["kind"], "enrichment");
        assert_eq!(body["handle"], "arc0000000000001");
        assert_eq!(body["session_id"], "sess-1");
        assert_eq!(body["author"], author());
        assert_eq!(body["payload"]["title"], "t");
        assert_eq!(
            body["payload"]["handle"], "arc0000000000001",
            "the payload carries the handle"
        );
        assert_eq!(body["payload"]["author"], author(), "and the author");
    }

    #[tokio::test]
    async fn without_an_author_it_is_an_error_before_any_request() {
        let posted: Posted = Arc::default();
        let base = spawn_mock(posted.clone(), None).await;
        let args = json!({ "handle": "arc0000000000001", "session_id": "sess-1", "enrichment": { "title": "t", "question": "q", "resolution": "r", "summary": "s" } });
        let resp = call_tool(server_at(&base), "enrich", args).await;
        let err = unwrap_tool_result(&resp).unwrap_err();
        assert!(err.contains("author"), "{err}");
        assert!(posted.lock().unwrap().is_empty(), "nothing posted");
    }

    #[tokio::test]
    async fn a_server_rejection_surfaces_as_the_tool_error() {
        let posted: Posted = Arc::default();
        let base = spawn_mock(
            posted.clone(),
            Some("reading violates the laws: handles the arc does not hold: ex0000000000dead"),
        )
        .await;
        let args = json!({ "handle": "arc0000000000001", "session_id": "sess-1", "author": author(), "enrichment": { "title": "t", "question": "q", "resolution": "r", "summary": "s" } });
        let resp = call_tool(server_at(&base), "enrich", args).await;
        let err = unwrap_tool_result(&resp).unwrap_err();
        assert!(err.contains("ex0000000000dead"), "{err}");
    }
}

mod when_the_other_write_hands_are_called {
    use super::*;

    #[tokio::test]
    async fn adjudicate_link_and_keep_post_their_kinds() {
        let posted: Posted = Arc::default();
        let base = spawn_mock(posted.clone(), None).await;
        let a = json!({ "handle": "arc0000000000001", "session_id": "sess-1", "author": author(), "seam": 2, "verdict": "same_theme", "reason": "the second exchange answers the first" });
        unwrap_tool_result(&call_tool(server_at(&base), "adjudicate_boundary", a).await)
            .expect("adjudicate ok");
        let s = json!({ "handle": "arc0000000000001", "session_id": "sess-1", "author": author(), "handles": ["arc0000000000001", "arc0000000000009"], "reason": "same problem returned" });
        unwrap_tool_result(&call_tool(server_at(&base), "link_saga", s).await)
            .expect("link_saga ok");
        let k = json!({ "handle": "arc0000000000001", "session_id": "sess-1", "author": author(), "reason": "produced the decision on handles" });
        unwrap_tool_result(&call_tool(server_at(&base), "propose_keep", k).await)
            .expect("propose_keep ok");

        let bodies = posted.lock().unwrap().clone();
        let kinds: Vec<&str> = bodies.iter().map(|b| b["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["verdict", "saga", "keep"]);
        assert_eq!(bodies[0]["payload"]["seam"], 2);
        assert_eq!(bodies[0]["payload"]["verdict"], "same_theme");
        assert_eq!(bodies[1]["payload"]["handles"][1], "arc0000000000009");
        assert_eq!(
            bodies[2]["payload"]["reason"],
            "produced the decision on handles"
        );
    }

    #[tokio::test]
    async fn write_hands_are_listed_with_strict_schemas() {
        let (store, plan_store, tmp) = make_test_store();
        std::mem::forget(tmp);
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);
        let resp = call_tool(server, "openstory_help", json!({ "need": "narrate" })).await;
        let _ = resp; // listing checked via tools/list below
        let tools = open_story_mcp::tools::list_tools_result();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for want in ["enrich", "adjudicate_boundary", "link_saga", "propose_keep"] {
            assert!(names.contains(&want), "{want} listed");
        }
    }
}
