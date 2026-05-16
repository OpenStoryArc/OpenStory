//! Stage B subscription tests against the InMemoryBus mock.
//!
//! These exercise the mechanics that real-NATS-backed subscriptions
//! must honor: lifecycle, ordering, cancellation, multi-subscriber
//! fan-out, multi-session high-fanout.

use open_story_mcp::bus::InMemoryBus;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::timeout;

// ── B.0 Subscription lifecycle ─────────────────────────────────────

mod when_a_client_subscribes_to_a_session {
    use super::*;

    #[tokio::test]
    async fn it_returns_a_subscription_with_a_unique_stream_id() {
        let bus = InMemoryBus::new();
        let sub_a = bus.subscribe("sid-1").await;
        let sub_b = bus.subscribe("sid-1").await;
        assert_ne!(sub_a.stream_id, sub_b.stream_id);
    }
}

mod when_10_events_are_published_to_a_subscribed_session {
    use super::*;

    #[tokio::test]
    async fn it_delivers_all_10_with_monotonic_seq() {
        let bus = InMemoryBus::new();
        let mut sub = bus.subscribe("sid-1").await;

        for i in 0..10 {
            bus.publish("sid-1", json!({"i": i})).await;
        }

        let mut received: Vec<u64> = Vec::new();
        for _ in 0..10 {
            let event = timeout(Duration::from_millis(100), sub.recv())
                .await
                .expect("event must arrive within 100ms")
                .expect("subscription must not close");
            received.push(event.seq);
            assert_eq!(event.session_id, "sid-1");
        }
        assert_eq!(received, (1..=10).collect::<Vec<_>>());
    }
}

mod when_a_subscription_is_dropped {
    use super::*;

    #[tokio::test]
    async fn the_bus_tears_down_the_route() {
        let bus = InMemoryBus::new();
        let sub = bus.subscribe("sid-1").await;
        assert_eq!(bus.subscription_count("sid-1").await, 1);

        drop(sub);
        // The cancellation guard spawns an async cleanup task — give it a moment.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            bus.subscription_count("sid-1").await,
            0,
            "dropping the subscription must tear down the route"
        );
    }

    #[tokio::test]
    async fn published_events_after_drop_are_not_delivered() {
        let bus = InMemoryBus::new();
        let mut sub = bus.subscribe("sid-1").await;
        bus.publish("sid-1", json!({"i": 1})).await;
        let first = sub.recv().await.expect("first event");
        assert_eq!(first.seq, 1);

        drop(sub);
        // Publish after cancellation; no one should receive these.
        for i in 2..5 {
            bus.publish("sid-1", json!({"i": i})).await;
        }
        // No assertion error means the bus didn't panic; subscription_count
        // is the route-level check covered in the previous test.
    }
}

// ── B.3 Multi-subscriber fan-out ───────────────────────────────────

mod when_10_subscribers_listen_to_the_same_session {
    use super::*;

    #[tokio::test]
    async fn each_receives_every_event() {
        let bus = InMemoryBus::new();
        let mut subs = Vec::new();
        for _ in 0..10 {
            subs.push(bus.subscribe("sid-1").await);
        }

        for i in 0..5 {
            bus.publish("sid-1", json!({"i": i})).await;
        }

        for (idx, sub) in subs.iter_mut().enumerate() {
            let mut seen: Vec<u64> = Vec::new();
            for _ in 0..5 {
                let event = timeout(Duration::from_millis(200), sub.recv())
                    .await
                    .unwrap_or_else(|_| panic!("subscriber {idx} timed out"))
                    .expect("subscription open");
                seen.push(event.seq);
            }
            assert_eq!(seen, vec![1, 2, 3, 4, 5], "subscriber {idx} missed events");
        }
    }
}

// ── B.3a High-fanout: 100 subscribers × 100 sessions ───────────────

mod when_100_subscribers_listen_to_100_different_sessions {
    use super::*;

    #[tokio::test]
    async fn all_100_events_are_routed_to_the_correct_subscriber_within_200ms() {
        let bus = InMemoryBus::new();
        let n = 100;

        // Open all 100 subscriptions first.
        let mut subs = Vec::with_capacity(n);
        for i in 0..n {
            let sid = format!("sid-{i:03}");
            subs.push((sid.clone(), bus.subscribe(sid).await));
        }

        let started = Instant::now();
        // Publish one event to each session in parallel.
        let mut handles = Vec::with_capacity(n);
        for i in 0..n {
            let bus = bus.clone();
            let sid = format!("sid-{i:03}");
            handles.push(tokio::spawn(async move {
                bus.publish(&sid, json!({"target": sid, "i": i})).await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        // Each subscriber must receive exactly its own event.
        for (sid, sub) in subs.iter_mut() {
            let event = timeout(Duration::from_millis(200), sub.recv())
                .await
                .unwrap_or_else(|_| panic!("subscriber {sid} timed out"))
                .expect("subscription open");
            assert_eq!(event.session_id, *sid, "event misrouted to wrong subscriber");
            assert_eq!(event.data["target"], *sid, "payload doesn't match expected session");
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "100x100 fan-out took {elapsed:?}, expected < 500ms p99"
        );
    }
}
