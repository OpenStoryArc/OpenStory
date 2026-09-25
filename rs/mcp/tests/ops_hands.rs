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

/// What /api/consistency serves (C-03): the report in the verdict's shape.
fn consistency_body() -> Value {
    json!({
        "host": "node-a",
        "level": "warn",
        "findings": [
            {"id": "diverged:node-b", "level": "warn", "text": "node-b differs on 1 project of 3"}
        ],
        "peers": [
            {"host": "node-b", "compared": true, "differing_projects": 1, "stale": false}
        ]
    })
}

fn mock_router() -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(health_body()) }))
        .route("/api/consistency", get(|| async { Json(consistency_body()) }))
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

/// M-05: `subscribe_health {interval_secs?}` streams verdict transitions and
/// any finding added or cleared, and says nothing while nothing changes.
mod when_health_flips_to_critical {
    use super::*;
    use open_story_mcp::stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::timeout;

    fn ok_body() -> Value {
        let mut b = health_body();
        b["leaf"]["connected"] = json!(true);
        b["streams"][0]["percent"] = json!(0.1);
        b["verdict"] = json!({"level": "ok", "findings": []});
        b
    }

    /// Health that is ok for the first two reads, then critical for good.
    fn flipping_router(calls: Arc<AtomicUsize>) -> Router {
        Router::new().route(
            "/api/health",
            get(move || {
                let calls = calls.clone();
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    Json(if n < 2 { ok_body() } else { health_body() })
                }
            }),
        )
    }

    #[tokio::test]
    async fn it_notifies_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let base = spawn_mock(flipping_router(calls.clone())).await;
        let (server, _sub, _dir) = make_test_server();
        let server = server.with_api_base(base);

        let (mut client_w, server_r) = tokio::io::duplex(64 * 1024);
        let (server_w, client_r) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(async move { stdio::run(server_r, server_w, server).await });

        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "subscribe_health", "arguments": {"interval_secs": 0.05}}});
        client_w
            .write_all(format!("{}\n", serde_json::to_string(&req).unwrap()).as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(client_r).lines();

        let ack: Value = serde_json::from_str(
            &timeout(Duration::from_secs(2), reader.next_line())
                .await
                .expect("ack within 2 s")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ack["id"], 1);
        let text: Value =
            serde_json::from_str(ack["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["status"], "started", "{text}");
        assert_eq!(
            text["verdict"]["level"], "ok",
            "the ack carries the current verdict: {text}"
        );
        let stream_id = text["stream_id"].as_str().unwrap().to_string();

        let notif: Value = serde_json::from_str(
            &timeout(Duration::from_secs(3), reader.next_line())
                .await
                .expect("the transition within 3 s")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(notif["method"], "notifications/openstory/health", "{notif}");
        let p = &notif["params"];
        assert_eq!(p["stream_id"], stream_id);
        assert_eq!(p["from"], "ok");
        assert_eq!(p["to"], "critical");
        assert_eq!(p["added"], json!(["leaf_down", "stream_cap:events"]), "{p}");
        assert_eq!(p["cleared"], json!([]));
        assert_eq!(p["verdict"]["level"], "critical");
        assert_eq!(p["seq"], 1);

        // Still critical, same findings: nothing more is said.
        let silence = timeout(Duration::from_millis(400), reader.next_line()).await;
        assert!(
            silence.is_err(),
            "no notification while nothing changes: {silence:?}"
        );
        assert!(
            calls.load(Ordering::SeqCst) >= 4,
            "it kept polling meanwhile"
        );

        drop(client_w);
        timeout(Duration::from_secs(2), task)
            .await
            .expect("server exits when stdin closes")
            .unwrap()
            .unwrap();
    }

    #[test]
    fn a_transition_is_a_level_change_or_a_finding_added_or_cleared() {
        use open_story_mcp::tools::ops::health_transition;
        let ok = json!({"level": "ok", "findings": []});
        let warn_a =
            json!({"level": "warn", "findings": [{"id": "a", "level": "warn", "text": "a"}]});
        let warn_ab = json!({"level": "warn", "findings": [
            {"id": "a", "level": "warn", "text": "a"}, {"id": "b", "level": "warn", "text": "b"}]});
        assert!(
            health_transition(&ok, &ok).is_none(),
            "same verdict, no transition"
        );
        let t = health_transition(&ok, &warn_a).unwrap();
        assert_eq!(t["from"], "ok");
        assert_eq!(t["to"], "warn");
        assert_eq!(t["added"], json!(["a"]));
        let t = health_transition(&warn_a, &warn_ab).unwrap();
        assert_eq!(t["from"], "warn");
        assert_eq!(
            t["to"], "warn",
            "same level, a finding added is still a transition"
        );
        assert_eq!(t["added"], json!(["b"]));
        let t = health_transition(&warn_ab, &ok).unwrap();
        assert_eq!(t["cleared"], json!(["a", "b"]));
        assert_eq!(t["added"], json!([]));
    }
}

/// M-08: the MCP may publish only under `ops.proposal.>` and `ui.>`. The
/// spec reads the crate's own source, so a new publish site cannot slip a
/// subject past it.
mod when_mcp_publishes {
    use std::path::Path;

    const ALLOWED: [&str; 2] = ["ops.proposal.", "ui."];

    /// Every `.publish…(` call: (file, line, first argument text).
    fn publish_sites(src: &str, file: &str) -> Vec<(String, usize, String)> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(pos) = src[i..].find(".publish") {
            let start = i + pos;
            let rest = &src[start..];
            let after = &rest[".publish".len()..];
            let after = after
                .strip_prefix("_bytes")
                .or_else(|| after.strip_prefix("_one"))
                .unwrap_or(after);
            if let Some(body) = after.strip_prefix('(') {
                let mut depth = 0i32;
                let mut end = body.len();
                for (j, c) in body.char_indices() {
                    match c {
                        '(' | '[' | '{' => depth += 1,
                        ')' | ']' | '}' if depth == 0 => {
                            end = j;
                            break;
                        }
                        ')' | ']' | '}' => depth -= 1,
                        ',' if depth == 0 => {
                            end = j;
                            break;
                        }
                        _ => {}
                    }
                }
                let line = src[..start].matches('\n').count() + 1;
                out.push((file.to_string(), line, body[..end].trim().to_string()));
            }
            i = start + ".publish".len();
        }
        out
    }

    /// The string literals inside an argument: what the subject starts with.
    fn literals(arg: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = arg;
        while let Some(a) = rest.find('"') {
            let tail = &rest[a + 1..];
            let Some(b) = tail.find('"') else { break };
            out.push(tail[..b].to_string());
            rest = &tail[b + 1..];
        }
        out
    }

    fn violations(sites: &[(String, usize, String)]) -> Vec<String> {
        sites
            .iter()
            .filter_map(|(file, line, arg)| {
                let lits = literals(arg);
                let ok = !lits.is_empty() && lits.iter().all(|l| ALLOWED.iter().any(|p| l.starts_with(p)));
                (!ok).then(|| format!("{file}:{line}: publish subject `{arg}` must be a literal under {ALLOWED:?}"))
            })
            .collect()
    }

    fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn it_only_touches_authored_subjects() {
        // The scanner itself, on fixtures: an observed subject is a violation,
        // an authored one is not, a variable with no literal is a violation.
        let fixture = r#"
            bus.publish("events.h.p.s.main", &batch).await;
            bus.publish(&format!("ops.proposal.{hand}"), &b).await;
            bus.publish_bytes("ui.control.x", &bytes).await;
            bus.publish(subject, &batch).await;
        "#;
        let sites = publish_sites(fixture, "fixture.rs");
        assert_eq!(sites.len(), 4, "{sites:?}");
        let v = violations(&sites);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].contains("events.h.p.s.main"));
        assert!(v[1].contains("`subject`"));

        // The crate's own source.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        let mut sites = Vec::new();
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            let rel = f.strip_prefix(&root).unwrap().display().to_string();
            sites.extend(publish_sites(&src, &rel));
        }
        let v = violations(&sites);
        assert!(
            v.is_empty(),
            "MCP publishes outside its lane:\n{}",
            v.join("\n")
        );
    }
}

