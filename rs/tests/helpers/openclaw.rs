//! OpenClaw + Open Story compose test helper.
//!
//! Starts an OpenClaw gateway and Open Story server in Docker containers
//! with a shared volume for JSONL session files. Provides methods to
//! send messages to OpenClaw and verify they appear in Open Story.
//!
//! Requires:
//!   - `openclaw:test` Docker image (build from ~/projects/openclaw)
//!   - `open-story:test` Docker image (build from ./rs)
//!   - `ANTHROPIC_API_KEY` environment variable

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::compose::rand_suffix;

/// A running OpenClaw + Open Story test stack.
pub struct OpenClawStack {
    pub compose_file: PathBuf,
    pub project_name: String,
    pub openstory_port: u16,
}

impl OpenClawStack {
    pub fn openstory_url(&self) -> String {
        format!("http://localhost:{}", self.openstory_port)
    }

    /// Send a message to the OpenClaw gateway agent via CLI.
    ///
    /// Uses `docker exec` to run the CLI inside the OpenClaw container,
    /// which connects to the gateway over the internal WebSocket protocol.
    pub async fn send_agent_message(&self, message: &str) {
        let container_name = format!("{}-openclaw-1", self.project_name);
        let output = std::process::Command::new("docker")
            .args([
                "exec",
                &container_name,
                "node",
                "dist/index.js",
                "agent",
                "--agent",
                "main",
                "--json",
                "--message",
                message,
            ])
            .env(
                "ANTHROPIC_API_KEY",
                std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            )
            .output()
            .expect("failed to exec CLI in OpenClaw container");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("CLI stdout: {}", &stdout[..stdout.len().min(500)]);
        eprintln!("CLI stderr: {}", &stderr[..stderr.len().min(500)]);
    }

    /// Poll Open Story until at least one session appears.
    pub async fn wait_for_openstory_session(&self, timeout: Duration) {
        let url = format!("{}/api/sessions", self.openstory_url());
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(sessions) = resp.json::<Vec<Value>>().await {
                    if !sessions.is_empty() {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        panic!(
            "timed out waiting for OpenClaw session in Open Story ({:.0}s)",
            timeout.as_secs_f64()
        );
    }

    /// Get all sessions from Open Story.
    pub async fn get_openstory_sessions(&self) -> Vec<Value> {
        let url = format!("{}/api/sessions", self.openstory_url());
        reqwest::get(&url)
            .await
            .expect("failed to get sessions")
            .json()
            .await
            .expect("invalid JSON")
    }

    /// Get events for a session from Open Story.
    pub async fn get_openstory_events(&self, session_id: &str) -> Vec<Value> {
        let url = format!("{}/api/sessions/{}/events", self.openstory_url(), session_id);
        reqwest::get(&url)
            .await
            .expect("failed to get events")
            .json()
            .await
            .expect("invalid JSON")
    }

    /// Get view records for a session from Open Story.
    pub async fn get_openstory_view_records(&self, session_id: &str) -> Vec<Value> {
        let url = format!(
            "{}/api/sessions/{}/view-records",
            self.openstory_url(),
            session_id
        );
        reqwest::get(&url)
            .await
            .expect("failed to get view records")
            .json()
            .await
            .expect("invalid JSON")
    }

    /// Dump the raw JSONL session files from inside the OpenClaw container.
    pub async fn dump_openclaw_sessions(&self) {
        let container_name = format!("{}-openclaw-1", self.project_name);

        // Find session files
        let output = std::process::Command::new("docker")
            .args([
                "exec",
                &container_name,
                "find",
                "/home/node/.openclaw/agents/",
                "-name",
                "*.jsonl",
            ])
            .output();

        if let Ok(output) = output {
            let files = String::from_utf8_lossy(&output.stdout);
            for file in files.lines() {
                let file = file.trim();
                if file.is_empty() {
                    continue;
                }
                eprintln!("  \x1b[2m{file}\x1b[0m");

                // Show first few lines
                if let Ok(cat_output) = std::process::Command::new("docker")
                    .args(["exec", &container_name, "head", "-5", file])
                    .output()
                {
                    let content = String::from_utf8_lossy(&cat_output.stdout);
                    for line in content.lines() {
                        // Pretty-print truncated JSON
                        let truncated = if line.len() > 120 {
                            format!("{}...", &line[..120])
                        } else {
                            line.to_string()
                        };
                        eprintln!("    {truncated}");
                    }
                }
                eprintln!();
            }
        }
    }
}

impl Drop for OpenClawStack {
    fn drop(&mut self) {
        let _ = compose_cmd(&self.compose_file, &self.project_name)
            .args(["down", "--volumes", "--remove-orphans"])
            .output();
    }
}

fn compose_cmd(compose_file: &std::path::Path, project_name: &str) -> Command {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("podman");
        c.arg("compose");
        c
    } else {
        let mut c = Command::new("docker");
        c.arg("compose");
        c
    };
    cmd.args(["-f"])
        .arg(compose_file)
        .args(["-p", project_name]);
    cmd.env("MSYS_NO_PATHCONV", "1");
    cmd
}

fn get_host_port(project_name: &str, service: &str, container_port: u16) -> Option<u16> {
    let (program, first_arg) = if cfg!(target_os = "windows") {
        ("podman", "compose")
    } else {
        ("docker", "compose")
    };
    let output = Command::new(program)
        .args([
            first_arg,
            "-p",
            project_name,
            "port",
            service,
            &container_port.to_string(),
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().rsplit(':').next()?.parse().ok()
}

/// Start the OpenClaw + Open Story compose stack.
///
/// Requires `ANTHROPIC_API_KEY` in the environment.
/// Both images must be pre-built: `openclaw:test` and `open-story:test`.
#[allow(dead_code)]
pub async fn start_openclaw_stack() -> OpenClawStack {
    // Verify API key is set
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        panic!("ANTHROPIC_API_KEY must be set to run OpenClaw integration tests");
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let compose_path = manifest_dir.join("tests/docker-compose.openclaw-integration.yml");
    let config_path = manifest_dir.join("tests/fixtures/openclaw-test-config.json");
    let project_name = format!("ostest-openclaw-{}-{}", std::process::id(), rand_suffix());

    // Start the stack
    let output = compose_cmd(&compose_path, &project_name)
        .args(["up", "-d"])
        .env(
            "ANTHROPIC_API_KEY",
            std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
        )
        .env("OPENCLAW_CONFIG", config_path.to_string_lossy().to_string())
        .output()
        .expect("failed to run docker compose up");

    if !output.status.success() {
        panic!(
            "docker compose up failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    // Wait for containers to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    let openstory_port =
        get_host_port(&project_name, "open-story", 3002).expect("failed to get Open Story port");

    // Wait for Open Story HTTP readiness
    let url = format!("http://localhost:{openstory_port}/api/sessions");
    for _ in 0..30 {
        if reqwest::get(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    OpenClawStack {
        compose_file: compose_path,
        project_name,
        openstory_port,
    }
}
