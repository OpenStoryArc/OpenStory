//! TDD: Hierarchical NATS subject naming for parent-child session relationships.
//!
//! These tests describe the DESIRED behavior — events published with
//! hierarchical subjects that encode project, session, and agent identity.
//!
//! Current: events.session.{session_id}           (flat, peers)
//! Target:  events.{project}.{session}.main       (main agent)
//!          events.{project}.{session}.agent.{id}  (subagent)
//!
//! The ".main" suffix on main agent subjects ensures events.{project}.{session}.>
//! matches BOTH main and subagent events (NATS ">" requires at least one token).
//!
//! Run with: cargo test -p open-story --test test_subject_hierarchy

mod helpers;

use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use serde_json::json;
use testcontainers::{GenericImage, ImageExt};
use testcontainers::runners::AsyncRunner;

/// Create a minimal CloudEvent for testing.
fn test_event(session_id: &str, subtype: &str) -> CloudEvent {
    let data = EventData::new(
        json!({"test": true}),
        1,
        session_id.to_string(),
    );
    CloudEvent::new(
        format!("arc://test/{session_id}"),
        "io.arc.event".to_string(),
        data,
        Some(subtype.to_string()),
        None,
        None,
        None,
        None,
        Some("claude-code".to_string()),
    )
}

/// Start a NATS container with JetStream and return a connected NatsBus.
async fn start_nats() -> (NatsBus, testcontainers::ContainerAsync<GenericImage>) {
    let container = GenericImage::new("nats", "2-alpine")
        .with_cmd(vec!["--jetstream"])
        .start()
        .await
        .expect("start NATS container");

    let port = container.get_host_port_ipv4(4222).await.expect("get port");
    let nats_url = format!("nats://localhost:{port}");

    // Retry connection (container may need a moment)
    let mut bus = None;
    for _ in 0..10 {
        match NatsBus::connect(&nats_url).await {
            Ok(b) => { bus = Some(b); break; }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
    let bus = bus.expect("connect to NATS");
    bus.ensure_streams().await.expect("create streams");

    (bus, container)
}

// ═══════════════════════════════════════════════════════════════════
// describe "subject composition" (pure functions, no containers)
// ═══════════════════════════════════════════════════════════════════

/// Compose the NATS subject for a main agent event.
fn subject_for_main(project_id: &str, session_id: &str) -> String {
    format!("events.{project_id}.{session_id}.main")
}

/// Compose the NATS subject for a subagent event.
fn subject_for_agent(project_id: &str, session_id: &str, agent_id: &str) -> String {
    format!("events.{project_id}.{session_id}.agent.{agent_id}")
}

#[test]
fn main_agent_subject_has_project_session_and_main_suffix() {
    assert_eq!(
        subject_for_main("openstory", "06907d46"),
        "events.openstory.06907d46.main"
    );
}

#[test]
fn subagent_subject_nests_under_parent_session() {
    assert_eq!(
        subject_for_agent("openstory", "06907d46", "a6dcf911"),
        "events.openstory.06907d46.agent.a6dcf911"
    );
}

#[test]
fn session_wildcard_pattern_matches_both() {
    // events.openstory.06907d46.> matches:
    //   events.openstory.06907d46.main           ✓ (main agent)
    //   events.openstory.06907d46.agent.a6dcf911 ✓ (subagent)
    // Because ">" requires at least one more token, and both have it.
    let main = subject_for_main("openstory", "06907d46");
    let agent = subject_for_agent("openstory", "06907d46", "a6dcf911");
    let prefix = "events.openstory.06907d46.";

    assert!(main.starts_with(prefix), "main subject starts with session prefix");
    assert!(agent.starts_with(prefix), "agent subject starts with session prefix");
}

// ═══════════════════════════════════════════════════════════════════
// describe "NATS integration" (testcontainers)
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn publish_and_subscribe_with_hierarchical_subjects() {
    let (bus, _container) = start_nats().await;

    // Subscribe to all openstory events
    let mut sub = bus.subscribe("events.openstory.>").await.expect("subscribe");

    // Publish main agent event
    let main_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.user.prompt")],
    };
    bus.publish("events.openstory.06907d46.main", &main_batch)
        .await.expect("publish main");

    // Publish subagent event
    let agent_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.assistant.tool_use")],
    };
    bus.publish("events.openstory.06907d46.agent.a6dcf911", &agent_batch)
        .await.expect("publish agent");

    // Receive both
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout first").expect("receive first");

    let second = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout second").expect("receive second");

    assert_eq!(first.session_id, "06907d46");
    assert_eq!(second.session_id, "06907d46");
}