/// M-06 / M-07 (MCP half): a tier-1 hand publishes `ops.proposal.<hand>`
/// with author, evidence, and an idempotency key, then calls the node's
/// endpoint; it refuses up front while the node is not serving.
mod when_node_reproject_is_called {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use axum::extract::{Path as AxPath, State};
    use axum::routing::post;
    use open_story_bus::IngestBatch;
    use open_story_mcp::subscription::{Subscribe, Subscription};
    use std::sync::{Arc, Mutex};

    /// The loopback plus a record of every proposal the MCP publishes.
    #[derive(Clone, Default)]
    struct Proposing {
        inner: common::LoopbackSubscriber,
        proposals: Arc<Mutex<Vec<(String, IngestBatch)>>>,
    }
    impl Proposing {
        fn proposals(&self) -> Vec<(String, IngestBatch)> {
            self.proposals.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl Subscribe for Proposing {
        async fn subscribe(&self, session_id: &str) -> Result<Subscription> {
            self.inner.subscribe(session_id).await
        }
        async fn publish_proposal(&self, hand: &str, batch: &IngestBatch) -> Result<()> {
            self.proposals
                .lock()
                .unwrap()
                .push((format!("ops.proposal.{hand}"), batch.clone()));
            Ok(())
        }
    }

    /// What the mock endpoint saw: the body, and whether the proposal was
    /// already on the bus when the call arrived.
    #[derive(Clone)]
    struct Seen {
        calls: Arc<Mutex<Vec<(String, Value, usize)>>>,
        subscriber: Proposing,
    }

    fn router(seen: Seen, phase: &'static str) -> Router {
        let mut health = health_body();
        health["boot"]["phase"] = json!(phase);
        health["boot"]["replay"] = json!({"done": 2, "total": 9, "elapsed_ms": 5});
        Router::new()
            .route(
                "/api/health",
                get(move || {
                    let health = health.clone();
                    async move { Json(health) }
                }),
            )
            .route(
                "/api/ops/{hand}",
                post(
                    |State(seen): State<Seen>,
                     AxPath(hand): AxPath<String>,
                     Json(body): Json<Value>| async move {
                        let proposals_now = seen.subscriber.proposals().len();
                        seen.calls.lock().unwrap().push((
                            hand.clone(),
                            body.clone(),
                            proposals_now,
                        ));
                        Json(json!({
                            "hand": hand,
                            "idempotency_key": body["idempotency_key"],
                            "replayed": false,
                            "result": {"sessions_reprojected": 1, "events_applied": 7},
                            "command_subject": format!("ops.command.{}", hand),
                        }))
                    },
                ),
            )
            .with_state(seen)
    }

    async fn server_with(
        phase: &'static str,
    ) -> (
        open_story_mcp::server::Server<Proposing>,
        Seen,
        tempfile::TempDir,
    ) {
        let subscriber = Proposing::default();
        let seen = Seen {
            calls: Arc::new(Mutex::new(Vec::new())),
            subscriber: subscriber.clone(),
        };
        let base = spawn_mock(router(seen.clone(), phase)).await;
        let (store, plan_store, dir) = common::make_test_store();
        let server =
            open_story_mcp::server::Server::new(subscriber, store, plan_store).with_api_base(base);
        (server, seen, dir)
    }

    #[tokio::test]
    async fn it_publishes_proposal_then_command() {
        let (server, seen, _dir) = server_with("serving").await;
        let resp = call_tool(
            server,
            "node_reproject",
            json!({"session_id": "s-1", "evidence": ["projections_stale"]}),
        )
        .await;
        let v = unwrap_tool_result(&resp).expect("node_reproject succeeds");
        assert_eq!(v["proposal"]["subject"], "ops.proposal.reproject", "{v}");
        let key = v["proposal"]["idempotency_key"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!key.is_empty());
        assert_eq!(v["result"]["sessions_reprojected"], 1);
        assert_eq!(v["command_subject"], "ops.command.reproject");

        let proposals = seen.subscriber.proposals();
        assert_eq!(proposals.len(), 1, "{proposals:?}");
        let (subject, batch) = &proposals[0];
        assert_eq!(subject, "ops.proposal.reproject");
        let ce = &batch.events[0];
        assert_eq!(ce.subtype.as_deref(), Some("ops.proposal.reproject"));
        assert_eq!(ce.data.raw["author"], "mcp");
        assert_eq!(ce.data.raw["evidence"], json!(["projections_stale"]));
        assert_eq!(ce.data.raw["idempotency_key"], key);
        assert_eq!(ce.data.raw["args"]["session_id"], "s-1");

        let calls = seen.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "{calls:?}");
        let (hand, body, proposals_when_called) = &calls[0];
        assert_eq!(hand, "reproject");
        assert_eq!(
            proposals_when_called, &1,
            "the proposal was on the bus before the act"
        );
        assert_eq!(
            body["idempotency_key"], key,
            "the same key reaches the node"
        );
        assert_eq!(body["session_id"], "s-1");
        assert_eq!(body["author"], "mcp");
        assert_eq!(body["evidence"], json!(["projections_stale"]));
    }

