//! Integration tests: Open Story captures events from Claude Code.
//!
//! These tests exercise the full observability loop:
//!   Claude Code (headless) → HTTP hooks → Open Story ingest → API
//!
//! Prerequisites:
//!   - docker build -t open-story:test ./rs
//!   - docker build -t claude-runner:test -f rs/tests/Dockerfile.claude-runner .
//!   - ANTHROPIC_API_KEY environment variable
//!
//! Run with:
//!   cargo test -p open-story --test test_claude_integration -- --ignored --nocapture
//!
//! Or via just:
//!   just test-claude

mod helpers;

use helpers::compose::{start_stack, to_docker_path, TestConfig};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Path to the integration compose file.
fn compose_file() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/docker-compose.integration.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Path to the test-repo fixture directory.
fn test_repo_dir() -> PathBuf {
    PathBuf::from(format!(
        "{}/tests/fixtures/test-repo",
        env!("CARGO_MANIFEST_DIR")
    ))
}

/// Start the integration stack (Open Story + Claude runner + verifier).
///
/// This uses docker compose directly rather than the TestStack helper,
/// because we need the claude-runner service which isn't in the standard configs.
async fn run_integration_compose() -> (String, u16) {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY must be set for Claude integration tests");

    let fixture_path = to_docker_path(&test_repo_dir());
    let compose_path = compose_file();
    let project_name = format!("ostest-claude-{}-{}", std::process::id(), rand_suffix());

    // Build and start
    let (program, first_arg) = if cfg!(target_os = "windows") {
        ("podman", "compose")
    } else {
        ("docker", "compose")
    };

    let output = Command::new(program)
        .args([first_arg, "-f"])
        .arg(&compose_path)
        .args(["-p", &project_name])
        .args(["up", "--build", "--abort-on-container-exit"])
        .env("FIXTURE_DIR", &fixture_path)
        .env("ANTHROPIC_API_KEY", &api_key)
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("failed to run compose up");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Get the server port before we check success (for diagnostics)
    let server_port = get_host_port(program, first_arg, &project_name, "open-story", 3002)
        .unwrap_or(0);

    if !output.status.success() {
        // Tear down on failure
        let _ = Command::new(program)
            .args([first_arg, "-f"])
            .arg(&compose_path)
            .args(["-p", &project_name, "down", "--volumes", "--remove-orphans"])
            .env("MSYS_NO_PATHCONV", "1")
            .output();

        panic!(
            "Integration compose failed:\nstdout: {}\nstderr: {}",
            &stdout[..stdout.len().min(2000)],
            &stderr[..stderr.len().min(2000)],
        );
    }

    (project_name, server_port)
}

/// Get host port for a service.
fn get_host_port(program: &str, first_arg: &str, project: &str, service: &str, port: u16) -> Option<u16> {
    let output = Command::new(program)
        .args([first_arg, "-p", project, "port", service, &port.to_string()])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().rsplit(':').next()?.parse().ok()
}

/// Random suffix for unique project names.
fn rand_suffix() -> String {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:x}", t % 0xFFFF)
}

/// Tear down a compose stack.
fn teardown(project_name: &str) {
    let compose_path = compose_file();
    let (program, first_arg) = if cfg!(target_os = "windows") {
        ("podman", "compose")
    } else {
        ("docker", "compose")
    };
    let _ = Command::new(program)
        .args([first_arg, "-f"])
        .arg(&compose_path)
        .args(["-p", project_name, "down", "--volumes", "--remove-orphans"])
        .env("MSYS_NO_PATHCONV", "1")
        .output();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Full loop: Claude Code runs a task, Open Story captures events via hooks.
///
/// This is the "gold standard" integration test — it proves the complete
/// observability pipeline works end-to-end with a real Claude session.
#[tokio::test]
#[ignore] // requires ANTHROPIC_API_KEY + Docker images
async fn claude_session_captured_by_open_story() {
    let (project_name, server_port) = run_integration_compose().await;

    // The compose file's verifier service already checks for sessions.
    // If we get here without panic, the compose succeeded (verifier exited 0).
    // But let's also verify from the test side if the port was discovered.
    if server_port > 0 {
        let url = format!("http://localhost:{server_port}/api/sessions");
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(sessions) = resp.json::<Vec<Value>>().await {
                assert!(
                    !sessions.is_empty(),
                    "Expected at least one session captured from Claude"
                );

                // Verify the session has events
                let session_id = sessions[0]["session_id"]
                    .as_str()
                    .expect("session_id should be a string");

                let events_url = format!(
                    "http://localhost:{server_port}/api/sessions/{session_id}/events"
                );
                let events: Vec<Value> = reqwest::get(&events_url)
                    .await
                    .expect("events request failed")
                    .json()
                    .await
                    .expect("events parse failed");

                assert!(
                    !events.is_empty(),
                    "Expected events in session {session_id}"
                );

                // Check for expected event subtypes
                let subtypes: Vec<&str> = events
                    .iter()
                    .filter_map(|e| e["subtype"].as_str())
                    .collect();

                println!("Captured {} events with subtypes: {:?}", events.len(), subtypes);

                // Should have at least some tool use (Read/Write/etc.)
                let has_tool_use = subtypes
                    .iter()
                    .any(|s| s.contains("tool_use") || s.contains("tool_result"));
                assert!(
                    has_tool_use,
                    "Expected tool_use or tool_result events, got: {:?}",
                    subtypes
                );
            }
        }
    }

    teardown(&project_name);
}

/// Smoke test: Open Story server starts and accepts hooks independently.
/// (No Claude needed — just validates the compose infrastructure works.)
#[tokio::test]
#[ignore] // requires Docker images
async fn integration_stack_open_story_healthy() {
    let fixture_path = to_docker_path(&test_repo_dir());
    let compose_path = compose_file();
    let project_name = format!("ostest-health-{}-{}", std::process::id(), rand_suffix());

    let (program, first_arg) = if cfg!(target_os = "windows") {
        ("podman", "compose")
    } else {
        ("docker", "compose")
    };

    // Start just the open-story service
    let output = Command::new(program)
        .args([first_arg, "-f"])
        .arg(&compose_path)
        .args(["-p", &project_name])
        .args(["up", "-d", "open-story"])
        .env("FIXTURE_DIR", &fixture_path)
        .env("ANTHROPIC_API_KEY", "not-needed-for-health-check")
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("failed to start open-story");

    assert!(
        output.status.success(),
        "Failed to start open-story: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    tokio::time::sleep(Duration::from_secs(3)).await;

    let server_port = get_host_port(program, first_arg, &project_name, "open-story", 3002);

    if let Some(port) = server_port {
        let url = format!("http://localhost:{port}/api/sessions");
        let resp = reqwest::get(&url).await;
        assert!(resp.is_ok(), "Open Story should respond on /api/sessions");
        assert_eq!(resp.unwrap().status(), 200);

        // Test hooks endpoint
        let client = reqwest::Client::new();
        let hook_body = serde_json::json!({
            "session_id": "integration-health-test",
            "hook_event_name": "PostToolUse",
            "tool_name": "Read",
            "transcript_path": ""
        });

        let hook_resp = client
            .post(format!("http://localhost:{port}/hooks"))
            .json(&hook_body)
            .send()
            .await
            .expect("POST /hooks failed");

        assert_eq!(hook_resp.status(), 202);
    }

    // Cleanup
    let _ = Command::new(program)
        .args([first_arg, "-f"])
        .arg(&compose_path)
        .args(["-p", &project_name, "down", "--volumes", "--remove-orphans"])
        .env("MSYS_NO_PATHCONV", "1")
        .output();
}
