//! Group M, tier 0: the ops hands an agent uses to watch and diagnose a
//! node through the MCP. Every hand reads the REST API; none writes.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod common;

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use common::{call_tool, make_test_server, unwrap_tool_result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn spawn_mock(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

/// What /api/health serves after M-01: the body with its own verdict.
fn health_body() -> Value {
    json!({
        "status": "ok",
        "host": "node-a",
        "boot": {"phase": "serving", "replay": {"done": 3, "total": 3, "elapsed_ms": 9}},
        "bus": {"connected": true},
        "leaf": {"configured": true, "connected": false, "hub": "hub:7422"},
        "streams": [
            {"name": "events", "bytes": 750, "messages": 10, "max_bytes": 1000, "percent": 0.75},
            {"name": "ui", "bytes": 0, "messages": 0, "max_bytes": null, "percent": null}
        ],
        "consumers": {"persist": {"alive": true, "restarts": 0, "lag": 0}},
        "watchers_detail": [],
        "presence": {"beats": 4, "failures": 0, "last_error": null, "interval_secs": 15},
        "verdict": {
            "level": "critical",
            "findings": [
                {"level": "critical", "id": "leaf_down", "text": "the leaf link to hub:7422 is not connected"},
                {"level": "warn", "id": "stream_cap:events", "text": "stream events is at 75% of its cap"}
            ]
        }
    })
}

fn mock_router() -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(health_body()) }))
        // Echoes its query so a spec can see which filters the hand passed.
        .route(
            "/api/logs",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                Json(json!({
                    "lines": [{"seq": 7, "level": "WARN", "actor": q.get("actor").cloned().unwrap_or_default(), "event": "publish_failed"}],
                    "next": 7,
                    "query": q,
                }))
            }),
        )
        .route(
            "/api/fleet/presence",
            get(|| async {
                Json(json!({
                    "interval_secs": 15,
                    "stale_after_secs": 45,
                    "nodes": [
                        {"host": "node-a", "principal_id": "dev", "age_secs": 5, "stale": false, "git_sha": "aaa"},
                        {"host": "node-b", "principal_id": "hub", "age_secs": 600, "stale": true, "git_sha": "bbb"}
                    ]
                }))
            }),
        )
}

async fn server_against_mock() -> (
    open_story_mcp::server::Server<common::LoopbackSubscriber>,
    tempfile::TempDir,
) {
    let base = spawn_mock(mock_router()).await;
    let (server, _sub, dir) = make_test_server();
    (server.with_api_base(base), dir)
}

mod when_node_health_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_verdict_and_findings() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(server, "node_health", json!({})).await;
        let v = unwrap_tool_result(&resp).expect("node_health succeeds");
        assert_eq!(v["verdict"]["level"], "critical", "{v}");
        let ids: Vec<&str> = v["verdict"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["leaf_down", "stream_cap:events"]);
        assert_eq!(v["host"], "node-a", "the whole health body rides along");
        assert_eq!(v["bus"]["connected"], true);
    }
}

mod when_node_logs_is_called_with_actor {
    use super::*;

    #[tokio::test]
    async fn it_filters() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(
            server,
            "node_logs",
            json!({"actor": "presence", "level": "WARN", "since": 3, "limit": 20}),
        )
        .await;
        let v = unwrap_tool_result(&resp).expect("node_logs succeeds");
        assert_eq!(v["query"]["actor"], "presence", "{v}");
        assert_eq!(v["query"]["level"], "WARN");
        assert_eq!(v["query"]["since"], "3");
        assert_eq!(v["query"]["limit"], "20");
        assert_eq!(v["lines"][0]["actor"], "presence");
        assert_eq!(v["next"], 7);
    }

    #[tokio::test]
    async fn it_passes_nothing_when_given_nothing() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(server, "node_logs", json!({})).await;
        let v = unwrap_tool_result(&resp).expect("node_logs succeeds");
        assert_eq!(
            v["query"],
            json!({}),
            "no filter means no query string: {v}"
        );
    }
}

mod when_node_streams_is_called {
    use super::*;

    #[tokio::test]
    async fn it_reports_percent_of_cap() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(server, "node_streams", json!({})).await;
        let v = unwrap_tool_result(&resp).expect("node_streams succeeds");
        let streams = v["streams"].as_array().expect("streams array");
        assert_eq!(streams.len(), 2, "{v}");
        assert_eq!(streams[0]["name"], "events");
        assert_eq!(streams[0]["bytes"], 750);
        assert_eq!(streams[0]["max_bytes"], 1000);
        assert_eq!(streams[0]["percent"], 0.75);
        assert_eq!(
            streams[0]["level"], "warn",
            "70 % warns, as the verdict does"
        );
        assert_eq!(
            streams[1]["percent"],
            Value::Null,
            "an uncapped stream has no percent"
        );
        assert_eq!(streams[1]["level"], "ok");
        assert!(
            v.get("verdict").is_none(),
            "streams only, not the whole body: {v}"
        );
    }
}

mod when_fleet_presence_is_called {
    use super::*;

    #[tokio::test]
    async fn it_lists_nodes_with_staleness() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(server, "fleet_presence", json!({})).await;
        let v = unwrap_tool_result(&resp).expect("fleet_presence succeeds");
        assert_eq!(v["interval_secs"], 15);
        let nodes = v["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "{v}");
        assert_eq!(nodes[0]["host"], "node-a");
        assert_eq!(nodes[0]["stale"], false);
        assert_eq!(nodes[1]["host"], "node-b");
        assert_eq!(nodes[1]["stale"], true);
        assert_eq!(nodes[1]["age_secs"], 600);
    }
}

mod when_the_api_base_is_unset {
    use super::*;

    #[tokio::test]
    async fn every_hand_says_which_env_var_is_missing() {
        for hand in ["node_health", "node_logs", "node_streams", "fleet_presence"] {
            let (server, _sub, _dir) = make_test_server();
            let resp = call_tool(server, hand, json!({})).await;
            let err = unwrap_tool_result(&resp).expect_err("no api base is an error");
            assert!(err.contains("OPENSTORY_API_URL"), "{hand}: {err}");
        }
    }
}

mod when_tools_are_listed {
    #[test]
    fn the_tier_zero_hands_are_registered_with_schemas() {
        let names: Vec<&str> = open_story_mcp::tools::TOOLS
            .iter()
            .map(|t| t.name)
            .collect();
        for hand in ["node_health", "node_logs", "node_streams", "fleet_presence"] {
            assert!(names.contains(&hand), "{hand} registered; have {names:?}");
            let def = open_story_mcp::tools::TOOLS
                .iter()
                .find(|t| t.name == hand)
                .unwrap();
            let schema = (def.input_schema)();
            assert_eq!(schema["type"], "object", "{hand} schema: {schema}");
            assert!(
                def.description.contains("MOTION:"),
                "{hand} names its motion: {}",
                def.description
            );
        }
        let logs = open_story_mcp::tools::TOOLS
            .iter()
            .find(|t| t.name == "node_logs")
            .unwrap();
        let schema = (logs.input_schema)();
        for key in ["since", "actor", "level", "limit"] {
            assert!(
                schema["properties"].get(key).is_some(),
                "node_logs takes {key}: {schema}"
            );
        }
    }
}