    #[tokio::test]
    async fn it_keeps_a_given_key() {
        let (server, seen, _dir) = server_with("serving").await;
        let resp = call_tool(
            server,
            "node_prune",
            json!({"older_than_days": 30, "idempotency_key": "mine-1"}),
        )
        .await;
        let v = unwrap_tool_result(&resp).expect("node_prune succeeds");
        assert_eq!(v["proposal"]["idempotency_key"], "mine-1", "{v}");
        let calls = seen.calls.lock().unwrap().clone();
        assert_eq!(calls[0].0, "prune");
        assert_eq!(calls[0].1["older_than_days"], 30);
    }

    #[tokio::test]
    async fn every_tier_one_hand_is_registered_under_propose() {
        for hand in [
            "node_reproject",
            "node_verify",
            "node_catch_up",
            "node_prune",
        ] {
            let def = open_story_mcp::tools::TOOLS
                .iter()
                .find(|t| t.name == hand)
                .unwrap_or_else(|| panic!("{hand} registered"));
            assert!(
                def.description.contains("MOTION: propose"),
                "{hand}: {}",
                def.description
            );
            assert!(
                def.description.contains("ops.proposal"),
                "{hand} says it proposes first"
            );
        }
    }

    mod when_replaying {
        use super::*;

        #[tokio::test]
        async fn tier_one_hands_refuse() {
            for (hand, args) in [
                ("node_reproject", json!({"session_id": "s"})),
                ("node_verify", json!({"session_id": "s"})),
                ("node_catch_up", json!({})),
                ("node_prune", json!({"older_than_days": 30})),
            ] {
                let (server, seen, _dir) = server_with("replaying").await;
                let resp = call_tool(server, hand, args).await;
                let err = unwrap_tool_result(&resp).expect_err("refused while replaying");
                assert!(
                    err.contains("replaying") && err.contains("serving"),
                    "{hand}: {err}"
                );
                assert!(err.contains("2 of 9"), "names the progress: {err}");
                assert!(
                    seen.subscriber.proposals().is_empty(),
                    "{hand}: no proposal for a refused act"
                );
                assert!(
                    seen.calls.lock().unwrap().is_empty(),
                    "{hand}: the node was not called"
                );
            }
        }
    }
}

/// C-03: `consistency_report {}` (tier 0) reads `/api/consistency` and
/// returns the report in the verdict's shape, findings ranked worst first.
mod when_consistency_report_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_findings_in_the_verdict_shape() {
        let (server, _dir) = server_against_mock().await;
        let resp = call_tool(server, "consistency_report", json!({})).await;
        let v = unwrap_tool_result(&resp).expect("consistency_report succeeds");
        assert_eq!(v["level"], "warn", "{v}");
        assert_eq!(v["host"], "node-a");
        let f = &v["findings"][0];
        assert_eq!(f["id"], "diverged:node-b");
        assert_eq!(f["level"], "warn");
        assert!(f["text"].as_str().unwrap().contains("1 project"), "{f}");
        assert_eq!(v["peers"][0]["host"], "node-b");
        assert_eq!(v["peers"][0]["differing_projects"], 1);
    }

    #[test]
    fn it_is_registered_under_diagnose_and_names_its_finding_ids() {
        let def = open_story_mcp::tools::TOOLS
            .iter()
            .find(|t| t.name == "consistency_report")
            .expect("consistency_report registered");
        assert!(
            def.description.contains("MOTION: diagnose"),
            "{}",
            def.description
        );
        for id in [
            "diverged:",
            "behind:",
            "lag:",
            "unverified",
            "stale_snapshot:",
        ] {
            assert!(
                def.description.contains(id),
                "names {id}: {}",
                def.description
            );
        }
        let schema = (def.input_schema)();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], json!({}), "takes nothing: {schema}");
    }
}
