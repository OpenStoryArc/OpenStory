//! Integration tests against real NATS in a Docker container.
//!
//! Requires Docker. The container helper boots `nats:2.10` with
//! JetStream (`-js`) on a random host port; tests connect a publisher
//! via `open_story_bus::NatsBus` (the production publish path) and a
//! subscriber via `open_story_mcp::NatsBus` (the MCP receive path).
//!
//! The 100×100 fan-out test and the binary-as-child-process test are
//! the marquee assertions — they exercise the parts of the system
//! that in-process fakes can't honestly assert.

mod common;

use common::{batch_with_raw, batch_with_usage, nats_container::start_nats_container};
use open_story_bus::nats_bus::NatsBus as PublisherBus;
use open_story_bus::Bus;
use open_story_mcp::nats_bus::NatsBus as McpBus;
use open_story_mcp::subscription::Subscribe;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::time::timeout;

// Small delay to let JetStream consumer wiring settle before publishing.
const CONSUMER_WIRE_DELAY: Duration = Duration::from_millis(200);
// Reasonable upper bound for a single round-trip through the container.
const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(3);

async fn connect_pair(url: &str) -> (PublisherBus, McpBus) {
    let publisher = PublisherBus::connect(url).await.expect("publisher connect");
    publisher.ensure_streams().await.expect("ensure_streams");
    let subscriber = McpBus::connect(url).await.expect("mcp connect");
    (publisher, subscriber)
}

// ── #1: round-trip via JetStream ───────────────────────────────────

mod when_publisher_writes_via_jetstream_and_subscriber_reads_via_mcp_nats_bus {
    use super::*;

    #[tokio::test]
    async fn the_event_arrives_at_the_subscriber_in_one_round_trip() {
        let (_container, url) = start_nats_container().await;
        let (publisher, subscriber) = connect_pair(&url).await;

        let sid = format!("rt-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&sid).await.expect("subscribe");
        tokio::time::sleep(CONSUMER_WIRE_DELAY).await;

        let batch = batch_with_raw(&sid, json!({"hello": "jetstream"}));
        publisher
            .publish(&format!("events.test-host.test-p.{}.main", sid), &batch)
            .await
            .expect("publish");

        let event = timeout(ROUND_TRIP_TIMEOUT, sub.recv())
            .await
            .expect("event within round-trip budget")
            .expect("subscription open");
        assert_eq!(event.seq, 1);
        assert_eq!(event.session_id, sid);
        // The wire shape is `serde_json::to_value(&IngestBatch)`. The
        // payload's project_id comes from the IngestBatch the publisher
        // built (via batch_with_raw → "test-project"), independent of
        // the NATS subject's project token.
        assert_eq!(event.data["session_id"], sid);
        assert_eq!(event.data["project_id"], "test-project");
        assert_eq!(event.data["events"][0]["data"]["raw"]["hello"], "jetstream");
    }
}

// ── #2: subagent subject wildcard ──────────────────────────────────

mod when_a_subagent_subject_is_published {
    use super::*;

    #[tokio::test]
    async fn the_session_wildcard_picks_it_up_with_no_extra_subscription() {
        let (_container, url) = start_nats_container().await;
        let (publisher, subscriber) = connect_pair(&url).await;

        let sid = format!("sub-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&sid).await.expect("subscribe");
        tokio::time::sleep(CONSUMER_WIRE_DELAY).await;

        let batch = batch_with_raw(&sid, json!({"from": "subagent"}));
        let subject = format!("events.test-host.test-p.{}.agent.aabbccdd", sid);
        publisher.publish(&subject, &batch).await.expect("publish");

        let event = timeout(ROUND_TRIP_TIMEOUT, sub.recv())
            .await
            .expect("subagent event within budget")
            .expect("open");
        assert_eq!(event.session_id, sid);
        assert_eq!(event.data["events"][0]["data"]["raw"]["from"], "subagent");
    }
}

// ── #3: monotonic seq across multiple events ──────────────────────

mod when_multiple_events_are_published_in_order {
    use super::*;

