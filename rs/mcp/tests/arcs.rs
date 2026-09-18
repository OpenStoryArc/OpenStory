//! `subscribe_arcs` — the memory hands stream hand (requirements group C).
//!
//! Story patterns (`story.exchange`, `story.arc`) are published as JSON
//! arrays on `patterns.{project}.{session}`. The stream hand subscribes,
//! keeps only story patterns, and delivers each as an `ArcClosed` value
//! with the pattern as its skeleton and what it needs from a host.
//! The loopback subscriber stands in for NATS here; `nats_smoke` covers
//! the real bus.

mod common;

use common::{make_test_store, LoopbackSubscriber};
use open_story_mcp::stdio;
use open_story_mcp::subscription::{arc_closed, Subscribe};
use open_story_patterns::PatternEvent;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

fn pattern(kind: &str, sid: &str, handle: &str, extra: Value) -> PatternEvent {
    let mut metadata = json!({ "handle": handle, "ambiguous_seams": [] });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            metadata[k] = v.clone();
        }
    }
    PatternEvent {
        pattern_type: kind.to_string(),
        session_id: sid.to_string(),
        event_ids: vec!["e1".into()],
        started_at: "2026-01-01T09:00:00Z".into(),
        ended_at: "2026-01-01T09:05:00Z".into(),
        summary: "golden".into(),
        metadata,
    }
}

fn sentence(sid: &str) -> PatternEvent {
    let mut p = pattern("turn.sentence", sid, "", json!({}));
    p.metadata = json!({ "verb": "read" });
    p
}

mod when_subscribe_arcs_is_opened {
    use super::*;

    #[tokio::test]
    async fn it_receives_only_story_patterns() {
        let subscriber = LoopbackSubscriber::new();
        let mut sub = subscriber
            .subscribe_arcs(Some("sid-a"), None)
            .await
            .unwrap();
        subscriber
            .publish_patterns(
                "sid-a",
                vec![
                    sentence("sid-a"),
                    pattern("story.exchange", "sid-a", "ex00000000000001", json!({})),
                ],
            )
            .await;
        subscriber
            .publish_patterns("sid-a", vec![sentence("sid-a")])
            .await;
        subscriber
            .publish_patterns(
                "sid-b",
                vec![pattern("story.arc", "sid-b", "arc0000000000001", json!({}))],
            )
            .await;

        let first = timeout(Duration::from_millis(300), sub.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.session_id, "sid-a");
        assert_eq!(first.data["kind"], "exchange");
        assert_eq!(first.data["handle"], "ex00000000000001");
        assert!(
            timeout(Duration::from_millis(200), sub.recv())
                .await
                .is_err(),
            "no sentence, no other session"
        );
    }
}

mod when_three_arcs_close {
    use super::*;

    #[tokio::test]
    async fn it_emits_seq_one_two_three() {
        let subscriber = LoopbackSubscriber::new();
        let mut sub = subscriber
            .subscribe_arcs(Some("sid-a"), None)
            .await
            .unwrap();
        for i in 1..=3 {
            subscriber
                .publish_patterns(
                    "sid-a",
                    vec![pattern(
                        "story.arc",
                        "sid-a",
                        &format!("arc000000000000{i}"),
                        json!({}),
                    )],
                )
                .await;
        }
        let mut seqs = Vec::new();
        for _ in 0..3 {
            seqs.push(
                timeout(Duration::from_millis(300), sub.recv())
                    .await
                    .unwrap()
                    .unwrap()
                    .seq,
            );
        }
        assert_eq!(seqs, vec![1, 2, 3]);
    }
}

mod when_an_arc_closes {
    use super::*;

    #[test]
    fn needs_is_derived_from_the_skeleton() {
        let plain =
            serde_json::to_value(pattern("story.arc", "s", "arc0000000000001", json!({}))).unwrap();
        let ambiguous = serde_json::to_value(pattern(
            "story.arc",
            "s",
            "arc0000000000002",
            json!({ "ambiguous_seams": [2] }),
        ))
        .unwrap();
        let exchange = serde_json::to_value(pattern(
            "story.exchange",
            "s",
            "ex00000000000001",
            json!({}),
        ))
        .unwrap();
        let sentence = serde_json::to_value(sentence("s")).unwrap();

        let a = arc_closed("s", &plain).unwrap();
        assert_eq!(a["kind"], "arc");
        assert_eq!(
            a["needs"],
            json!(["enrich"]),
            "untitled arc needs enrichment"
        );
        let b = arc_closed("s", &ambiguous).unwrap();
        assert_eq!(b["needs"], json!(["enrich", "adjudicate"]));
        let c = arc_closed("s", &exchange).unwrap();
        assert_eq!(c["kind"], "exchange");
        assert_eq!(
            c["needs"],
            json!(["read"]),
            "a closed exchange feeds the streaming read"
        );
        assert_eq!(c["skeleton"]["pattern_type"], "story.exchange");
        assert!(arc_closed("s", &sentence).is_none());
    }
}

async fn read_line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut tokio::io::Lines<BufReader<R>>,
) -> Value {
    let line = timeout(Duration::from_millis(500), reader.next_line())
        .await
        .expect("line in time")
        .unwrap()
        .expect("a line");
    serde_json::from_str(&line).unwrap()
}

mod when_subscribe_arcs_tool_is_called_over_stdio {
    use super::*;

