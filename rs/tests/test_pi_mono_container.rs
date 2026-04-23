//! Integration tests: pi-mono sessions via testcontainers.
//!
//! Verifies the end-to-end path: pi-mono JSONL fixture → file watcher →
//! format detection → translate_pi → ingest → SQLite → API.
//!
//! These tests verify durable persistence — events returned by the API
//! are read from SQLite, not in-memory projections.
//!
//! Requires Docker and a pre-built image:
//!   docker build -t open-story:test ./rs
//!
//! Run with: cargo test -p open-story --test test_pi_mono_container

mod helpers;

use helpers::container::start_open_story;
use serde_json::Value;

/// Create a fixture directory containing only the pi-mono session file.
fn pi_mono_fixture_dir() -> std::path::PathBuf {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    let src = fixtures.join("pi_mono_session.jsonl");
    let dst = tmp.path().join("pi_mono_session.jsonl");
    std::fs::copy(&src, &dst).expect("copy pi-mono fixture");
    // Leak the TempDir so it survives past the function return.
    let path = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    path
}

/// Helper: fetch sessions list from the API, unwrapping the `{sessions, total}` envelope.
async fn fetch_sessions(base_url: &str) -> Vec<Value> {
    let body: Value = reqwest::get(format!("{base_url}/api/sessions"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body.get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .expect("/api/sessions response should contain a `sessions` array")
}

/// Helper: find the pi-mono session in the sessions list.
fn find_pi_session(sessions: &[Value]) -> &Value {
    sessions
        .iter()
        .find(|s| {
            s["session_id"]
                .as_str()
                .map_or(false, |id| id.contains("pi_mono"))
        })
        .expect("pi-mono session not found in /api/sessions")
}

/// Pi-mono session loads and session metadata is persisted to SQLite.
#[tokio::test]
async fn pi_mono_session_metadata_persisted() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session = find_pi_session(&sessions);

    // Session ID matches fixture filename stem
    let session_id = session["session_id"].as_str().unwrap();
    assert!(
        session_id.contains("pi_mono"),
        "session_id should contain 'pi_mono', got: {session_id}"
    );

    // Event count persisted (read from SQLite sessions table)
    let event_count = session.get("event_count").and_then(|v| v.as_u64());
    assert!(
        event_count.is_some() && event_count.unwrap() > 0,
        "event_count should be > 0, got: {:?}",
        event_count
    );

    // Project ID present
    let project_id = session.get("project_id").and_then(|v| v.as_str());
    assert!(
        project_id.is_some(),
        "project_id should be present"
    );
}

/// Exact event count matches fixture (10 lines → 10 events in SQLite).
#[tokio::test]
async fn pi_mono_exact_event_count_in_sqlite() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session_id = find_pi_session(&sessions)["session_id"].as_str().unwrap();

    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{}/events",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    // Fixture has exactly 10 JSONL lines, all translatable
    assert_eq!(
        events.len(),
        10,
        "expected exactly 10 events from pi_mono_session.jsonl, got {}",
        events.len()
    );
}

/// Full subtype distribution matches fixture exactly (order-independent).
#[tokio::test]
async fn pi_mono_subtype_distribution_matches_fixture() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session_id = find_pi_session(&sessions)["session_id"].as_str().unwrap();

    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{}/events",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    let mut subtypes: Vec<&str> = events
        .iter()
        .filter_map(|e| e.get("subtype").and_then(|v| v.as_str()))
        .collect();
    subtypes.sort();

    let mut expected = vec![
        "message.assistant.text",
        "message.assistant.thinking",
        "message.assistant.tool_use",
        "message.user.prompt",
        "message.user.prompt",
        "message.user.tool_result",
        "progress.bash",
        "system.compact",
        "system.model_change",
        "system.session_start",
    ];
    expected.sort();

    assert_eq!(
        subtypes, expected,
        "subtype distribution doesn't match fixture"
    );
}

