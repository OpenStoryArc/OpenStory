//! OpenClaw + Open Story integration tests.
//!
//! Validates that Open Story correctly observes OpenClaw agent sessions
//! end-to-end: real gateway, real LLM call, real JSONL, real ingest.
//!
//! Requires:
//!   - `openclaw:test` image: `cd ~/projects/openclaw && docker build -t openclaw:test .`
//!   - `open-story:test` image: `cd rs && docker build -t open-story:test .`
//!   - `ANTHROPIC_API_KEY` environment variable
//!
//! Run with:
//!   ANTHROPIC_API_KEY=$(cat .anthropic_api_key) cargo test -p open-story \
//!     --test test_openclaw_integration -- --ignored --nocapture

mod helpers;

use helpers::openclaw::start_openclaw_stack;
use std::time::Duration;

fn header(title: &str) {
    eprintln!("\n\x1b[1m  ── {} ──\x1b[0m", title);
}

fn step(msg: &str) {
    eprintln!("  \x1b[36m▸\x1b[0m {msg}");
}

fn detail(label: &str, value: &str) {
    eprintln!("    \x1b[2m{label}:\x1b[0m {value}");
}

fn ok(msg: &str) {
    eprintln!("  \x1b[32m✓\x1b[0m {msg}");
}

/// Core test: OpenClaw session appears in Open Story after sending a message.
#[tokio::test]
#[ignore] // Requires openclaw:test image + API key (costs real money)
async fn openclaw_session_observed_by_openstory() {
    header("OpenClaw → Open Story end-to-end");

    step("Starting compose stack (OpenClaw gateway + Open Story server)...");
    let stack = start_openclaw_stack().await;
    ok("Both containers healthy");
    detail("Open Story", &stack.openstory_url());

    step("Sending message to OpenClaw agent via CLI...");
    stack
        .send_agent_message("Hello. Reply with just the word 'acknowledged' and nothing else.")
        .await;
    ok("Agent responded");

    step("Waiting for session to appear in Open Story...");
    stack
        .wait_for_openstory_session(Duration::from_secs(90))
        .await;

    let sessions = stack.get_openstory_sessions().await;
    ok(&format!("Found {} session(s)", sessions.len()));

    let session_id = sessions[0]["session_id"]
        .as_str()
        .expect("session_id should be a string");
    detail("session_id", session_id);
    if let Some(proj) = sessions[0].get("project_id").and_then(|v| v.as_str()) {
        detail("project_id", proj);
    }

    step("Fetching events from Open Story API...");
    let events = stack.get_openstory_events(session_id).await;
    ok(&format!("Found {} events", events.len()));
    assert!(!events.is_empty(), "session should have events");

    // Print each event
    header("Event timeline");
    for (i, event) in events.iter().enumerate() {
        let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or("?");
        let agent = event.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
        let data = event.get("data").unwrap_or(&serde_json::Value::Null);
        let text = data.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let tool = data.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        let model = data.get("model").and_then(|v| v.as_str()).unwrap_or("");

        let summary = match subtype {
            s if s.contains("user.prompt") => format!("\"{}\"", truncate(text, 80)),
            s if s.contains("assistant.text") => format!("\"{}\"", truncate(text, 80)),
            s if s.contains("tool_use") => format!("tool={tool}"),
            s if s.contains("tool_result") => {
                let tool_name = data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                format!("tool={tool_name}")
            }
            s if s.contains("session_start") => format!("model={model}"),
            s if s.contains("model_change") => format!("model={model}"),
            _ => String::new(),
        };

        eprintln!(
            "  \x1b[33m{:>2}\x1b[0m  \x1b[2m[{agent}]\x1b[0m {subtype} {summary}",
            i + 1
        );
    }

    // Verify subtypes
    let subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.get("subtype").and_then(|v| v.as_str()))
        .collect();

    header("Assertions");
    assert!(
        subtypes.contains(&"message.user.prompt"),
        "should have user prompt, got: {:?}",
        subtypes
    );
    ok("Has message.user.prompt");

    assert!(
        subtypes.iter().any(|s| s.starts_with("message.assistant")),
        "should have assistant response, got: {:?}",
        subtypes
    );
    ok("Has message.assistant.*");

    // Verify agent field
    for event in &events {
        if let Some(agent) = event.get("agent").and_then(|v| v.as_str()) {
            assert_eq!(agent, "pi-mono", "OpenClaw events should have agent=pi-mono");
        }
    }
    ok("All events have agent=\"pi-mono\"");

    // SQLite persistence: verify field-level integrity on every event
    header("SQLite persistence checks");
    assert!(
        events.len() >= 4,
        "expected at least 4 events (session_start + model_change + prompt + response), got {}",
        events.len()
    );
    ok(&format!("Event count: {} (>= 4)", events.len()));

    for (i, event) in events.iter().enumerate() {
        let ctx = format!("event[{i}]");
        assert!(
            event.get("id").and_then(|v| v.as_str()).map_or(false, |s| !s.is_empty()),
            "{ctx}: id missing or empty"
        );
        assert!(
            event.get("subtype").and_then(|v| v.as_str()).is_some(),
            "{ctx}: subtype missing"
        );
        assert!(
            event.get("time").and_then(|v| v.as_str()).map_or(false, |s| !s.is_empty()),
            "{ctx}: time missing or empty"
        );
        let data = event.get("data").expect(&format!("{ctx}: data missing"));
        assert!(
            data.get("session_id").and_then(|v| v.as_str()).is_some(),
            "{ctx}: data.session_id missing"
        );
        assert!(
            !data.get("raw").unwrap_or(&serde_json::Value::Null).is_null(),
            "{ctx}: data.raw missing"
        );
    }
    ok("All events have id, subtype, time, data.session_id, data.raw");

    // Session metadata persisted
    let session = &sessions[0];
    let event_count = session.get("event_count").and_then(|v| v.as_u64());
    assert!(
        event_count.is_some() && event_count.unwrap() > 0,
        "session event_count should be > 0"
    );
    ok(&format!("Session event_count: {}", event_count.unwrap()));

    // Raw data integrity: user prompt preserves pi-mono format
    let user_prompt = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.user.prompt"));
    if let Some(prompt) = user_prompt {
        let raw = &prompt["data"]["raw"];
        assert_eq!(raw["type"], "message", "raw.type should be 'message'");
        assert_eq!(raw["message"]["role"], "user", "raw.message.role should be 'user'");
        ok("User prompt raw data preserved in pi-mono native format");
    }

    // Dump raw JSONL from container for inspection
    header("Raw JSONL from OpenClaw container");
    stack.dump_openclaw_sessions().await;

    eprintln!("\n  \x1b[32m\x1b[1mPASS\x1b[0m\n");
}