    #[tokio::test]
    async fn it_returns_started_then_notifications_and_stops_on_cancel() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);
        let (store, plan_store, _tmp) = make_test_store();
        let test_server =
            open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, test_server)
                .await
                .unwrap()
        });

        let request = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "subscribe_arcs", "arguments": { "session_id": "sid-a" } } });
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(client_read).lines();
        let ack = read_line(&mut reader).await;
        assert_eq!(ack["id"], 7);
        let text: Value =
            serde_json::from_str(ack["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["status"], "started");
        let stream_id = text["stream_id"].as_str().unwrap().to_string();

        subscriber
            .publish_patterns(
                "sid-a",
                vec![pattern("story.arc", "sid-a", "arc0000000000001", json!({}))],
            )
            .await;
        let notif = read_line(&mut reader).await;
        assert_eq!(notif["method"], "notifications/openstory/arcs");
        assert_eq!(notif["params"]["stream_id"], stream_id);
        assert_eq!(notif["params"]["seq"], 1);
        assert_eq!(notif["params"]["data"]["kind"], "arc");
        assert_eq!(notif["params"]["data"]["handle"], "arc0000000000001");

        // C-05: cancel against the original request id, then publish again.
        let cancel = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 7 } });
        client_write
            .write_all(format!("{cancel}\n").as_bytes())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        subscriber
            .publish_patterns(
                "sid-a",
                vec![pattern("story.arc", "sid-a", "arc0000000000002", json!({}))],
            )
            .await;
        assert!(
            timeout(Duration::from_millis(300), reader.next_line())
                .await
                .is_err(),
            "no notification after cancel"
        );
        assert_eq!(
            subscriber.arc_route_count("sid-a").await,
            0,
            "the subscription was dropped"
        );

        drop(client_write);
        timeout(Duration::from_millis(500), server)
            .await
            .expect("server exits")
            .unwrap();
    }
}

mod when_from_seq_is_given {
    use super::*;

    #[tokio::test]
    async fn it_replays_the_prefix_then_streams_the_tail() {
        let subscriber = LoopbackSubscriber::new();
        for i in 1..=3 {
            subscriber
                .publish_patterns(
                    "sid-a",
                    vec![pattern(
                        "story.exchange",
                        "sid-a",
                        &format!("ex0000000000000{i}"),
                        json!({}),
                    )],
                )
                .await;
        }
        // Batches 1..3 are history. Resume from batch 2.
        let mut sub = subscriber
            .subscribe_arcs(Some("sid-a"), Some(2))
            .await
            .unwrap();
        subscriber
            .publish_patterns(
                "sid-a",
                vec![pattern(
                    "story.exchange",
                    "sid-a",
                    "ex00000000000004",
                    json!({}),
                )],
            )
            .await;
        let mut got = Vec::new();
        for _ in 0..3 {
            let ev = timeout(Duration::from_millis(300), sub.recv())
                .await
                .unwrap()
                .unwrap();
            got.push((
                ev.data["batch_seq"].as_u64().unwrap(),
                ev.data["handle"].as_str().unwrap().to_string(),
            ));
        }
        assert_eq!(
            got,
            vec![
                (2, "ex00000000000002".into()),
                (3, "ex00000000000003".into()),
                (4, "ex00000000000004".into())
            ],
            "stored prefix from the cursor, then the live tail"
        );
        assert!(timeout(Duration::from_millis(200), sub.recv())
            .await
            .is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════
// E-02: the notification names the prompt to run for each need
// ═══════════════════════════════════════════════════════════════════

mod when_an_arc_closes_the_notification_names_its_prompt {
    use super::*;

    #[test]
    fn needs_map_to_prompts_with_arguments() {
        let ambiguous = serde_json::to_value(pattern(
            "story.arc",
            "s",
            "arc0000000000002",
            json!({ "ambiguous_seams": [2, 5] }),
        ))
        .unwrap();
        let a = arc_closed("s", &ambiguous).unwrap();
        assert_eq!(
            a["prompts"],
            json!([
                { "name": "narrate_arc", "arguments": { "handle": "arc0000000000002", "session_id": "s" } },
                { "name": "segment_arc", "arguments": { "handle": "arc0000000000002", "session_id": "s" } },
                { "name": "adjudicate_seam", "arguments": { "handle": "arc0000000000002", "session_id": "s", "seam": 2 } },
                { "name": "adjudicate_seam", "arguments": { "handle": "arc0000000000002", "session_id": "s", "seam": 5 } },
            ]),
            "enrich → narrate_arc + segment_arc; each seam → adjudicate_seam"
        );
        let exchange = serde_json::to_value(pattern(
            "story.exchange",
            "s",
            "ex00000000000001",
            json!({}),
        ))
        .unwrap();
        let e = arc_closed("s", &exchange).unwrap();
        assert_eq!(
            e["prompts"],
            json!([{ "name": "read_exchange", "arguments": { "handle": "ex00000000000001", "session_id": "s" } }])
        );
    }

    #[tokio::test]
    async fn the_stdio_notification_carries_them() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);
        let (store, plan_store, _tmp) = make_test_store();
        let test_server =
            open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, test_server)
                .await
                .unwrap()
        });
        let request = json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "subscribe_arcs", "arguments": { "session_id": "sid-a" } } });
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(client_read).lines();
        let _ack = read_line(&mut reader).await;
        subscriber
            .publish_patterns(
                "sid-a",
                vec![pattern("story.arc", "sid-a", "arc0000000000009", json!({}))],
            )
            .await;
        let notif = read_line(&mut reader).await;
        let prompts = notif["params"]["data"]["prompts"]
            .as_array()
            .expect("prompts on the wire");
        assert_eq!(prompts[0]["name"], "narrate_arc");
        assert_eq!(prompts[0]["arguments"]["handle"], "arc0000000000009");
        drop(client_write);
        timeout(Duration::from_millis(500), server)
            .await
            .expect("server exits")
            .unwrap();
    }
}