/// Every event has required CloudEvent fields persisted in SQLite.
#[tokio::test]
async fn pi_mono_event_fields_persisted() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session_id = find_pi_session(&sessions)["session_id"].as_str().unwrap();

    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{}/events",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    for (i, event) in events.iter().enumerate() {
        let ctx = format!("event[{i}]");

        // CloudEvent ID
        let id = event.get("id").and_then(|v| v.as_str());
        assert!(id.is_some() && !id.unwrap().is_empty(), "{ctx}: id missing or empty");

        // Subtype
        let subtype = event.get("subtype").and_then(|v| v.as_str());
        assert!(subtype.is_some(), "{ctx}: subtype missing");

        // Agent field
        let agent = event.get("agent").and_then(|v| v.as_str());
        assert_eq!(agent, Some("pi-mono"), "{ctx}: agent should be 'pi-mono'");

        // Timestamp
        let time = event.get("time").and_then(|v| v.as_str());
        assert!(time.is_some() && !time.unwrap().is_empty(), "{ctx}: time missing or empty");

        // Data envelope
        let data = event.get("data");
        assert!(data.is_some(), "{ctx}: data missing");

        // Session ID in data matches
        let data_session = data.unwrap().get("session_id").and_then(|v| v.as_str());
        assert_eq!(
            data_session,
            Some(session_id),
            "{ctx}: data.session_id should match queried session"
        );

        // Raw is present (untouched original line)
        let raw = data.unwrap().get("raw");
        assert!(raw.is_some() && !raw.unwrap().is_null(), "{ctx}: data.raw missing");
    }
}

/// Raw data integrity — raw field matches the original pi-mono JSONL structure.
/// Proves raw was never mutated through: file → watcher → translate → ingest → SQLite → API.
#[tokio::test]
async fn pi_mono_raw_data_integrity() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session_id = find_pi_session(&sessions)["session_id"].as_str().unwrap();

    let events: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{}/events",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    // Find the user prompt event
    let user_prompt = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.user.prompt"))
        .expect("no user prompt event found");

    let raw = &user_prompt["data"]["raw"];

    // Raw preserves pi-mono's native structure
    assert_eq!(raw["type"], "message", "raw.type should be 'message' (pi-mono format)");
    assert_eq!(raw["message"]["role"], "user", "raw.message.role should be 'user'");
    assert_eq!(
        raw["message"]["content"][0]["type"], "text",
        "raw.message.content[0].type should be 'text'"
    );
    assert_eq!(
        raw["message"]["content"][0]["text"],
        "Read the config file and explain it",
        "raw.message.content[0].text should match fixture"
    );

    // Find the tool_use event — raw should preserve toolCall (not tool_use)
    let tool_use = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.assistant.tool_use"))
        .expect("no tool_use event found");

    let raw = &tool_use["data"]["raw"];
    assert_eq!(raw["type"], "message", "raw.type should be 'message'");
    assert_eq!(raw["message"]["role"], "assistant");

    // Raw preserves pi-mono's native toolCall type — NOT normalized to tool_use
    let content = &raw["message"]["content"];
    let has_tool_call = content
        .as_array()
        .map_or(false, |arr| arr.iter().any(|b| b["type"] == "toolCall"));
    assert!(
        has_tool_call,
        "raw should preserve pi-mono's native 'toolCall' type, not normalize to 'tool_use'"
    );

    // Find a tool result — raw should preserve pi-mono's toolResult role
    let tool_result = events
        .iter()
        .find(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("message.user.tool_result"))
        .expect("no tool_result event found");

    let raw = &tool_result["data"]["raw"];
    assert_eq!(
        raw["message"]["role"], "toolResult",
        "raw should preserve pi-mono's native 'toolResult' role"
    );
    assert_eq!(
        raw["message"]["toolCallId"], "tc-001",
        "raw should preserve pi-mono's native 'toolCallId' field"
    );
}

/// View records render correctly from SQLite-stored events.
#[tokio::test]
async fn pi_mono_view_records_from_sqlite() {
    let fixture_dir = pi_mono_fixture_dir();
    let server = start_open_story(&fixture_dir).await;
    server.wait_for_sessions().await;

    let sessions = fetch_sessions(&server.base_url()).await;

    let session_id = find_pi_session(&sessions)["session_id"].as_str().unwrap();

    let records: Vec<Value> = reqwest::get(format!(
        "{}/api/sessions/{}/view-records",
        server.base_url(),
        session_id
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();

    assert!(
        !records.is_empty(),
        "should have view records for pi-mono session"
    );

    // Every record has record_type
    for record in &records {
        assert!(
            record.get("record_type").is_some(),
            "view record missing record_type: {:?}",
            record
        );
    }

    let record_types: Vec<&str> = records
        .iter()
        .filter_map(|r| r.get("record_type").and_then(|v| v.as_str()))
        .collect();

    assert!(record_types.contains(&"user_message"), "should have user_message");
    assert!(
        record_types.contains(&"tool_call") || record_types.contains(&"assistant_message"),
        "should have tool_call or assistant_message, got: {:?}",
        record_types
    );
}