    #[tokio::test]
    async fn the_subscriber_sees_seq_one_through_five() {
        let (_container, url) = start_nats_container().await;
        let (publisher, subscriber) = connect_pair(&url).await;

        let sid = format!("seq-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&sid).await.expect("subscribe");
        tokio::time::sleep(CONSUMER_WIRE_DELAY).await;

        for i in 0..5 {
            let batch = batch_with_raw(&sid, json!({"i": i}));
            publisher
                .publish(&format!("events.test-host.test-p.{}.main", sid), &batch)
                .await
                .expect("publish");
        }

        let mut seqs = Vec::new();
        for _ in 0..5 {
            let ev = timeout(ROUND_TRIP_TIMEOUT, sub.recv())
                .await
                .expect("event within budget")
                .expect("open");
            seqs.push(ev.seq);
        }
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }
}

// ── #4: 100 sessions × 100 subscribers, the honest fan-out ────────

mod when_one_hundred_subscribers_listen_to_one_hundred_sessions {
    use super::*;

    #[tokio::test]
    async fn each_receives_only_its_own_event_within_a_bounded_time() {
        let (_container, url) = start_nats_container().await;
        let (publisher, subscriber) = connect_pair(&url).await;

        // Open 100 subscriptions, one per session.
        let n = 100;
        let mut subs: Vec<(String, _)> = Vec::with_capacity(n);
        for i in 0..n {
            let sid = format!("fan-{:03}-{}", i, uuid::Uuid::new_v4());
            let sub = subscriber.subscribe(&sid).await.expect("subscribe");
            subs.push((sid, sub));
        }

        // Give the broker a moment to wire all 100 consumers.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Publish one event per session serially — JetStream publish is
        // fast enough that 100 sequential publishes are well under budget.
        let started = Instant::now();
        for (sid, _) in &subs {
            let subject = format!("events.test-host.test-p.{}.main", sid);
            let batch = batch_with_raw(sid, json!({"target": sid}));
            publisher.publish(&subject, &batch).await.expect("publish");
        }

        // Each subscriber must receive exactly its own event.
        for (sid, sub) in subs.iter_mut() {
            let event = timeout(Duration::from_secs(10), sub.recv())
                .await
                .unwrap_or_else(|_| panic!("subscriber for {sid} timed out"))
                .expect("subscription open");
            assert_eq!(
                event.session_id, *sid,
                "event misrouted to subscriber for {sid}"
            );
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(15),
            "100×100 fan-out via real JetStream took {elapsed:?}, expected < 15s"
        );
    }
}

// ── #5: subscribe_tokens end-to-end with cache fields ─────────────

mod when_an_assistant_event_with_cache_fields_is_published {
    use super::*;
    use open_story_mcp::tokens::TokenAggregator;

