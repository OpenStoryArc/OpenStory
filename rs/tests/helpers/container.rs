//! Testcontainers helper for open-story integration tests.
//!
//! Spins up the open-story server in a Docker container with fixture data
//! mounted at /watch. The container is disposable — fresh state per test.
//!
//! Build the image first: `docker build -t open-story:test ./rs`

use std::path::{Path, PathBuf};
use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{ContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// Default image name — build with: `docker build -t open-story:test ./rs`
const IMAGE_NAME: &str = "open-story";
const IMAGE_TAG: &str = "test";
const CONTAINER_PORT: u16 = 3002;

/// A running open-story container with its mapped host port.
pub struct OpenStoryContainer {
    pub container: ContainerAsync<GenericImage>,
    pub host_port: u16,
}

impl OpenStoryContainer {
    /// Base URL for HTTP requests (e.g., "http://localhost:32789").
    pub fn base_url(&self) -> String {
        format!("http://localhost:{}", self.host_port)
    }

    /// WebSocket URL (e.g., "ws://localhost:32789/ws").
    pub fn ws_url(&self) -> String {
        format!("ws://localhost:{}/ws", self.host_port)
    }

    /// Poll GET /api/sessions until at least one session is loaded.
    ///
    /// The server starts accepting HTTP before the watcher finishes backfilling.
    /// This method retries until sessions appear or the timeout is reached.
    pub async fn wait_for_sessions(&self) {
        let url = format!("{}/api/sessions", self.base_url());
        for _ in 0..40 {
            if let Ok(resp) = reqwest::get(&url).await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    // API returns either {"sessions": [...]} or [...] depending on version
                    let sessions = body.get("sessions")
                        .and_then(|v| v.as_array())
                        .or_else(|| body.as_array());
                    if let Some(arr) = sessions {
                        if !arr.is_empty() {
                            return;
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("timed out waiting for sessions to load (20s)");
    }
}

/// Start an open-story container with the given fixture directory mounted at /watch.
///
/// The fixture dir should contain JSONL session files. The container starts,
/// loads the fixtures via the file watcher, and becomes ready when
/// GET /api/sessions returns HTTP 200.
///
/// **Note:** The HTTP health check passes before the watcher finishes backfilling.
/// Call `wait_for_sessions()` before querying session data.
///
/// # Panics
///
/// Panics if the Docker image `open-story:test` doesn't exist.
/// Build it first: `docker build -t open-story:test ./rs`
pub async fn start_open_story(fixture_dir: &Path) -> OpenStoryContainer {
    // Touch fixture files so they have fresh mtimes. The watcher's backfill
    // window (24h) skips old files, and committed fixtures may be days old.
    // filetime updates metadata only — git tracks content, not mtime.
    let now = filetime::FileTime::now();
    for entry in std::fs::read_dir(fixture_dir).expect("failed to read fixture dir") {
        let path = entry.expect("failed to read fixture entry").path();
        let _ = filetime::set_file_mtime(&path, now);
    }

    let fixture_path = fixture_dir
        .canonicalize()
        .expect("fixture dir must exist");

    // Docker on Windows needs forward-slash paths without the \\?\ UNC prefix
    let mount_source = to_docker_path(&fixture_path);

    let container = GenericImage::new(IMAGE_NAME, IMAGE_TAG)
        .with_exposed_port(ContainerPort::Tcp(CONTAINER_PORT))
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/api/sessions")
                .with_port(ContainerPort::Tcp(CONTAINER_PORT))
                .with_expected_status_code(200u16),
        ))
        .with_mount(Mount::bind_mount(mount_source, "/watch"))
        .start()
        .await
        .expect("failed to start open-story container");

    let host_port = container
        .get_host_port_ipv4(CONTAINER_PORT)
        .await
        .expect("failed to get mapped port");

    OpenStoryContainer {
        container,
        host_port,
    }
}

/// Convert a Windows path to a Docker-compatible bind mount path.
///
/// Strips the `\\?\` UNC prefix from canonicalized paths and converts
/// backslashes to forward slashes. On non-Windows, returns the path as-is.
fn to_docker_path(path: &PathBuf) -> String {
    let s = path.to_string_lossy().to_string();

    // Strip \\?\ UNC prefix that Windows canonicalize() adds
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);

    // Convert backslashes to forward slashes for Docker
    s.replace('\\', "/")
}
