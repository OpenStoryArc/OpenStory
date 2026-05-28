//! Faithful lab federation — models the REAL deployment, not just hub fan-in.
//!
//! Each node is a full machine: its own NATS **leaf** (durable local
//! JetStream, federates to the hub over a leaf connection) plus an OpenStory in
//! `--role full` (watches local transcripts, publishes to its leaf, AND
//! consumes back — a complete local mirror with its own dashboard). The hub is
//! a NATS hub + an OpenStory consumer (the common dashboard).
//!
//! This is heavier than test_federation_scale (≈2 containers/node + hub) but it
//! exercises what the lightweight publisher model abstracts away: **bidirectional
//! convergence** — JetStream propagates both ways, so every node must end up
//! mirroring EVERY node's session, not just its own. That "every machine sees
//! all team data" property is the lab's actual promise; this test asserts it at
//! scale, not just on the 1-leaf topology of test_leaf_cluster.
//!
//!   docker build -t open-story:test ./rs
//!   cargo test -p open-story --test test_federation_lab -- --ignored --nocapture
//!
//! #[ignore] — container-heavy (~2N+2 containers).

mod helpers;

use helpers::synth;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const TOKEN: &str = "test-cluster-token";

struct Lab {
    project: String,
    compose_file: PathBuf,
    _fixtures: tempfile::TempDir,
}

impl Drop for Lab {
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
    p.canonicalize().expect("canonicalize").to_string_lossy().replace('\\', "/")
}

fn generate_node_fixtures(root: &Path, n: usize, events: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let dir = root.join(format!("node-{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            let sid = format!("node-{i}");
            std::fs::write(dir.join(format!("{sid}.jsonl")), synth::generate_session(&sid, events, 0))
                .unwrap();
            dir
        })
        .collect()
}

/// A NATS hub: client port 4222 (token auth) + leaf listener 7422.
fn hub_nats_service() -> String {
    format!(
        "  nats-hub:\n    image: nats:2-alpine\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'EOF'\n        listen: 0.0.0.0:4222\n        jetstream {{ store_dir: /data/jetstream, max_mem: 256MB, max_file: 2GB }}\n        authorization {{ token: \"{TOKEN}\" }}\n        leafnodes {{ listen: \"0.0.0.0:7422\" }}\n        EOF\n        exec nats-server -c /tmp/nats.conf\n"
    )
}

/// A per-node NATS leaf: local JetStream, federates up to the hub at :7422.
fn leaf_nats_service(i: usize) -> String {
    format!(
        "  nats-leaf-{i}:\n    image: nats:2-alpine\n    depends_on:\n      - nats-hub\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'EOF'\n        listen: 0.0.0.0:4222\n        jetstream {{ store_dir: /data/jetstream, max_mem: 256MB, max_file: 2GB }}\n        leafnodes {{ remotes [ {{ url: \"nats://{TOKEN}@nats-hub:7422\" }} ] }}\n        EOF\n        exec nats-server -c /tmp/nats.conf\n"
    )
}

fn generate_lab_compose(node_dirs: &[PathBuf], catch_up: bool) -> String {
    let mut yaml = String::from("services:\n");
    yaml.push_str(&hub_nats_service());
    // Hub dashboard: consumer on the hub NATS.
    yaml.push_str(&format!(
        "  hub:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"consumer\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://{TOKEN}@nats-hub:4222\", \"--data-dir\", \"/data\"]\n    ports:\n      - \"3002\"\n    depends_on:\n      - nats-hub\n"
    ));
    // Optional env block — when catch_up is off we want a *pure transport*
    // convergence test (Phase 2b RED). When on, the HTTP catch-up backstop
    // shipped in `024fcc2` papers over the JetStream cold-boot race.
    let env_block = if catch_up {
        "    environment:\n      - OPEN_STORY_CATCH_UP_PEER=http://hub:3002\n"
    } else {
        ""
    };
    for (i, dir) in node_dirs.iter().enumerate() {
        yaml.push_str(&leaf_nats_service(i));
        // Full node: watches its own fixture, publishes to + consumes from its
        // own leaf NATS (a complete local mirror), serves a local dashboard.
        yaml.push_str(&format!(
            "  node-{i}:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"full\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://nats-leaf-{i}:4222\", \"--data-dir\", \"/data\", \"--watch-dir\", \"/watch\"]\n{env_block}    ports:\n      - \"3002\"\n    volumes:\n      - {mount}:/watch:ro\n    depends_on:\n      - nats-leaf-{i}\n",
            mount = docker_path(dir),
        ));
    }
    yaml
}

