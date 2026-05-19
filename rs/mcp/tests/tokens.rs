//! Token-aggregator unit tests + subscribe_tokens streaming integration.
//!
//! The unit tests exercise `TokenAggregator` directly. The integration
//! test wires `subscribe_tokens` through stdio + `LoopbackSubscriber`,
//! same pipeline production uses (modulo NatsBus's underlying transport).

mod common;

use common::{batch_with_raw, batch_with_usage, LoopbackSubscriber};
use open_story_mcp::stdio;
use open_story_mcp::tokens::TokenAggregator;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::time::timeout;

// ── unit: aggregator math ──────────────────────────────────────────

mod when_a_single_event_carries_usage {
    use super::*;

    #[test]
    fn extract_one_pulls_input_output_cache_read_and_cache_create() {
        let event = json!({
            "data": {
                "raw": {
                    "message": {
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 340,
                            "cache_read_input_tokens": 100_000,
                            "cache_creation_input_tokens": 2_500,
                        }
                    }
                }
            }
        });
        let counts = TokenAggregator::extract_one(&event);
        assert_eq!(counts.input, 12);
        assert_eq!(counts.output, 340);
        assert_eq!(counts.cache_read, 100_000);
        assert_eq!(counts.cache_create, 2_500);
        assert_eq!(counts.total(), 12 + 340 + 100_000 + 2_500);
    }

    #[test]
    fn extract_one_handles_iterations_for_output_tokens() {
        let event = json!({
            "data": { "raw": { "message": { "usage": {
                "input_tokens": 5,
                "iterations": [
                    { "output_tokens": 100 },
                    { "output_tokens": 200 },
                ]
            }}}}
        });
        let counts = TokenAggregator::extract_one(&event);
        assert_eq!(counts.output, 300);
    }

    #[test]
    fn an_event_without_usage_returns_zero() {
        let event = json!({"data": {"raw": {"type": "user"}}});
        assert!(TokenAggregator::extract_one(&event).is_zero());
    }
}

mod when_observed_repeatedly {
    use super::*;

    #[test]
    fn running_total_accumulates_across_batches() {
        let mut agg = TokenAggregator::new();
        let make_batch = |input: u64, output: u64| {
            json!({"events": [{"data": {"raw": {"message": {"usage": {
                "input_tokens": input, "output_tokens": output,
            }}}}}]})
        };
        let (delta1, after1) = agg.observe(&make_batch(10, 100)).unwrap();
        let (delta2, after2) = agg.observe(&make_batch(5, 50)).unwrap();
        assert_eq!(delta1.input, 10);
        assert_eq!(after1.input, 10);
        assert_eq!(delta2.input, 5);
        assert_eq!(after2.input, 15);
        assert_eq!(after2.output, 150);
    }

    #[test]
    fn zero_token_batches_return_none_so_callers_can_skip() {
        let mut agg = TokenAggregator::new();
        let batch = json!({"events": [{"data": {"raw": {"type": "user"}}}]});
        assert!(agg.observe(&batch).is_none());
    }
}

// ── integration: subscribe_tokens over stdio + LoopbackSubscriber ──

async fn read_line<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut Lines<BufReader<R>>,
) -> Value {
    let line = timeout(Duration::from_millis(500), reader.next_line())
        .await
        .expect("response within 500ms")
        .expect("readline")
        .expect("stream open");
    serde_json::from_str(&line).unwrap()
}

mod when_a_client_calls_subscribe_tokens {
    use super::*;

    #[tokio::test]
    async fn it_emits_a_tokens_notification_per_usage_event_with_running_total() {
        let subscriber = LoopbackSubscriber::new();
        let (mut client_w, server_r) = tokio::io::duplex(8192);
        let (server_w, client_r) = tokio::io::duplex(8192);
        let (store, plan_store, _tmp) = common::make_test_store();
        let test_server = open_story_mcp::server::Server::new(subscriber.clone(), store, plan_store);
        let server = tokio::spawn(async move {
            stdio::run(server_r, server_w, test_server).await.unwrap();
        });

        // 1. subscribe_tokens
        let req = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": { "name": "subscribe_tokens", "arguments": { "session_id": "sid-tok" } }
        });
        let mut line = serde_json::to_string(&req).unwrap();
        line.push('\n');
        client_w.write_all(line.as_bytes()).await.unwrap();

        let mut reader = BufReader::new(client_r).lines();
        let ack = read_line(&mut reader).await;
        assert_eq!(ack["id"], 11);
        assert_eq!(ack["result"]["isError"], false);

        // 2. Publish 3 batches, only 2 of which carry usage data.
        subscriber
            .publish(
                "sid-tok",
                batch_with_usage("sid-tok", json!({"input_tokens": 10, "output_tokens": 100})),
            )
            .await;
        subscriber
            .publish("sid-tok", batch_with_raw("sid-tok", json!({"type": "user"})))
            .await;
        subscriber
            .publish(
                "sid-tok",
                batch_with_usage("sid-tok", json!({"input_tokens": 5, "output_tokens": 50})),
            )
            .await;

        // 3. Expect exactly 2 token notifications, with monotonic running.
        let n1 = read_line(&mut reader).await;
        let n2 = read_line(&mut reader).await;
        for n in [&n1, &n2] {
            assert_eq!(n["jsonrpc"], "2.0");
            assert_eq!(n["method"], "notifications/openstory/tokens");
            assert!(n["id"].is_null());
            assert_eq!(n["params"]["session_id"], "sid-tok");
        }
        assert_eq!(n1["params"]["delta"]["input"], 10);
        assert_eq!(n1["params"]["delta"]["output"], 100);
        assert_eq!(n1["params"]["running"]["input"], 10);
        assert_eq!(n1["params"]["running"]["output"], 100);
        assert_eq!(n2["params"]["delta"]["input"], 5);
        assert_eq!(n2["params"]["running"]["input"], 15);
        assert_eq!(n2["params"]["running"]["output"], 150);

        // 4. No third notification (the usage-less event was filtered).
        let nothing = timeout(Duration::from_millis(200), reader.next_line()).await;
        match nothing {
            Err(_) => { /* timeout — correct */ }
            Ok(Ok(None)) => { /* closed — also fine */ }
            Ok(Ok(Some(extra))) => panic!("unexpected third notification: {extra}"),
            Ok(Err(e)) => panic!("read error: {e}"),
        }

        drop(client_w);
        timeout(Duration::from_millis(500), server).await.unwrap().unwrap();
    }
}
