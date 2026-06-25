//! Smoke test against a live NATS server with JetStream.
//!
//! Requires:
//!   - NATS running with JetStream (e.g., `just nats` or `just up`)
//!
//! Set `OPENSTORY_NATS_URL=skip` to bypass these tests in environments
//! where NATS isn't reachable. The default is `nats://localhost:4222`.
//!
//! Both publisher and subscriber go through `open_story_bus::NatsBus`
//! (JetStream-aware) — same path the OpenStory server uses in
//! production. The test is the canonical "MCP subscribes to the same
//! events the server publishes" assertion against a real bus.

mod common;

use common::batch_with_raw;
use open_story_bus::nats_bus::NatsBus as InnerNatsBus;
use open_story_bus::Bus;
use open_story_mcp::nats_bus::NatsBus;
use open_story_mcp::subscription::Subscribe;
use std::time::Duration;
use tokio::time::timeout;

fn nats_url() -> Option<String> {
    match std::env::var("OPENSTORY_NATS_URL") {
        Ok(v) if v == "skip" => None,
        Ok(v) => Some(v),
        Err(_) => Some("nats://localhost:4222".to_string()),
    }
}

async fn connect_or_skip(url: &str) -> Option<(InnerNatsBus, NatsBus)> {
    let publisher = match InnerNatsBus::connect(url).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("NATS not reachable at {url}: {e} — skipping smoke test");
            return None;
        }
    };
    if let Err(e) = publisher.ensure_streams().await {
        eprintln!("ensure_streams failed at {url}: {e} — skipping");
        return None;
    }
    let subscriber = NatsBus::connect(url).await.ok()?;
    Some((publisher, subscriber))
}

mod when_a_subscriber_listens_on_nats_and_an_event_is_published_via_jetstream {
    use super::*;

    #[tokio::test]
    async fn it_arrives_at_the_subscriber_with_matching_session_id() {
        let Some(url) = nats_url() else { return };
        let Some((publisher, subscriber)) = connect_or_skip(&url).await else {
            return;
        };

        let session_id = format!("smoke-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&session_id).await.expect("subscribe");

        // Give the NATS consumer a moment to wire up before publishing.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let batch = batch_with_raw(&session_id, serde_json::json!({"hello": "from-jetstream"}));
        let subject = format!("events.test-host.test-project.{}.main", session_id);
        publisher
            .publish(&subject, &batch)
            .await
            .expect("publish via jetstream");

        let event = timeout(Duration::from_secs(2), sub.recv())
            .await
            .expect("event must arrive within 2s")
            .expect("subscription open");
        assert_eq!(event.seq, 1);
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.data["session_id"], session_id);
        assert_eq!(event.data["project_id"], "test-project");
    }

    #[tokio::test]
    async fn the_session_wildcard_matches_subagent_subjects_too() {
        let Some(url) = nats_url() else { return };
        let Some((publisher, subscriber)) = connect_or_skip(&url).await else {
            return;
        };

        let session_id = format!("smoke-{}", uuid::Uuid::new_v4());
        let mut sub = subscriber.subscribe(&session_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let batch = batch_with_raw(&session_id, serde_json::json!({"from": "subagent"}));
        let subject = format!("events.test-host.test-project.{}.agent.aabb1122", session_id);
        publisher.publish(&subject, &batch).await.unwrap();

        let event = timeout(Duration::from_secs(2), sub.recv())
            .await
            .expect("subagent event must arrive within 2s")
            .expect("open");
        assert_eq!(event.session_id, session_id);
    }
}
