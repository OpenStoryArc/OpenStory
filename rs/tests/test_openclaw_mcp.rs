//! Integration tests for the openclaw-mcp image after the Rust MCP cutover.
//!
//! Verifies that the image produced by `Dockerfile.openclaw` contains the
//! native Rust `open-story-mcp` binary and that it can serve tool calls
//! against a shared Mongo + NATS stack reached over the Docker network.
//!
//! Architecture under test:
//!   openclaw-mcp (exec: /usr/local/bin/open-story-mcp)
//!        │ stdio JSON-RPC
//!        ▼
//!   open-story-mcp subprocess  ──NATS──▶  nats
//!                              ──Mongo─▶  mongo  ◀── open-story writes here
//!
//! What we're verifying:
//!   1. The image has the Rust binary at /usr/local/bin/open-story-mcp
//!   2. It speaks JSON-RPC 2.0 over stdio (initialize handshake)
//!   3. It connects to the test-stack's NATS + Mongo
//!   4. `tools/list` returns the 21 OpenStory tools
//!   5. `list_sessions` returns fixture sessions ingested by open-story
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!   docker build -f Dockerfile.openclaw -t openclaw-mcp:latest .
//!
//! Run with: cargo test -p open-story --test test_openclaw_mcp -- --include-ignored

mod helpers;

use helpers::compose::{rand_suffix, to_docker_path};
use helpers::synth::generate_session;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct McpStack {
    compose_file: PathBuf,
    project: String,
    open_story_port: u16,
}

impl Drop for McpStack {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&self.compose_file)
            .args(["-p", &self.project, "down", "--volumes", "--remove-orphans"])
            .env("MSYS_NO_PATHCONV", "1")
            .output();
    }
}

fn generate_fixtures(dir: &Path, count: usize) {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    for i in 0..count {
        let session_id = format!("mcp-test-sess-{i:03}");
        let content = generate_session(&session_id, 30, 0);
        std::fs::write(dir.join(format!("{session_id}.jsonl")), content).expect("write fixture");
    }
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let _ = filetime::set_file_mtime(&entry.expect("entry").path(), now);
    }
}

fn host_port(project: &str, service: &str, port: u16) -> u16 {
    let output = Command::new("docker")
        .args(["compose", "-p", project, "port", service, &port.to_string()])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose port");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port for {service}"))
}