    #[tokio::test]
    async fn subscribe_tokens_extracts_input_output_cache_read_and_cache_create() {
        // This test asserts on TokenAggregator's extraction from the wire
        // shape that the actual NatsBus pump produces. It locks the
        // TokenUsage cache-field fix end-to-end: if the views-layer
        // projection ever drops cache fields again, this assertion
        // catches it through the streaming MCP receive path.

        let (_container, url) = start_nats_container().await;
        let (publisher, subscriber) = connect_pair(&url).await;

        let sid = format!("tok-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&sid).await.expect("subscribe");
        tokio::time::sleep(CONSUMER_WIRE_DELAY).await;

        let usage = json!({
            "input_tokens": 1,
            "output_tokens": 454,
            "cache_read_input_tokens": 274_190,
            "cache_creation_input_tokens": 2_326,
        });
        let batch = batch_with_usage(&sid, usage);
        publisher
            .publish(&format!("events.test-host.test-p.{}.main", sid), &batch)
            .await
            .expect("publish");

        let event = timeout(ROUND_TRIP_TIMEOUT, sub.recv())
            .await
            .expect("event within budget")
            .expect("subscription open");

        // Verify TokenAggregator extracts the cache fields from the
        // exact wire shape the bus produced.
        let mut agg = TokenAggregator::new();
        let (delta, running) = agg.observe(&event.data).expect("usage extracted");
        assert_eq!(delta.input, 1);
        assert_eq!(delta.output, 454);
        assert_eq!(delta.cache_read, 274_190);
        assert_eq!(delta.cache_create, 2_326);
        assert_eq!(running.total(), 1 + 454 + 274_190 + 2_326);
    }
}

// ── #6: the binary, spawned as a child process ─────────────────────

mod when_the_compiled_binary_is_spawned_with_nats_url {
    use super::*;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn it_completes_handshake_and_streams_notifications_for_a_subscribed_session() {
        let (_container, url) = start_nats_container().await;
        // Connect publisher early so streams exist before the binary subscribes.
        let publisher = PublisherBus::connect(&url)
            .await
            .expect("publisher connect");
        publisher.ensure_streams().await.expect("ensure_streams");

        // The binary needs a writable data dir for its SqliteStore.
        // We point it at a temp dir held alive for the duration of this test.
        let data_dir = tempfile::tempdir().expect("temp data dir");

        let binary = env!("CARGO_BIN_EXE_open-story-mcp");
        let mut child = tokio::process::Command::new(binary)
            .env("OPENSTORY_NATS_URL", &url)
            .env("OPENSTORY_DATA_DIR", data_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn open-story-mcp");

        let mut stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout).lines();

        // Drain stderr in the background so the buffer doesn't fill.
        let stderr = child.stderr.take().expect("stderr");
        tokio::spawn(async move {
            let mut r = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = r.next_line().await {}
        });

        let sid = format!("bin-{}", uuid::Uuid::new_v4());

        // 1. initialize
        let init = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "tc", "version": "0"}}
        });
        stdin
            .write_all(format!("{init}\n").as_bytes())
            .await
            .unwrap();

        let init_line = timeout(ROUND_TRIP_TIMEOUT, reader.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("init response");
        let init_resp: Value = serde_json::from_str(&init_line).unwrap();
        assert_eq!(init_resp["id"], 1);
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "open-story-mcp");

        // 2. notifications/initialized (no response)
        let nu = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        stdin.write_all(format!("{nu}\n").as_bytes()).await.unwrap();

        // 3. subscribe_session
        let sub_req = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "subscribe_session", "arguments": {"session_id": sid}}
        });
        stdin
            .write_all(format!("{sub_req}\n").as_bytes())
            .await
            .unwrap();

        let ack_line = timeout(ROUND_TRIP_TIMEOUT, reader.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("ack");
        let ack: Value = serde_json::from_str(&ack_line).unwrap();
        assert_eq!(ack["id"], 2);
        assert_eq!(ack["result"]["isError"], false);

        // Let JetStream consumer wire up before publishing.
        tokio::time::sleep(CONSUMER_WIRE_DELAY).await;

        // 4. Publish via JetStream — the binary should emit a notification.
        let batch = batch_with_raw(&sid, json!({"marker": "from-test"}));
        publisher
            .publish(&format!("events.test-host.test-p.{}.main", sid), &batch)
            .await
            .expect("publish");

        let notif_line = timeout(ROUND_TRIP_TIMEOUT, reader.next_line())
            .await
            .unwrap()
            .unwrap()
            .expect("notification");
        let notif: Value = serde_json::from_str(&notif_line).unwrap();
        assert_eq!(notif["method"], "notifications/openstory/stream");
        assert_eq!(notif["params"]["session_id"], sid);
        assert_eq!(
            notif["params"]["data"]["events"][0]["data"]["raw"]["marker"],
            "from-test"
        );

        // Cleanup: close stdin, wait for the binary to exit.
        drop(stdin);
        let _ = timeout(ROUND_TRIP_TIMEOUT, child.wait()).await;
    }
}
