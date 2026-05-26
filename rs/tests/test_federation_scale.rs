//! Federation scale tests — can ONE hub aggregate a network of N nodes?
//!
//! Topology is GENERATED (you can't hand-write 100 services): one NATS broker,
//! one hub in `--role consumer` (aggregates + serves the API), and N nodes in
//! `--role publisher` (each watches its own one-session fixture and forwards to
//! the broker, no local consume/API — the lightweight faithful unit for testing
//! hub fan-in). The real per-machine leaf-NATS topology is validated separately
//! at small N by test_leaf_cluster; here we push node COUNT to find the ceiling.
//!
//! Asserts the hub aggregates every node's session (completeness at scale) and
//! reports time-to-converge. Run the ramp to find where a single host breaks:
//!
//!   docker build -t open-story:test ./rs
//!   cargo test -p open-story --test test_federation_scale -- --ignored --nocapture
//!
//! #[ignore] — slow and container-heavy.

mod helpers;

use helpers::synth;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Cleans up the generated compose project on drop, even if a test panics.
struct Federation {
    project: String,
    compose_file: PathBuf,
    _fixtures: tempfile::TempDir,
}

impl Drop for Federation {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args([
                "compose",
                "-f",
                &self.compose_file.to_string_lossy(),
                "-p",
                &self.project,
                "down",
                "-v",
                "--remove-orphans",
            ])
            .env("MSYS_NO_PATHCONV", "1")
            .output();
    }
}

fn docker_path(p: &Path) -> String {
    let c = p.canonicalize().expect("canonicalize");
    c.to_string_lossy().replace('\\', "/")
}

/// Generate N per-node fixture dirs, each with a single unique session file.
fn generate_node_fixtures(root: &Path, n: usize, events_per_node: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let dir = root.join(format!("node-{i}"));
            std::fs::create_dir_all(&dir).expect("mkdir node dir");
            let sid = format!("node-{i}");
            let content = synth::generate_session(&sid, events_per_node, 0);
            std::fs::write(dir.join(format!("{sid}.jsonl")), content).expect("write fixture");
            dir
        })
        .collect()
}

/// Build a compose YAML: nats + hub(consumer) + N node(publisher) services.
fn generate_compose(node_dirs: &[PathBuf]) -> String {
    let mut yaml = String::from(
        "services:\n  \
         nats:\n    image: nats:2-alpine\n    command: [\"--jetstream\", \"--store_dir\", \"/data/jetstream\"]\n  \
         hub:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"consumer\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://nats:4222\", \"--data-dir\", \"/data\"]\n    ports:\n      - \"3002\"\n    depends_on:\n      - nats\n",
    );
    for (i, dir) in node_dirs.iter().enumerate() {
        let mount = docker_path(dir);
        yaml.push_str(&format!(
            "  node-{i}:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"publisher\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://nats:4222\", \"--watch-dir\", \"/watch\"]\n    volumes:\n      - {mount}:/watch:ro\n    depends_on:\n      - nats\n",
        ));
    }
    yaml
}

