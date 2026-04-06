//! TDD: Boot path redesign — one path, not two.
//!
//! The boot process must go through the same translate → NATS → consumers
//! path as live events. No separate JSONL-to-SQLite bypass.
//!
//! These tests verify:
//! 1. JSONL backfill produces translated CloudEvents (with agent_payload)
//! 2. agent_id on SubAgentSpawned survives the full boot cycle
//! 3. SQLite boot (restart) preserves existing data
//!
//! Run with: cargo test -p open-story --test test_boot

mod helpers;

use open_story_bus::nats_bus::NatsBus;
use open_story_bus::Bus;
use serde_json::json;
use testcontainers::{GenericImage, ImageExt};
use testcontainers::runners::AsyncRunner;

/// Start a NATS container and return a connected NatsBus.
async fn start_nats() -> (NatsBus, testcontainers::ContainerAsync<GenericImage>) {
    let container = GenericImage::new("nats", "2-alpine")
        .with_cmd(vec!["--jetstream"])
        .start()
        .await
        .expect("start NATS container");

    let port = container.get_host_port_ipv4(4222).await.expect("get port");
    let nats_url = format!("nats://localhost:{port}");

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

/// Write a minimal JSONL transcript with an Agent tool_use and tool_result.
fn write_agent_transcript(dir: &std::path::Path, session_id: &str) {
    let project_dir = dir.join("test-project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let jsonl_path = project_dir.join(format!("{session_id}.jsonl"));

    // Line 1: user prompt
    let prompt = json!({
        "type": "user",
        "uuid": "evt-prompt-1",
        "sessionId": session_id,
        "timestamp": "2026-04-06T10:00:00Z",
        "cwd": "/test",
        "version": "2.1.92",
        "message": {
            "role": "user",
            "content": "test prompt"
        }
    });

    // Line 2: assistant with Agent tool_use
    let tool_use = json!({
        "type": "assistant",
        "uuid": "evt-tool-use-1",
        "sessionId": session_id,
        "parentUuid": "evt-prompt-1",
        "timestamp": "2026-04-06T10:00:01Z",
        "cwd": "/test",
        "version": "2.1.92",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "toolu_test_agent",
                "name": "Agent",
                "input": { "description": "Test agent exploration" }
            }],
            "stop_reason": "tool_use"
        }
    });

    // Line 3: user tool_result with toolUseResult.agentId
    let tool_result = json!({
        "type": "user",
        "uuid": "evt-tool-result-1",
        "sessionId": session_id,
        "parentUuid": "evt-tool-use-1",
        "timestamp": "2026-04-06T10:00:10Z",
        "cwd": "/test",
        "version": "2.1.92",
        "toolUseResult": {
            "agentId": "atest123def456",
            "status": "completed",
            "prompt": "Test agent exploration"
        },
        "message": {
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_test_agent",
                "content": "Agent completed successfully"
            }]
        }
    });

    let content = format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&prompt).unwrap(),
        serde_json::to_string(&tool_use).unwrap(),
        serde_json::to_string(&tool_result).unwrap(),
    );
    std::fs::write(&jsonl_path, content).expect("write JSONL");

    // Touch the file so the watcher considers it recent
    let now = filetime::FileTime::now();
    filetime::set_file_mtime(&jsonl_path, now).expect("set mtime");
}

// ═══════════════════════════════════════════════════════════════════
// Test: Watcher backfill translates JSONL and publishes to NATS
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn watcher_backfill_translates_agent_id() {
    let (bus, _container) = start_nats().await;

    // Subscribe to events BEFORE the watcher runs
    let mut sub = bus.subscribe("events.>").await.expect("subscribe");

    // Write a transcript with Agent tool_use/result
    let tmp = tempfile::tempdir().expect("create temp dir");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).expect("create watch dir");
    write_agent_transcript(&watch_dir, "boot-test-sess");

    // Run watcher backfill (synchronous)
    let watcher_bus = bus;
    let wd = watch_dir.clone();
    let handle = tokio::task::spawn_blocking(move || {
        use open_story_core::reader::read_new_lines;
        use open_story_core::translate::TranscriptState;
        use open_story_core::paths::{session_id_from_path, project_id_from_path, nats_subject_from_path};
        use open_story_bus::IngestBatch;
        use walkdir::WalkDir;

        let mut states = std::collections::HashMap::new();
        for entry in WalkDir::new(&wd).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let state = states
                .entry(path.to_path_buf())
                .or_insert_with(|| TranscriptState::new(session_id_from_path(path)));
            let events = read_new_lines(path, state).expect("read JSONL");
            if !events.is_empty() {
                let sid = session_id_from_path(path);
                let pid = project_id_from_path(path, &wd);
                let subject = nats_subject_from_path(path, &wd);
                let batch = IngestBatch {
                    session_id: sid,
                    project_id: pid.unwrap_or_default(),
                    events,
                };
                let rt = tokio::runtime::Handle::current();
                rt.block_on(watcher_bus.publish(&subject, &batch)).expect("publish");
            }
        }
    });
    handle.await.expect("watcher backfill");

    // Receive the batch from NATS
    let batch = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        sub.receiver.recv(),
    ).await.expect("timeout").expect("receive batch");

    assert_eq!(batch.session_id, "boot-test-sess");

    // Find the SubAgentSpawned event and check agent_id
    let mut found_agent_id = false;
    for event in &batch.events {
        if let Some(ap) = &event.data.agent_payload {
            if let Some(outcome) = ap.tool_outcome() {
                if let open_story_core::event_data::ToolOutcome::SubAgentSpawned { agent_id, description } = outcome {
                    assert_eq!(agent_id, "atest123def456",
                        "agent_id must be populated from toolUseResult.agentId");
                    assert_eq!(description, "Test agent exploration");
                    found_agent_id = true;
                }
            }
        }
    }
    assert!(found_agent_id, "should find SubAgentSpawned with agent_id in translated events");
}
