//! TestStack — shared compose orchestration for all configuration tests.
//!
//! Supports 4 configurations: Minimal, Bus, Search, Full.
//! Uses `podman compose` (or `docker compose`) via shell for cross-platform support.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Which configuration to test.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum TestConfig {
    /// Server only — no NATS
    Minimal,
    /// Server + NATS (existing docker-compose.test.yml)
    Bus,
    /// Server + NATS (docker-compose.full.yml)
    Full,
    /// Split: publisher + NATS + consumer (docker-compose.split.yml)
    Split,
    /// Leaf cluster: hub NATS + leaf NATS + two Open Story instances
    LeafCluster,
}

/// A running test stack with discovered ports.
#[allow(dead_code)]
pub struct TestStack {
    pub compose_file: PathBuf,
    pub project_name: String,
    pub server_port: u16,
    pub publisher_port: Option<u16>,
    /// For LeafCluster: the hub server's port (common dashboard).
    pub hub_server_port: Option<u16>,
}

#[allow(dead_code)]
impl TestStack {
    pub fn base_url(&self) -> String {
        format!("http://localhost:{}", self.server_port)
    }

    /// Poll GET /api/sessions until at least one session appears.
    pub async fn wait_for_sessions(&self) {
        let url = format!("{}/api/sessions", self.base_url());
        for _ in 0..30 {
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(sessions) = resp.json::<Vec<serde_json::Value>>().await {
                    if !sessions.is_empty() {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("timed out waiting for sessions to load (15s)");
    }

    /// Poll GET /api/search until at least `min_results` results appear.
    pub async fn wait_for_search(&self, query: &str, min_results: usize, timeout: Duration) {
        let url = format!(
            "{}/api/search?q={}&limit=100",
            self.base_url(),
            urlencoding::encode(query)
        );
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(resp) = reqwest::get(&url).await {
                if resp.status() == 200 {
                    if let Ok(results) = resp.json::<Vec<serde_json::Value>>().await {
                        if results.len() >= min_results {
                            return;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!(
            "timed out waiting for {min_results} search results for '{query}' ({:.0}s)",
            timeout.as_secs_f64()
        );
    }

    /// Check if the server is healthy (HTTP 200 on /api/sessions).
    pub async fn is_healthy(&self) -> bool {
        let url = format!("{}/api/sessions", self.base_url());
        reqwest::get(&url)
            .await
            .map(|r| r.status() == 200)
            .unwrap_or(false)
    }

}

impl Drop for TestStack {
    fn drop(&mut self) {
        // Tear down compose stack on test completion
        let _ = compose_cmd(&self.compose_file, &self.project_name)
            .args(["down", "--volumes", "--remove-orphans"])
            .output();
    }
}

/// Compose file path for a given configuration.
fn compose_file(config: TestConfig) -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let filename = match config {
        TestConfig::Minimal | TestConfig::Bus => "docker-compose.test.yml",
        TestConfig::Full => "docker-compose.full.yml",
        TestConfig::Split => "docker-compose.split.yml",
        TestConfig::LeafCluster => "docker-compose.leafcluster.yml",
    };
    PathBuf::from(format!("{manifest}/tests/{filename}"))
}

/// Build a compose command targeting the right file and project.
/// Uses `docker compose` on Linux, `podman compose` on Windows.
fn compose_cmd(compose_file: &Path, project_name: &str) -> Command {
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
    // Prevent MSYS path mangling on Windows
    cmd.env("MSYS_NO_PATHCONV", "1");
    cmd
}

/// Generate a short random suffix for unique project names.
pub fn rand_suffix() -> String {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:x}", t % 0xFFFF)
}

/// Canonical, Docker-compatible path to a directory.
pub fn to_docker_path(path: &Path) -> String {
    let canonical = path.canonicalize().expect("canonicalize path");
    let s = canonical.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// Get the host port for a container service's internal port.
fn get_host_port(project_name: &str, service: &str, container_port: u16) -> Option<u16> {
    let (program, first_arg) = if cfg!(target_os = "windows") {
        ("podman", "compose")
    } else {
        ("docker", "compose")
    };
    let output = Command::new(program)
        .args([first_arg, "-p", project_name, "port", service, &container_port.to_string()])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "0.0.0.0:12345\n"
    stdout.trim().rsplit(':').next()?.parse().ok()
}

/// Start a compose stack for the given configuration.
///
/// Mounts `fixture_dir` at /watch in the server container.
/// Returns a TestStack with discovered ports.
#[allow(dead_code)]
pub async fn start_stack(config: TestConfig, fixture_dir: &Path) -> TestStack {
    // Touch fixture files for fresh mtimes
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(fixture_dir).expect("read fixture dir") {
        let path = entry.expect("read entry").path();
        let _ = filetime::set_file_mtime(&path, now);
    }

    let fixture_path = to_docker_path(fixture_dir);
    let compose_path = compose_file(config);
    // Unique project name per test invocation (PID + random suffix)
    let project_name = format!(
        "ostest-{}-{}-{}",
        match config {
            TestConfig::Minimal => "min",
            TestConfig::Bus => "bus",
            TestConfig::Full => "full",
            TestConfig::Split => "split",
            TestConfig::LeafCluster => "leaf",
        },
        std::process::id(),
        rand_suffix()
    );

    // Start the stack
    let output = compose_cmd(&compose_path, &project_name)
        .args(["up", "-d"])
        .env("FIXTURE_DIR", &fixture_path)
        .output()
        .expect("failed to run podman compose up");

    assert!(
        output.status.success(),
        "podman compose up failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Discover ports
    // Wait a moment for containers to start and ports to be assigned
    tokio::time::sleep(Duration::from_secs(2)).await;

    // For split mode, the "consumer" service serves the API.
    // For leaf cluster, the "leaf-server" is the primary (watches fixtures).
    // Otherwise "server".
    let api_service = match config {
        TestConfig::Split => "consumer",
        TestConfig::LeafCluster => "leaf-server",
        _ => "server",
    };

    let server_port = get_host_port(&project_name, api_service, 3002)
        .expect("failed to get server port");

    let publisher_port = match config {
        TestConfig::Split => {
            Some(get_host_port(&project_name, "publisher", 3002)
                .expect("failed to get publisher port"))
        }
        _ => None,
    };

    let hub_server_port = match config {
        TestConfig::LeafCluster => {
            Some(get_host_port(&project_name, "hub-server", 3002)
                .expect("failed to get hub-server port"))
        }
        _ => None,
    };

    // Wait for HTTP readiness
    let health_path = match config {
        TestConfig::Split => "/health",
        _ => "/api/sessions",
    };
    let url = format!("http://localhost:{server_port}{health_path}");
    for _ in 0..30 {
        if reqwest::get(&url).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // For leaf cluster, also wait for hub server
    if let Some(hub_port) = hub_server_port {
        let hub_url = format!("http://localhost:{hub_port}/api/sessions");
        for _ in 0..30 {
            if reqwest::get(&hub_url).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    TestStack {
        compose_file: compose_path,
        project_name,
        server_port,
        publisher_port,
        hub_server_port,
    }
}