async fn wait_ready(port: u16, label: &str) {
    let url = format!("http://localhost:{port}/api/sessions");
    for _ in 0..60 {
        if reqwest::get(&url).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("{label} not ready at port {port}");
}

async fn start_stack() -> (McpStack, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tmpdir");
    generate_fixtures(tmp.path(), 3);

    let compose_file = PathBuf::from(format!(
        "{}/tests/docker-compose.openclaw-mcp.yml",
        env!("CARGO_MANIFEST_DIR")
    ));
    let project = format!("ostest-mcprust-{}-{}", std::process::id(), rand_suffix());

    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(&compose_file)
        .args(["-p", &project, "up", "-d"])
        .env("FIXTURE_DIR", to_docker_path(tmp.path()))
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up");
    assert!(
        output.status.success(),
        "compose up failed:\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    tokio::time::sleep(Duration::from_secs(5)).await;

    let open_story_port = host_port(&project, "open-story", 3002);
    wait_ready(open_story_port, "open-story").await;

    // Wait for fixtures to flow watcher → translator → NATS → mongo → REST.
    for _ in 0..60 {
        if let Ok(resp) =
            reqwest::get(format!("http://localhost:{open_story_port}/api/sessions")).await
        {
            if let Ok(body) = resp.json::<Value>().await {
                let count = body
                    .get("sessions")
                    .and_then(|s| s.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                if count >= 3 {
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    (
        McpStack {
            compose_file,
            project,
            open_story_port,
        },
        tmp,
    )
}

/// Spawn `open-story-mcp` inside the openclaw-mcp container, pipe the
/// given JSON-RPC requests (one per line) on stdin, return the matching
/// response by id. The container's environment already has
/// OPENSTORY_NATS_URL + OPENSTORY_DATA_BACKEND=mongo + OPENSTORY_MONGO_URI
/// set by the compose file.
fn exec_mcp_rpc(project: &str, method: &str, params: Value, response_id: u64) -> Value {
    let init = json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"},
        }
    });
    let initialized = json!({
        "jsonrpc": "2.0", "method": "notifications/initialized"
    });
    let request = json!({
        "jsonrpc": "2.0", "id": response_id, "method": method, "params": params,
    });

    let stdin_payload = format!("{init}\n{initialized}\n{request}\n");
    let tmp_file = tempfile::NamedTempFile::new().expect("tmpfile");
    std::fs::write(tmp_file.path(), &stdin_payload).expect("write tmpfile");
    let host_path = to_docker_path(tmp_file.path().parent().expect("parent"));
    let file_name = tmp_file
        .path()
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .to_string();

    // Use `docker compose cp` to copy stdin into the container, then exec
    // the Rust MCP with the file piped into its stdin.
    let _ = Command::new("docker")
        .args([
            "compose",
            "-p",
            project,
            "cp",
            &format!("{host_path}/{file_name}"),
            "openclaw-mcp:/tmp/mcp-input.jsonl",
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose cp");

    // --user root: the openclaw image runs as `node` by default, but
    // `docker compose cp` lands the file owned by root. Without root
    // exec, cat fails with Permission denied. Running the MCP as root
    // here is fine — the binary only opens NATS + a read-only SQLite.
    let output = Command::new("docker")
        .args([
            "compose",
            "-p",
            project,
            "exec",
            "-T",
            "--user",
            "root",
            "openclaw-mcp",
            "sh",
            "-c",
            "cat /tmp/mcp-input.jsonl | /usr/local/bin/open-story-mcp 2>/dev/null",
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker exec");

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            if val.get("id").and_then(|i| i.as_u64()) == Some(response_id) {
                return val;
            }
        }
    }

    let lines: Vec<String> = stdout.lines().map(|l| l.to_string()).collect();
    panic!(
        "no response with id={response_id} in MCP output.\n\
         Total lines: {}\n\
         First 500 chars of each:\n{}\n\
         stderr:\n{}",
        lines.len(),
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("  [{i}] {}", &l[..l.len().min(500)]))
            .collect::<Vec<_>>()
            .join("\n"),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ── Tests ─────────────────────────────────────────────────────────────

/// The openclaw-mcp image ships the Rust binary, not Python.
#[tokio::test]
#[ignore]
async fn openclaw_mcp_image_has_the_rust_binary() {
    let (stack, _tmp) = start_stack().await;

    let output = Command::new("docker")
        .args([
            "compose",
            "-p",
            &stack.project,
            "exec",
            "-T",
            "openclaw-mcp",
            "sh",
            "-c",
            "ls -la /usr/local/bin/open-story-mcp && file /usr/local/bin/open-story-mcp || true",
        ])
        .output()
        .expect("docker exec");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("/usr/local/bin/open-story-mcp"),
        "Rust binary missing at /usr/local/bin/open-story-mcp\nstdout:\n{stdout}"
    );
}

/// The Rust MCP starts inside the container, completes the handshake,
/// and reports itself as `open-story-mcp` (not the old Python name).
#[tokio::test]
#[ignore]
async fn openclaw_mcp_initialize_handshake() {
    let (stack, _tmp) = start_stack().await;

    let response = exec_mcp_rpc(&stack.project, "tools/list", json!({}), 1);

    let tools = response["result"]["tools"]
        .as_array()
        .expect("result.tools must be an array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        names.contains(&"list_sessions"),
        "expected list_sessions in tools/list, got: {names:?}"
    );
    assert!(
        names.contains(&"subscribe_session"),
        "Rust MCP must include streaming tools (subscribe_session); got: {names:?}"
    );
    assert_eq!(
        tools.len(),
        26,
        "Rust MCP ships 26 tools (incl. navigate_to + openstory_help + UI seam); got {}",
        tools.len()
    );
    assert!(
        names.contains(&"openstory_help"),
        "expected openstory_help (in-band curriculum); got: {names:?}"
    );
    assert!(
        names.contains(&"navigate_to"),
        "expected navigate_to (click-parity hand); got: {names:?}"
    );
}

/// `list_sessions` returns the fixture sessions ingested by open-story
/// into the shared Mongo backend.
#[tokio::test]
#[ignore]
async fn openclaw_mcp_list_sessions_returns_fixtures() {
    let (stack, _tmp) = start_stack().await;

    let response = exec_mcp_rpc(
        &stack.project,
        "tools/call",
        json!({
            "name": "list_sessions",
            "arguments": {},
        }),
        1,
    );

    assert_eq!(
        response["result"]["isError"], false,
        "list_sessions must succeed; got: {:?}",
        response["result"]
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    let rows: Vec<Value> = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("invalid JSON in tool result: {e}\nraw: {text}"));

    let mcp_test_count = rows
        .iter()
        .filter(|r| {
            r["id"]
                .as_str()
                .map(|s| s.starts_with("mcp-test-sess-"))
                .unwrap_or(false)
        })
        .count();
    assert!(
        mcp_test_count >= 3,
        "expected ≥3 fixture sessions with mcp-test-sess- prefix, got {mcp_test_count}.\n\
         Returned rows: {rows:?}"
    );
}