fn hub_port(project: &str, compose_file: &Path) -> Option<u16> {
    let out = Command::new("docker")
        .args([
            "compose",
            "-f",
            &compose_file.to_string_lossy(),
            "-p",
            project,
            "port",
            "hub",
            "3002",
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .rsplit(':')
        .next()?
        .parse()
        .ok()
}

#[derive(Debug)]
struct FederationResult {
    nodes: usize,
    sessions_found: usize,
    elapsed: Duration,
    converged: bool,
    healthy: bool,
}

async fn run_federation(nodes: usize, events_per_node: usize) -> FederationResult {
    eprintln!("\n  ══ Federation: {nodes} nodes × {events_per_node} events ══");
    let fixtures = tempfile::TempDir::new().expect("tempdir");
    let node_dirs = generate_node_fixtures(fixtures.path(), nodes, events_per_node);

    let compose_file = fixtures.path().join("docker-compose.federation.yml");
    std::fs::write(&compose_file, generate_compose(&node_dirs)).expect("write compose");

    let project = format!("osfed-{nodes}-{}", std::process::id());
    let fed = Federation {
        project: project.clone(),
        compose_file: compose_file.clone(),
        _fixtures: fixtures,
    };

    let started = Instant::now();
    let up = Command::new("docker")
        .args([
            "compose",
            "-f",
            &compose_file.to_string_lossy(),
            "-p",
            &project,
            "up",
            "-d",
            "--remove-orphans",
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("docker compose up");
    if !up.status.success() {
        eprintln!(
            "  compose up FAILED: {}",
            String::from_utf8_lossy(&up.stderr)
                .lines()
                .take(6)
                .collect::<Vec<_>>()
                .join("\n  ")
        );
        return FederationResult {
            nodes,
            sessions_found: 0,
            elapsed: started.elapsed(),
            converged: false,
            healthy: false,
        };
    }

    // Discover the hub port (retry — compose may still be wiring).
    let mut port = None;
    for _ in 0..30 {
        if let Some(p) = hub_port(&project, &compose_file) {
            port = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let Some(port) = port else {
        eprintln!("  never discovered hub port");
        return FederationResult {
            nodes,
            sessions_found: 0,
            elapsed: started.elapsed(),
            converged: false,
            healthy: false,
        };
    };

    // Poll the hub until it has aggregated all N sessions, or timeout. Budget
    // scales with node count (each publisher boots + backfills its fixture).
    let url = format!("http://localhost:{port}/api/sessions");
    let timeout = Duration::from_secs((120 + nodes as u64 * 12).min(900));
    let deadline = Instant::now() + timeout;
    let mut sessions_found = 0;
    let mut last_log = Instant::now();
    while Instant::now() < deadline {
        if let Ok(resp) = reqwest::get(&url).await {
            if let Ok(body) = resp.json::<Value>().await {
                let arr = body
                    .get("sessions")
                    .and_then(|s| s.as_array())
                    .cloned()
                    .unwrap_or_default();
                sessions_found = arr.len();
                if sessions_found >= nodes {
                    break;
                }
            }
        }
        // Progress trace — climbing means slow convergence; a plateau below
        // `nodes` means events were dropped (hard miss), not just slow.
        if last_log.elapsed() >= Duration::from_secs(10) {
            eprintln!(
                "    +{:>4.0}s: {sessions_found}/{nodes} aggregated",
                started.elapsed().as_secs_f64()
            );
            last_log = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let elapsed = started.elapsed();

    // Completeness: are all node-i sessions present on the hub?
    let healthy = reqwest::get(&url).await.map(|r| r.status() == 200).unwrap_or(false);
    let mut present = std::collections::BTreeSet::new();
    if let Ok(resp) = reqwest::get(&url).await {
        if let Ok(body) = resp.json::<Value>().await {
            if let Some(arr) = body.get("sessions").and_then(|s| s.as_array()) {
                for s in arr {
                    if let Some(id) = s["session_id"].as_str() {
                        present.insert(id.to_string());
                    }
                }
            }
        }
    }
    let converged = (0..nodes).all(|i| present.contains(&format!("node-{i}")));

    eprintln!(
        "  {nodes} nodes → hub aggregated {sessions_found}/{nodes} sessions in {:.1}s, converged={converged}, healthy={healthy}",
        elapsed.as_secs_f64()
    );
    drop(fed); // explicit teardown before returning
    FederationResult {
        nodes,
        sessions_found,
        elapsed,
        converged,
        healthy,
    }
}

/// A 10-node federation: the hub must aggregate every node's session.
#[tokio::test]
#[ignore]
async fn federation_converges_10_nodes() {
    let r = run_federation(10, 12).await;
    assert!(r.healthy, "hub should be healthy");
    assert!(
        r.converged,
        "hub must aggregate all 10 node sessions (found {}/{})",
        r.sessions_found, r.nodes
    );
}

/// Ramp node count until the hub fails to aggregate them all in time — the
/// single-host federation ceiling. Each round generates a fresh stack.
#[tokio::test]
#[ignore]
async fn federation_scale_ramp() {
    let mut results = Vec::new();
    for &n in &[10usize, 25, 50, 100] {
        let r = run_federation(n, 12).await;
        let broke = !r.converged || !r.healthy;
        results.push((n, r.sessions_found, r.elapsed.as_secs_f64(), r.converged, r.healthy));
        if broke {
            eprintln!("  *** federation did not fully converge at {n} nodes ***");
            break;
        }
    }
    eprintln!("\n  ══ Federation scale summary ══");
    eprintln!("  {:>6} {:>10} {:>9} {:>9} {:>8}", "Nodes", "Found", "Time(s)", "Converged", "Healthy");
    for (n, found, secs, conv, healthy) in &results {
        eprintln!("  {n:>6} {found:>10} {secs:>9.1} {:>9} {:>8}", conv, healthy);
    }
    assert!(results.first().map(|r| r.3).unwrap_or(false), "10 nodes must converge");
}