/// View records render correctly for OpenClaw sessions.
#[tokio::test]
#[ignore]
async fn openclaw_view_records_render() {
    header("View record rendering");

    step("Starting compose stack...");
    let stack = start_openclaw_stack().await;
    ok("Both containers healthy");

    step("Sending message...");
    stack
        .send_agent_message("Hello. Reply with just the word 'acknowledged' and nothing else.")
        .await;
    ok("Agent responded");

    step("Waiting for session...");
    stack
        .wait_for_openstory_session(Duration::from_secs(90))
        .await;

    let sessions = stack.get_openstory_sessions().await;
    let session_id = sessions[0]["session_id"].as_str().unwrap();
    detail("session_id", session_id);

    step("Fetching view records...");
    let records = stack.get_openstory_view_records(session_id).await;
    ok(&format!("Found {} view records", records.len()));
    assert!(!records.is_empty(), "should have view records");

    // Print each record
    header("View records");
    for (i, record) in records.iter().enumerate() {
        let record_type = record.get("record_type").and_then(|v| v.as_str()).unwrap_or("?");
        let payload = record.get("payload").unwrap_or(&serde_json::Value::Null);

        let summary = match record_type {
            "user_message" => {
                let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
                format!("\"{}\"", truncate(text, 60))
            }
            "assistant_message" => {
                let model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("?");
                format!("model={model}")
            }
            "tool_call" => {
                let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                format!("tool={name}")
            }
            "tool_result" => {
                let error = payload.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                format!("error={error}")
            }
            "token_usage" => {
                let input = payload.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = payload.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("in={input} out={output}")
            }
            _ => String::new(),
        };

        eprintln!("  \x1b[33m{:>2}\x1b[0m  {record_type} {summary}", i + 1);
    }

    let record_types: Vec<&str> = records
        .iter()
        .filter_map(|r| r.get("record_type").and_then(|v| v.as_str()))
        .collect();

    header("Assertions");
    assert!(
        record_types.contains(&"user_message"),
        "should have user_message records, got: {:?}",
        record_types
    );
    ok("Has user_message");

    for record in &records {
        assert!(
            record.get("record_type").is_some(),
            "view record missing record_type"
        );
    }
    ok("All records have record_type");

    eprintln!("\n  \x1b[32m\x1b[1mPASS\x1b[0m\n");
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
