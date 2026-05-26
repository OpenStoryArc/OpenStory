//! Streaming notifications over stdio against the LoopbackSubscriber.
//!
//! These tests exercise the end-to-end loop that powers
//! `subscribe_session`: tool call returns immediately with a
//! stream_id, then notifications/openstory/stream lines deliver
//! events as they arrive on the bus. The "bus" here is a
//! `LoopbackSubscriber` that pushes test-constructed IngestBatches
//! through the same `pump_subscription` production uses.

mod common;

use common::{batch_with_raw, LoopbackSubscriber};
use open_story_bus::IngestBatch;
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, CodexPayload, EventData};
use open_story_mcp::stdio;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::time::timeout;

async fn read_next_response<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut Lines<BufReader<R>>,
) -> Value {
    let line = timeout(Duration::from_millis(500), reader.next_line())
        .await
        .expect("response should arrive within 500ms")
        .expect("readline must not error")
        .expect("stream should not be closed");
    serde_json::from_str(&line).expect("server output must be valid JSON")
}

fn extract_stream_id(response: &Value) -> String {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("response content[0].text must be a string");
    let parsed: Value = serde_json::from_str(text).expect("text must contain JSON");
    parsed["stream_id"]
        .as_str()
        .expect("stream_id must be a string")
        .to_string()
}

mod when_a_client_calls_subscribe_session_then_events_are_published {
    use super::*;

    #[tokio::test]
    async fn the_client_receives_the_initial_ack_and_then_per_event_notifications() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);

        let (store, plan_store, _tmp) = common::make_test_store();
        let test_server =
            open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, test_server)
                .await
                .unwrap();
        });

        // 1. tools/call subscribe_session
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "subscribe_session", "arguments": { "session_id": "sid-1" } }
        });
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        client_write.write_all(line.as_bytes()).await.unwrap();

        let mut reader = BufReader::new(client_read).lines();
        let ack = read_next_response(&mut reader).await;
        assert_eq!(ack["id"], 1);
        let stream_id = extract_stream_id(&ack);
        assert!(
            !stream_id.is_empty(),
            "stream_id must be present in the ack"
        );

        // 2. Publish 3 batches through the loopback.
        for i in 0..3 {
            let batch = batch_with_raw("sid-1", json!({"i": i, "msg": format!("event-{i}")}));
            subscriber.publish("sid-1", batch).await;
        }

        // 3. Three notification lines should arrive tagged with stream_id.
        let mut seqs = Vec::new();
        for _ in 0..3 {
            let notif = read_next_response(&mut reader).await;
            assert_eq!(notif["jsonrpc"], "2.0");
            assert_eq!(notif["method"], "notifications/openstory/stream");
            assert!(notif["id"].is_null(), "notifications have no id");
            assert_eq!(notif["params"]["stream_id"], stream_id);
            assert_eq!(notif["params"]["session_id"], "sid-1");
            seqs.push(notif["params"]["seq"].as_u64().unwrap());
        }
        assert_eq!(seqs, vec![1, 2, 3]);

        drop(client_write);
        timeout(Duration::from_millis(500), server)
            .await
            .expect("server should exit cleanly")
            .unwrap();
    }
}

mod when_the_client_sends_notifications_cancelled_with_the_request_id {
    use super::*;

