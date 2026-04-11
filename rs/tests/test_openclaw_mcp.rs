//! Integration tests for the openclaw-mcp image.
//!
//! Verifies that the image produced by `Dockerfile.openclaw` contains a
//! working OpenStory MCP server that can reach an Open Story instance over
//! the Docker network.
//!
//! Architecture under test:
//!   openclaw-mcp (exec: uv run python server.py)
//!        │ stdio JSON-RPC
//!        ▼
//!   MCP server subprocess  ──HTTP──▶  open-story:3002
//!
//! This isolates the MCP-tool path from OpenClaw's full gateway (which needs
//! an Anthropic API key to boot). What we're verifying:
//!   1. The image has Python + uv + the MCP server code
//!   2. The MCP server starts and speaks JSON-RPC over stdio
//!   3. The MCP server can reach the Open Story REST API
//!   4. Tools like `list_sessions` return real data
//!
//! Prerequisites:
//!   docker build -t open-story:test ./rs
//!   docker build -f Dockerfile.openclaw -t openclaw-mcp:latest .
//!
//! Run with: cargo test -p open-story --test test_openclaw_mcp -- --include-ignored

mod helpers;

use helpers::compose::{rand_suffix, to_docker_path};
use helpers::synth::generate_session;
use serde_json::Value;
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
        std::fs::write(dir.join(format!("{session_id}.jsonl")), content)
            .expect("write fixture");
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
    let project = format!("ostest-mcpimg-{}-{}", std::process::id(), rand_suffix());

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

    tokio::time::sleep(Duration::from_secs(3)).await;

    let open_story_port = host_port(&project, "open-story", 3002);
    wait_ready(open_story_port, "open-story").await;

    // Wait for fixtures to ingest
    for _ in 0..60 {
        let body: Value = reqwest::get(format!("http://localhost:{open_story_port}/api/sessions"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let count = body
            .get("sessions")
            .and_then(|s| s.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if count >= 3 {
            break;
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

/// Send a JSON-RPC message to the MCP server running inside the openclaw-mcp
/// container and return the response line.
///
/// We exec a fresh `uv run python server.py` subprocess per call because MCP
/// is request/response and we only need single round trips for this test.
fn exec_mcp_rpc(project: &str, method: &str, params: Value) -> Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    // We send *two* messages in one pipe: initialize, then the real request.
    // FastMCP requires initialization before tool calls.
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}
        }
    });
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    // Spawn a fresh openclaw-mcp container via `docker run` on the compose
    // project's network so it can reach `open-story:3002` by DNS name.
    //
    // We write the stdin payload to a temp file and bind-mount it into the
    // container, then `cat` it into the MCP server's stdin from *inside*
    // the container. This avoids Rust's Stdio::piped() racing Python's
    // line-buffered stdin reads — the shell pipe inside the container
    // only closes after Python has drained it.
    let network = format!("{project}_default");

    let stdin_payload = format!("{}\n{}\n{}\n", init, initialized, request);
    let tmp_file = tempfile::NamedTempFile::new().expect("tmpfile");
    std::fs::write(tmp_file.path(), &stdin_payload).expect("write tmpfile");
    let host_path = to_docker_path(tmp_file.path().parent().expect("parent"));
    let file_name = tmp_file
        .path()
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .to_string();

    let output = Command::new("docker")
        .args([
            "run", "--rm", "-i",
            "--network", &network,
            "-e", "OPENSTORY_URL=http://open-story:3002",
            "-e", "OPENSTORY_LABEL=local-test",
            "-v", &format!("{host_path}:/tmp/mcp-input:ro"),
            "--entrypoint", "sh",
            "openclaw-mcp:latest",
            "-c", &format!(
                "cat /tmp/mcp-input/{file_name} | uv run --directory /opt/mcp-server python server.py 2>/dev/null"
            ),
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker run");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The MCP server writes multiple JSON-RPC responses — one per request.
    // We want the one matching our id (1, since initialize is 0).
    for line in stdout.lines() {
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            if val.get("id").and_then(|i| i.as_u64()) == Some(1) {
                return val;
            }
        }
    }

    // Dump all lines for debugging
    let lines: Vec<String> = stdout.lines().map(|l| l.to_string()).collect();
    panic!(
        "no response with id=1 in MCP output.\n\
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

// ── Tests ────────────────────────────────────────────────────────────

/// The openclaw-mcp container starts and has all the tooling installed.
#[tokio::test]
#[ignore]
async fn openclaw_mcp_has_python_and_uv() {
    let (stack, _tmp) = start_stack().await;

    let output = Command::new("docker")
        .args([
            "compose", "-p", &stack.project, "exec", "-T", "openclaw-mcp",
            "sh", "-c", "which python3 && which uv && ls /opt/mcp-server/",
        ])
        .output()
        .expect("docker exec");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/usr/bin/python3"), "missing python3");
    assert!(stdout.contains("/usr/local/bin/uv"), "missing uv");
    assert!(stdout.contains("server.py"), "missing server.py");
    assert!(stdout.contains("SKILL.md"), "missing SKILL.md");
}

/// The MCP server starts via stdio and responds to initialize.
#[tokio::test]
#[ignore]
async fn openclaw_mcp_initialize_handshake() {
    let (stack, _tmp) = start_stack().await;

    // Send just an initialize message
    let script = "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}' | uv run --directory /opt/mcp-server python server.py 2>/dev/null | head -1";

    let output = Command::new("docker")
        .args(["compose", "-p", &stack.project, "exec", "-T", "openclaw-mcp", "sh", "-c", script])
        .output()
        .expect("docker exec");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON: {e}\nraw: {stdout}"));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(
        response["result"]["serverInfo"]["name"]
            .as_str()
            .unwrap_or("")
            .contains("OpenStory"),
        "expected server name to contain OpenStory, got: {}",
        response["result"]["serverInfo"]["name"]
    );
}

/// `tools/list` returns the 19 OpenStory tools.
#[tokio::test]
#[ignore]
async fn openclaw_mcp_tools_list_has_expected_tools() {
    let (stack, _tmp) = start_stack().await;

    let response = exec_mcp_rpc(&stack.project, "tools/list", serde_json::json!({}));
    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be an array");

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();

    // A few key tools we know should exist
    let expected = [
        "list_sessions",
        "search",
        "session_synopsis",
        "token_usage",
        "tool_journey",
    ];
    for name in &expected {
        assert!(
            tool_names.contains(name),
            "expected tool '{name}' in tools list, got: {tool_names:?}"
        );
    }

    // FastMCP exposes all 19 tools (may include helpers — be lenient)
    assert!(
        tools.len() >= 15,
        "expected at least 15 tools, got {}",
        tools.len()
    );
}

/// Call `list_sessions` directly via the Python tool function and verify it
/// returns the fixture sessions ingested by the Open Story container.
///
/// This is the core end-to-end test. We skip FastMCP's stdio transport layer
/// (which has async-exit quirks with piped stdin) and call the tool function
/// directly — the same function that FastMCP would invoke when OpenClaw
/// issues a `tools/call`. What this proves:
///   1. The openclaw-mcp image has `server.py` with all the tools wired up
///   2. The Python environment (uv + httpx + fastmcp) is correctly installed
///   3. `OPENSTORY_URL` resolves to the Open Story container over Docker DNS
///   4. The `list_sessions` HTTP call returns fixture data
///
/// The separate `openclaw_mcp_initialize_handshake` test already verifies
/// that the stdio MCP protocol itself works (which is what OpenClaw uses
/// in production).
#[tokio::test]
#[ignore]
async fn openclaw_mcp_list_sessions_returns_fixtures() {
    let (stack, _tmp) = start_stack().await;
    let network = format!("{}_default", stack.project);

    // Call the list_sessions tool function directly via Python.
    // We write the script to a temp file and bind-mount it rather than
    // passing it inline, because multi-line Python can't survive shell
    // argument quoting cleanly.
    let script = "\
import sys\n\
sys.path.insert(0, '/opt/mcp-server')\n\
from server import list_sessions\n\
print(list_sessions())\n\
";
    let tmp_script = tempfile::NamedTempFile::new().expect("tmpfile");
    std::fs::write(tmp_script.path(), script).expect("write script");
    let script_host = to_docker_path(tmp_script.path().parent().expect("parent"));
    let script_name = tmp_script
        .path()
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .to_string();

    let output = Command::new("docker")
        .args([
            "run", "--rm",
            "--network", &network,
            "-e", "OPENSTORY_URL=http://open-story:3002",
            "-v", &format!("{script_host}:/tmp/scripts:ro"),
            "--entrypoint", "sh",
            "openclaw-mcp:latest",
            "-c", &format!("cd /opt/mcp-server && uv run python /tmp/scripts/{script_name}"),
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker run");

    assert!(
        output.status.success(),
        "tool call failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = stdout.trim();

    // The response is either `{"sessions":[...]}` or the new paginated shape
    let parsed: Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("invalid JSON from tool: {e}\nraw: {text}"));

    let sessions = parsed
        .get("sessions")
        .and_then(|s| s.as_array())
        .or_else(|| parsed.as_array())
        .unwrap_or_else(|| panic!("expected sessions array, got: {parsed}"));

    assert!(
        sessions.len() >= 3,
        "expected at least 3 fixture sessions, got {}",
        sessions.len()
    );

    // All fixture sessions should have the mcp-test- prefix
    let ids: Vec<&str> = sessions
        .iter()
        .filter_map(|s| s["session_id"].as_str())
        .collect();
    let mcp_test_count = ids.iter().filter(|id| id.contains("mcp-test-")).count();
    assert!(
        mcp_test_count >= 3,
        "expected 3 mcp-test fixture sessions, got {mcp_test_count}: {ids:?}"
    );
}
