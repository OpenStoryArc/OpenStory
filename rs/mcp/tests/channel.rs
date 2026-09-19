//! Channel mode (memory hands, E-12): Claude Code does not deliver custom
//! MCP notifications, but it registers a listener for servers that declare
//! the research-preview `claude/channel` capability and emit
//! `notifications/claude/channel { content, meta }`. In channel mode the
//! MCP auto-subscribes to story arcs on connect and pushes each closed node
//! as a channel event, so the stream lands in the session with no tool call.

mod common;

use common::{make_test_store, LoopbackSubscriber};
use open_story_mcp::server::{ChannelConfig, Server};
use open_story_mcp::stdio;
use open_story_patterns::PatternEvent;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

fn arc(sid: &str, handle: &str) -> PatternEvent {
    PatternEvent {
        pattern_type: "story.arc".into(),
        session_id: sid.into(),
        event_ids: vec!["e1".into()],
        started_at: "2026-01-01T09:00:00Z".into(),
        ended_at: "2026-01-01T09:05:00Z".into(),
        summary: "golden prompt 0".into(),
        metadata: json!({ "handle": handle, "ambiguous_seams": [2], "exchanges": ["ex00000000000001"] }),
    }
}

async fn read_line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut tokio::io::Lines<BufReader<R>>,
) -> Value {
    let line = timeout(Duration::from_millis(800), reader.next_line())
        .await
        .expect("line in time")
        .unwrap()
        .expect("a line");
    serde_json::from_str(&line).unwrap()
}

fn server_with(
    channel: Option<ChannelConfig>,
) -> (
    Server<LoopbackSubscriber>,
    LoopbackSubscriber,
    tempfile::TempDir,
) {
    let subscriber = LoopbackSubscriber::new();
    let (store, plan_store, tmp) = make_test_store();
    let mut server = Server::new(subscriber.clone(), store, plan_store);
    if let Some(c) = channel {
        server = server.with_channel(c);
    }
    (server, subscriber, tmp)
}

mod when_channel_mode_is_on {
    use super::*;

    #[tokio::test]
    async fn initialize_declares_the_capability_and_instructs() {
        let (server, _sub, _tmp) = server_with(Some(ChannelConfig { session_id: None }));
        let (mut cw, sr) = tokio::io::duplex(1 << 16);
        let (sw, cr) = tokio::io::duplex(1 << 16);
        let task = tokio::spawn(async move { stdio::run(sr, sw, server).await.unwrap() });
        cw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(cr).lines();
        let init = read_line(&mut reader).await;
        assert_eq!(
            init["result"]["capabilities"]["experimental"]["claude/channel"],
            json!({}),
            "{init}"
        );
        let instructions = init["result"]["instructions"].as_str().unwrap();
        assert!(
            instructions.contains("<channel"),
            "instructions tell the host what arrives: {instructions}"
        );
        assert!(instructions.contains("prompts"), "and what to do with it");
        drop(cw);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}

mod when_a_story_arc_closes_in_channel_mode {
    use super::*;

    #[tokio::test]
    async fn it_arrives_as_a_channel_notification() {
        let (server, subscriber, _tmp) = server_with(Some(ChannelConfig {
            session_id: Some("sid-a".into()),
        }));
        let (mut cw, sr) = tokio::io::duplex(1 << 16);
        let (sw, cr) = tokio::io::duplex(1 << 16);
        let task = tokio::spawn(async move { stdio::run(sr, sw, server).await.unwrap() });
        cw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(cr).lines();
        let _init = read_line(&mut reader).await;
        // Give the auto-subscription a moment to register, then publish.
        tokio::time::sleep(Duration::from_millis(100)).await;
        subscriber
            .publish_patterns("sid-b", vec![arc("sid-b", "arc000000000000b")])
            .await;
        subscriber
            .publish_patterns("sid-a", vec![arc("sid-a", "arc000000000000a")])
            .await;

        let notif = read_line(&mut reader).await;
        assert_eq!(notif["method"], "notifications/claude/channel", "{notif}");
        assert!(notif["id"].is_null());
        let meta = &notif["params"]["meta"];
        assert_eq!(meta["kind"], "arc");
        assert_eq!(
            meta["handle"], "arc000000000000a",
            "only the configured session"
        );
        assert_eq!(meta["session_id"], "sid-a");
        assert_eq!(
            meta["needs"], "enrich,adjudicate",
            "meta values are strings"
        );
        let content: Value =
            serde_json::from_str(notif["params"]["content"].as_str().unwrap()).unwrap();
        assert_eq!(content["kind"], "arc");
        assert_eq!(
            content["prompts"][0]["name"], "narrate_arc",
            "the body carries the prompts to run"
        );
        assert!(
            timeout(Duration::from_millis(300), reader.next_line())
                .await
                .is_err(),
            "sid-b was filtered out"
        );
        drop(cw);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}

mod when_channel_mode_is_off {
    use super::*;

    #[tokio::test]
    async fn nothing_changes() {
        let (server, subscriber, _tmp) = server_with(None);
        let (mut cw, sr) = tokio::io::duplex(1 << 16);
        let (sw, cr) = tokio::io::duplex(1 << 16);
        let task = tokio::spawn(async move { stdio::run(sr, sw, server).await.unwrap() });
        cw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(cr).lines();
        let init = read_line(&mut reader).await;
        assert!(
            init["result"]["capabilities"].get("experimental").is_none(),
            "{init}"
        );
        subscriber
            .publish_patterns("sid-a", vec![arc("sid-a", "arc000000000000a")])
            .await;
        assert!(
            timeout(Duration::from_millis(300), reader.next_line())
                .await
                .is_err(),
            "no auto-subscription"
        );
        drop(cw);
        timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
    }
}