    #[tokio::test]
    async fn the_stream_stops_and_post_cancel_publishes_are_not_delivered() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);

        let (store, plan_store, _tmp) = common::make_test_store();
        let test_server =
            open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, test_server)
                .await
                .unwrap();
        });

        // Subscribe.
        let subscribe = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "subscribe_session", "arguments": { "session_id": "sid-x" } }
        });
        let mut line = serde_json::to_string(&subscribe).unwrap();
        line.push('\n');
        client_write.write_all(line.as_bytes()).await.unwrap();

        let mut reader = BufReader::new(client_read).lines();
        let _ack = read_next_response(&mut reader).await;

        // Publish 2 events — both should arrive.
        for i in 0..2 {
            subscriber
                .publish("sid-x", batch_with_raw("sid-x", json!({"i": i})))
                .await;
        }
        for _ in 0..2 {
            let n = read_next_response(&mut reader).await;
            assert_eq!(n["method"], "notifications/openstory/stream");
        }

        // Cancel.
        let cancel = json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 7 }
        });
        let mut line = serde_json::to_string(&cancel).unwrap();
        line.push('\n');
        client_write.write_all(line.as_bytes()).await.unwrap();

        // Give the cancellation a moment to land, then publish more.
        tokio::time::sleep(Duration::from_millis(50)).await;
        for i in 2..5 {
            subscriber
                .publish("sid-x", batch_with_raw("sid-x", json!({"i": i})))
                .await;
        }

        // Now any new notification would be a violation — assert nothing comes within 200ms.
        let nothing = timeout(Duration::from_millis(200), reader.next_line()).await;
        match nothing {
            Err(_) => { /* timeout — correct, no more events */ }
            Ok(Ok(None)) => { /* stream closed — also correct */ }
            Ok(Ok(Some(unexpected))) => {
                panic!("got unexpected notification after cancel: {unexpected}")
            }
            Ok(Err(e)) => panic!("read error: {e}"),
        }

        drop(client_write);
        timeout(Duration::from_millis(500), server)
            .await
            .expect("server should exit")
            .unwrap();
    }
}

fn codex_tool_call_batch(session_id: &str) -> IngestBatch {
    let mut payload = CodexPayload::new();
    payload.item_type = Some("function_call".to_string());
    payload.tool = Some("exec_command".to_string());
    payload.call_id = Some("call_pwd".to_string());
    payload.args = Some(json!({"cmd": "pwd"}));

    let event = CloudEvent::new(
        format!("codex://session/{session_id}"),
        "io.arc.event".to_string(),
        EventData::with_payload(
            json!({
                "timestamp": "2026-05-23T12:12:00.552Z",
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"pwd\"}",
                    "call_id": "call_pwd"
                }
            }),
            1,
            session_id.to_string(),
            AgentPayload::Codex(payload),
        ),
        Some("message.assistant.tool_use".to_string()),
        Some("codex-tool-call-1".to_string()),
        Some("2026-05-23T12:12:00.552Z".to_string()),
        None,
        None,
        Some("codex".to_string()),
    );

    IngestBatch {
        session_id: session_id.to_string(),
        project_id: "codex-project".to_string(),
        events: vec![event],
    }
}

mod when_a_codex_batch_is_published_to_a_session_subscription {
    use super::*;

    #[tokio::test]
    async fn the_stream_notification_preserves_agent_subtype_and_payload() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);

        let (store, plan_store, _tmp) = common::make_test_store();
        let test_server =
            open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, test_server)
                .await
                .unwrap();
        });

        let request = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": { "name": "subscribe_session", "arguments": { "session_id": "codex-thread-1" } }
        });
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        client_write.write_all(line.as_bytes()).await.unwrap();

        let mut reader = BufReader::new(client_read).lines();
        let ack = read_next_response(&mut reader).await;
        assert_eq!(ack["id"], 9);
        let stream_id = extract_stream_id(&ack);

        subscriber
            .publish("codex-thread-1", codex_tool_call_batch("codex-thread-1"))
            .await;

        let notif = read_next_response(&mut reader).await;
        assert_eq!(notif["method"], "notifications/openstory/stream");
        assert_eq!(notif["params"]["stream_id"], stream_id);
        assert_eq!(notif["params"]["session_id"], "codex-thread-1");

        let event = &notif["params"]["data"]["events"][0];
        assert_eq!(event["agent"], "codex");
        assert_eq!(event["subtype"], "message.assistant.tool_use");
        assert_eq!(event["data"]["agent_payload"]["meta"]["agent"], "codex");
        assert_eq!(event["data"]["agent_payload"]["tool"], "exec_command");
        assert_eq!(event["data"]["agent_payload"]["call_id"], "call_pwd");
        assert_eq!(event["data"]["raw"]["type"], "response_item");

        drop(client_write);
        timeout(Duration::from_millis(500), server)
            .await
            .expect("server should exit cleanly")
            .unwrap();
    }
}