#[tokio::test]
async fn session_wildcard_receives_main_and_all_subagents() {
    let (bus, _container) = start_nats().await;

    // Subscribe to one session's events (main + agents)
    let mut sub = bus.subscribe("events.openstory.06907d46.>")
        .await.expect("subscribe");

    // Publish main
    let main_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.user.prompt")],
    };
    bus.publish("events.openstory.06907d46.main", &main_batch)
        .await.expect("publish main");

    // Publish two different subagents
    for agent_id in &["a6dcf911", "afdf1bb2"] {
        let batch = IngestBatch {
            session_id: "06907d46".to_string(),
            project_id: "openstory".to_string(),
            events: vec![test_event("06907d46", "message.assistant.text")],
        };
        bus.publish(&subject_for_agent("openstory", "06907d46", agent_id), &batch)
            .await.expect("publish agent");
    }

    // Should receive all 3
    let mut received = Vec::new();
    for _ in 0..3 {
        let batch = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            sub.receiver.recv(),
        ).await.expect("timeout").expect("receive");
        received.push(batch);
    }

    assert_eq!(received.len(), 3, "should receive main + 2 agents");
    assert!(received.iter().all(|b| b.session_id == "06907d46"), "all from same session");
}

#[tokio::test]
async fn agent_only_subscription_excludes_main() {
    let (bus, _container) = start_nats().await;

    // Subscribe to ONLY subagent events (not main)
    let mut sub = bus.subscribe("events.openstory.06907d46.agent.>")
        .await.expect("subscribe");

    // Publish main — should NOT be received
    let main_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.user.prompt")],
    };
    bus.publish("events.openstory.06907d46.main", &main_batch)
        .await.expect("publish main");

    // Publish subagent — SHOULD be received
    let agent_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.assistant.tool_use")],
    };
    bus.publish("events.openstory.06907d46.agent.a6dcf911", &agent_batch)
        .await.expect("publish agent");

    // Should receive only the agent event
    let received = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");

    assert_eq!(received.events[0].subtype.as_deref(), Some("message.assistant.tool_use"));

    // Main should NOT arrive (give it a moment to be sure)
    let no_more = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sub.receiver.recv(),
    ).await;
    assert!(no_more.is_err(), "should not receive main agent event on agent.> subscription");
}

#[tokio::test]
async fn different_projects_are_isolated() {
    let (bus, _container) = start_nats().await;

    // Subscribe to openstory only
    let mut sub = bus.subscribe("events.openstory.>").await.expect("subscribe");

    // Publish to openstory
    let os_batch = IngestBatch {
        session_id: "06907d46".to_string(),
        project_id: "openstory".to_string(),
        events: vec![test_event("06907d46", "message.user.prompt")],
    };
    bus.publish("events.openstory.06907d46.main", &os_batch).await.expect("publish os");

    // Publish to openclaw — should NOT be received
    let oc_batch = IngestBatch {
        session_id: "b9a810f6".to_string(),
        project_id: "openclaw".to_string(),
        events: vec![test_event("b9a810f6", "message.user.prompt")],
    };
    bus.publish("events.openclaw.b9a810f6.main", &oc_batch).await.expect("publish oc");

    // Should receive only openstory
    let received = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive");
    assert_eq!(received.project_id, "openstory");

    // openclaw should not arrive
    let no_more = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sub.receiver.recv(),
    ).await;
    assert!(no_more.is_err(), "openclaw event should not reach openstory subscription");
}