fn service_port(project: &str, compose_file: &Path, service: &str) -> Option<u16> {
    let out = Command::new("docker")
        .args([
            "compose",
            "-f",
            &compose_file.to_string_lossy(),
            "-p",
            project,
            "port",
            service,
            "3002",
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().rsplit(':').next()?.parse().ok()
}

async fn session_ids(port: u16) -> BTreeSet<String> {
    let url = format!("http://localhost:{port}/api/sessions");
    let body: Value = match reqwest::get(&url).await {
        Ok(r) => r.json().await.unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    body.get("sessions")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s["session_id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug)]
struct LabResult {
    nodes: usize,
    hub_has: usize,
    min_node_has: usize,
    fully_mirrored: bool,
    elapsed: Duration,
}

async fn run_lab_federation(nodes: usize, events: usize, catch_up: bool) -> LabResult {
    let label = if catch_up { "faithful, catch-up ON" } else { "faithful, catch-up OFF (cold)" };
    eprintln!("\n  ══ Lab federation ({label}): {nodes} nodes × {events} events ══");
    let fixtures = tempfile::TempDir::new().unwrap();
    let node_dirs = generate_node_fixtures(fixtures.path(), nodes, events);
    let compose_file = fixtures.path().join("docker-compose.lab.yml");
    std::fs::write(&compose_file, generate_lab_compose(&node_dirs, catch_up)).unwrap();

    let project = format!("oslab-{nodes}-{}", std::process::id());
    let lab = Lab {
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
        .expect("compose up");
    if !up.status.success() {
        eprintln!(
            "  compose up FAILED:\n  {}",
            String::from_utf8_lossy(&up.stderr).lines().take(8).collect::<Vec<_>>().join("\n  ")
        );
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    }

    // Discover hub + every node port (retry while compose wires up).
    let mut hub_port = None;
    for _ in 0..40 {
        if let Some(p) = service_port(&project, &compose_file, "hub") {
            hub_port = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let Some(hub_port) = hub_port else {
        eprintln!("  never discovered hub port");
        let ps = Command::new("docker")
            .args(["compose", "-f", &compose_file.to_string_lossy(), "-p", &project, "ps", "-a"])
            .env("MSYS_NO_PATHCONV", "1")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        eprintln!("  DEBUG ps:\n{ps}");
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    };
    let node_ports: Vec<u16> = (0..nodes)
        .filter_map(|i| service_port(&project, &compose_file, &format!("node-{i}")))
        .collect();

    let expected: BTreeSet<String> = (0..nodes).map(|i| format!("node-{i}")).collect();

    // Converged when the hub AND every node mirror all N sessions.
    let timeout = Duration::from_secs((60 + nodes as u64 * 6).min(180));
    let deadline = Instant::now() + timeout;
    let mut hub_has = 0;
    let mut min_node_has = 0;
    let mut last_log = Instant::now();
    loop {
        let hub = session_ids(hub_port).await;
        hub_has = hub.len();
        let mut min = usize::MAX;
        for &p in &node_ports {
            min = min.min(session_ids(p).await.len());
        }
        min_node_has = if node_ports.is_empty() { 0 } else { min };

        let hub_full = expected.is_subset(&hub);
        if hub_full && min_node_has >= nodes {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        if last_log.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "    +{:>4.0}s: hub {hub_has}/{nodes}, slowest node {min_node_has}/{nodes}",
                started.elapsed().as_secs_f64()
            );
            last_log = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let elapsed = started.elapsed();
    let fully_mirrored = hub_has >= nodes && min_node_has >= nodes;
    eprintln!(
        "  {nodes} nodes → hub {hub_has}/{nodes}, slowest node {min_node_has}/{nodes} in {:.1}s, fully_mirrored={fully_mirrored}",
        elapsed.as_secs_f64()
    );
    drop(lab);
    LabResult { nodes, hub_has, min_node_has, fully_mirrored, elapsed }
}

/// 10 faithful nodes: hub AND every node must mirror all 10 sessions
/// (bidirectional JetStream — the lab's "every machine sees all team data").
#[tokio::test]
#[ignore]
async fn lab_federation_full_mirror_10_nodes() {
    let r = run_lab_federation(10, 12, true).await;
    assert!(
        r.fully_mirrored,
        "every node must mirror all {} sessions (hub {}/{}, slowest node {}/{})",
        r.nodes, r.hub_has, r.nodes, r.min_node_has, r.nodes
    );
}

/// T2 cold-boot — Phase 2b RED. Same 10-node star as above but with the
/// HTTP catch-up backstop disabled, so convergence depends purely on
/// JetStream transport. Per `federation-bidirectional-mirror-gap`, today
/// this gets ~8/10 on the slowest leaf — the gap Idea A's cross-domain
/// I/O wrapper is meant to close. Goes green when Phase 2b Step 2 lands.
#[tokio::test]
#[ignore]
async fn lab_federation_full_mirror_10_nodes_cold() {
    let r = run_lab_federation(10, 12, false).await;
    assert!(
        r.fully_mirrored,
        "cold boot (no catch-up): every node must mirror all {} sessions (hub {}/{}, slowest node {}/{})",
        r.nodes, r.hub_has, r.nodes, r.min_node_has, r.nodes
    );
}

/// Ramp the faithful topology until full mirroring breaks — the single-host
/// ceiling for the real lab shape (≈2 containers/node).
#[tokio::test]
#[ignore]
async fn lab_federation_ramp() {
    let mut rows = Vec::new();
    for &n in &[2usize, 3, 5, 10] {
        let r = run_lab_federation(n, 12, true).await;
        rows.push((n, r.hub_has, r.min_node_has, r.fully_mirrored, r.elapsed.as_secs_f64()));
        if !r.fully_mirrored {
            eprintln!("  *** full mirroring broke at {n} faithful nodes ***");
            break;
        }
    }
    eprintln!("\n  ══ Lab federation summary ══");
    eprintln!("  {:>6} {:>8} {:>10} {:>10} {:>8}", "Nodes", "Hub", "SlowNode", "Mirrored", "Time(s)");
    for (n, hub, node, ok, secs) in &rows {
        eprintln!("  {n:>6} {hub:>8} {node:>10} {:>10} {secs:>8.1}", ok);
    }
    assert!(rows.first().map(|r| r.3).unwrap_or(false), "5 faithful nodes must fully mirror");
}
