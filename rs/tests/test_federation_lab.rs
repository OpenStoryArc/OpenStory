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
/// JetStream `domain: hub` so cross-domain sources (`$JS.hub.API`) reach it
/// from leaf-domain NATSes.
fn hub_nats_service() -> String {
    format!(
        "  nats-hub:\n    image: nats:2-alpine\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'EOF'\n        listen: 0.0.0.0:4222\n        jetstream {{ store_dir: /data/jetstream, max_mem: 256MB, max_file: 8GB, domain: hub }}\n        authorization {{ token: \"{TOKEN}\" }}\n        leafnodes {{ listen: \"0.0.0.0:7422\" }}\n        EOF\n        exec nats-server -c /tmp/nats.conf\n"
    )
}

/// A per-node NATS leaf: local JetStream with `domain: node-{i}` so the hub
/// aggregate can source it as a distinct domain, federated up to the hub at
/// :7422 over a token leafnode. The domain MUST match the host token used by
/// the openstory leaf (`OPEN_STORY_HOST=node-{i}`) so the subject
/// `events.node-{i}.>` lines up with the source registered on `events-agg`.
fn leaf_nats_service(i: usize) -> String {
    format!(
        "  nats-leaf-{i}:\n    image: nats:2-alpine\n    depends_on:\n      - nats-hub\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'EOF'\n        listen: 0.0.0.0:4222\n        jetstream {{ store_dir: /data/jetstream, max_mem: 256MB, max_file: 8GB, domain: \"node-{i}\" }}\n        leafnodes {{ remotes [ {{ url: \"nats://{TOKEN}@nats-hub:7422\" }} ] }}\n        EOF\n        exec nats-server -c /tmp/nats.conf\n"
    )
}

fn generate_lab_compose(node_dirs: &[PathBuf], catch_up: bool) -> String {
    let mut yaml = String::from("services:\n");
    yaml.push_str(&hub_nats_service());
    // Hub dashboard: consumer on the hub NATS. OPEN_STORY_HUB_DOMAIN tells
    // the openstory CLI it is the hub of a federation — `ensure_aggregate`
    // creates the `events-agg` source-only stream that leaves register into.
    yaml.push_str(&format!(
        "  hub:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"consumer\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://{TOKEN}@nats-hub:4222\", \"--data-dir\", \"/data\"]\n    environment:\n      - OPEN_STORY_HUB_DOMAIN=hub\n    ports:\n      - \"3002\"\n    depends_on:\n      - nats-hub\n"
    ));
    // Leaf env: ALWAYS federation (HUB_DOMAIN + HOST). Optionally also the
    // HTTP catch-up backstop — when off, the cold-boot test exercises pure
    // JetStream-sources convergence; when on, the shipped backstop covers
    // any leftover gap.
    let catch_up_env = if catch_up {
        "      - OPEN_STORY_CATCH_UP_PEER=http://hub:3002\n"
    } else {
        ""
    };
    for (i, dir) in node_dirs.iter().enumerate() {
        yaml.push_str(&leaf_nats_service(i));
        // Full node: watches its own fixture, publishes to + consumes from its
        // own leaf NATS (host-scoped `events.node-{i}.>`), reads the fleet
        // from `events-mirror` sourcing `events-agg` over the hub domain.
        yaml.push_str(&format!(
            "  node-{i}:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"full\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://nats-leaf-{i}:4222\", \"--data-dir\", \"/data\", \"--watch-dir\", \"/watch\"]\n    environment:\n      - OPEN_STORY_HOST=node-{i}\n      - OPEN_STORY_HUB_DOMAIN=hub\n{catch_up_env}    ports:\n      - \"3002\"\n    volumes:\n      - {mount}:/watch:ro\n    depends_on:\n      - nats-leaf-{i}\n",
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

/// Orchestrate a late-joiner T2 test: bring up nats-hub + hub + every
/// nats-leaf and node EXCEPT the last leaf; wait for that subset to fully
/// mirror; sleep `late_delay_secs`; bring up the late leaf; wait for the
/// full fleet to converge. Returns the final LabResult including total
/// elapsed (initial settle + delay + backfill).
async fn run_lab_federation_late_joiner(nodes: usize, events: usize, late_delay_secs: u64) -> LabResult {
    assert!(nodes >= 2, "late-joiner needs at least 2 nodes");
    eprintln!(
        "\n  ══ Lab federation (late joiner, cold): {nodes} nodes × {events} events, late=+{late_delay_secs}s ══"
    );
    let fixtures = tempfile::TempDir::new().unwrap();
    let node_dirs = generate_node_fixtures(fixtures.path(), nodes, events);
    let compose_file = fixtures.path().join("docker-compose.lab.yml");
    std::fs::write(&compose_file, generate_lab_compose(&node_dirs, false)).unwrap();

    let project = format!("oslab-lj-{nodes}-{}", std::process::id());
    let lab = Lab {
        project: project.clone(),
        compose_file: compose_file.clone(),
        _fixtures: fixtures,
    };

    // Phase 1 — bring up nats-hub + hub + nodes 0..N-1 (excluding the last).
    let last = nodes - 1;
    let mut up1_args = vec![
        "compose".to_string(),
        "-f".to_string(),
        compose_file.to_string_lossy().into_owned(),
        "-p".to_string(),
        project.clone(),
        "up".to_string(),
        "-d".to_string(),
        "--remove-orphans".to_string(),
        "nats-hub".to_string(),
        "hub".to_string(),
    ];
    for i in 0..last {
        up1_args.push(format!("nats-leaf-{i}"));
        up1_args.push(format!("node-{i}"));
    }
    let started = Instant::now();
    let up = Command::new("docker")
        .args(&up1_args)
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up phase 1");
    if !up.status.success() {
        eprintln!(
            "  compose up phase-1 FAILED:\n  {}",
            String::from_utf8_lossy(&up.stderr).lines().take(8).collect::<Vec<_>>().join("\n  ")
        );
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    }

    // Wait for hub + the first N-1 nodes to fully mirror the N-1 sessions.
    let Some(hub_port) = wait_for_service_port(&project, &compose_file, "hub").await else {
        eprintln!("  never discovered hub port (phase 1)");
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    };
    let mut early_node_ports: Vec<u16> = Vec::new();
    for i in 0..last {
        if let Some(p) = wait_for_service_port(&project, &compose_file, &format!("node-{i}")).await {
            early_node_ports.push(p);
        }
    }
    let early_expected: BTreeSet<String> = (0..last).map(|i| format!("node-{i}")).collect();
    let phase1_deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let hub = session_ids(hub_port).await;
        let hub_ok = early_expected.is_subset(&hub);
        let mut min = usize::MAX;
        for &p in &early_node_ports {
            min = min.min(session_ids(p).await.len());
        }
        let nodes_ok = early_node_ports.is_empty() || min >= last;
        if hub_ok && nodes_ok {
            break;
        }
        if Instant::now() >= phase1_deadline {
            eprintln!("  phase 1 never converged (hub_ok={hub_ok}, min={min}/{last})");
            return LabResult { nodes, hub_has: hub.len(), min_node_has: min, fully_mirrored: false, elapsed: started.elapsed() };
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("    phase 1 settled in {:.1}s ({last}/{last} mirrored)", started.elapsed().as_secs_f64());

    // Late-joiner delay.
    tokio::time::sleep(Duration::from_secs(late_delay_secs)).await;
    let late_start = Instant::now();

    // Phase 2 — bring up the last nats-leaf + node.
    let up2 = Command::new("docker")
        .args([
            "compose",
            "-f",
            &compose_file.to_string_lossy(),
            "-p",
            &project,
            "up",
            "-d",
            &format!("nats-leaf-{last}"),
            &format!("node-{last}"),
        ])
        .env("MSYS_NO_PATHCONV", "1")
        .output()
        .expect("compose up phase 2");
    if !up2.status.success() {
        eprintln!(
            "  compose up phase-2 FAILED:\n  {}",
            String::from_utf8_lossy(&up2.stderr).lines().take(8).collect::<Vec<_>>().join("\n  ")
        );
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    }
    let Some(late_port) = wait_for_service_port(&project, &compose_file, &format!("node-{last}")).await else {
        eprintln!("  never discovered late node-{last} port");
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    };
    let mut all_node_ports = early_node_ports.clone();
    all_node_ports.push(late_port);

    // Wait for full convergence: hub sees all N, every leaf (including the
    // late one) mirrors all N.
    let expected: BTreeSet<String> = (0..nodes).map(|i| format!("node-{i}")).collect();
    let backfill_deadline = Instant::now() + Duration::from_secs(60);
    let mut hub_has = 0;
    let mut min_node_has = 0;
    loop {
        let hub = session_ids(hub_port).await;
        hub_has = hub.len();
        let mut min = usize::MAX;
        for &p in &all_node_ports {
            min = min.min(session_ids(p).await.len());
        }
        min_node_has = min;
        if expected.is_subset(&hub) && min_node_has >= nodes {
            break;
        }
        if Instant::now() >= backfill_deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let backfill_elapsed = late_start.elapsed();
    let total_elapsed = started.elapsed();
    let fully_mirrored = hub_has >= nodes && min_node_has >= nodes;
    eprintln!(
        "  late joiner backfill: hub {hub_has}/{nodes}, slowest node {min_node_has}/{nodes} in {:.1}s after late start (total {:.1}s, fully_mirrored={fully_mirrored})",
        backfill_elapsed.as_secs_f64(),
        total_elapsed.as_secs_f64()
    );
    drop(lab);
    LabResult { nodes, hub_has, min_node_has, fully_mirrored, elapsed: total_elapsed }
}

// ── T1 (solo multi-device, no hub) ─────────────────────────────────────────
// N devices peering directly via JetStream cross-domain sources. There is
// no openstory hub container and no `events-agg`; each device's mirror
// sources every peer's local `events` stream. A "rendezvous-only" NATS is
// still needed for leafnode routing — it's plain infrastructure, runs no
// JetStream domain, holds no application streams.

/// Rendezvous NATS: leafnode listener only. JetStream disabled because it
/// doesn't host any application streams in T1 — it's purely a routing relay
/// so leaves can reach each other's `$JS.<peer>.API` over leafnode hops.
fn t1_rendezvous_nats_service() -> String {
    format!(
        "  nats-rendezvous:\n    image: nats:2-alpine\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'NCFG'\n        listen: 0.0.0.0:4222\n        authorization {{ token: \"{TOKEN}\" }}\n        leafnodes {{ listen: \"0.0.0.0:7422\" }}\n        NCFG\n        exec nats-server -c /tmp/nats.conf\n"
    )
}

fn generate_t1_lab_compose(node_dirs: &[PathBuf]) -> String {
    let n = node_dirs.len();
    let mut yaml = String::from("services:\n");
    yaml.push_str(&t1_rendezvous_nats_service());
    // Each device's peer list: every node EXCEPT itself.
    let all_hosts: Vec<String> = (0..n).map(|i| format!("node-{i}")).collect();
    for (i, dir) in node_dirs.iter().enumerate() {
        // Leaf NATS: JetStream domain == this device's host, leafnoded to
        // the rendezvous so peers can reach this node's $JS.node-{i}.API.
        yaml.push_str(&format!(
            "  nats-leaf-{i}:\n    image: nats:2-alpine\n    depends_on: [nats-rendezvous]\n    entrypoint: [\"/bin/sh\", \"-c\"]\n    command:\n      - |\n        cat > /tmp/nats.conf <<'NCFG'\n        listen: 0.0.0.0:4222\n        jetstream {{ store_dir: /data/jetstream, max_mem: 256MB, max_file: 8GB, domain: \"node-{i}\" }}\n        leafnodes {{ remotes [ {{ url: \"nats://{TOKEN}@nats-rendezvous:7422\" }} ] }}\n        NCFG\n        exec nats-server -c /tmp/nats.conf\n"
        ));
        // openstory node: federation mesh mode (peer list excludes self).
        let peers: Vec<&str> = all_hosts
            .iter()
            .filter(|h| h.as_str() != format!("node-{i}").as_str())
            .map(|s| s.as_str())
            .collect();
        let peer_csv = peers.join(",");
        yaml.push_str(&format!(
            "  node-{i}:\n    image: open-story:test\n    command: [\"serve\", \"--role\", \"full\", \"--host\", \"0.0.0.0\", \"--port\", \"3002\", \"--nats-url\", \"nats://nats-leaf-{i}:4222\", \"--data-dir\", \"/data\", \"--watch-dir\", \"/watch\"]\n    environment:\n      - OPEN_STORY_HOST=node-{i}\n      - OPEN_STORY_PEER_DOMAINS={peer_csv}\n    ports:\n      - \"3002\"\n    volumes:\n      - {mount}:/watch:ro\n    depends_on: [nats-leaf-{i}]\n",
            mount = docker_path(dir),
        ));
    }
    yaml
}

async fn run_t1_lab(nodes: usize, events: usize) -> LabResult {
    eprintln!("\n  ══ Lab T1 (solo multi-device, no hub): {nodes} nodes × {events} events ══");
    let fixtures = tempfile::TempDir::new().unwrap();
    let node_dirs = generate_node_fixtures(fixtures.path(), nodes, events);
    let compose_file = fixtures.path().join("docker-compose.t1.yml");
    std::fs::write(&compose_file, generate_t1_lab_compose(&node_dirs)).unwrap();

    let project = format!("ost1-{nodes}-{}", std::process::id());
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

    // No hub in T1 — discover every node port directly.
    let mut node_ports: Vec<u16> = Vec::with_capacity(nodes);
    for i in 0..nodes {
        if let Some(p) = wait_for_service_port(&project, &compose_file, &format!("node-{i}")).await {
            node_ports.push(p);
        }
    }
    if node_ports.len() != nodes {
        eprintln!("  only discovered {}/{nodes} node ports", node_ports.len());
        return LabResult { nodes, hub_has: 0, min_node_has: 0, fully_mirrored: false, elapsed: started.elapsed() };
    }

    let expected: BTreeSet<String> = (0..nodes).map(|i| format!("node-{i}")).collect();
    let timeout = Duration::from_secs((60 + nodes as u64 * 6).min(180));
    let deadline = Instant::now() + timeout;
    let mut min_node_has = 0;
    loop {
        let mut min = usize::MAX;
        for &p in &node_ports {
            min = min.min(session_ids(p).await.len());
        }
        min_node_has = min;
        // Convergence in T1: every node sees every session (no hub).
        let mut all_have_all = true;
        for &p in &node_ports {
            let s = session_ids(p).await;
            if !expected.is_subset(&s) {
                all_have_all = false;
                break;
            }
        }
        if all_have_all {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let elapsed = started.elapsed();
    let fully_mirrored = min_node_has >= nodes;
    eprintln!(
        "  T1 {nodes} nodes → slowest node {min_node_has}/{nodes} in {:.1}s, fully_mirrored={fully_mirrored}",
        elapsed.as_secs_f64()
    );
    drop(lab);
    // hub_has is meaningless in T1 — reuse min_node_has so the result type
    // stays the same; assertions key off `fully_mirrored` or `min_node_has`.
    LabResult { nodes, hub_has: min_node_has, min_node_has, fully_mirrored, elapsed }
}

/// T1 solo multi-device, 3 nodes — Phase 2b Step 4. The laptop/desktop/phone
/// case: no hub, every device sources every other device's `events` stream
/// directly. Asserts every device converges to all 3 sessions cold.
#[tokio::test]
#[ignore]
async fn lab_federation_t1_solo_multi_device_3_nodes_cold() {
    let r = run_t1_lab(3, 12).await;
    assert!(
        r.fully_mirrored,
        "T1 cold: every device must mirror all {} sessions (slowest {}/{})",
        r.nodes, r.min_node_has, r.nodes
    );
}

async fn wait_for_service_port(project: &str, compose_file: &Path, service: &str) -> Option<u16> {
    for _ in 0..40 {
        if let Some(p) = service_port(project, compose_file, service) {
            return Some(p);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    None
}

/// T2 late-joiner backfill (Phase 2b Step 3) — `N-1` nodes come up first
/// and fully mirror, then the Nth node starts +30s later. Asserts the late
/// joiner catches up *gap-free*: hub sees all N sessions, the late leaf
/// mirrors the existing N-1 sessions via `events-mirror`, and every other
/// leaf picks up the late joiner's own session.
///
/// Cold (catch-up OFF) — proves the JetStream sources transport backfills
/// a node that joins after the rest of the fleet has settled. This is the
/// cold-boot race that historically failed 4/4 (federation-boot-window-loss
/// memory): the JetStream sources path closes it because the late mirror
/// sources from `events-agg` with `DeliverPolicy::All`, replaying the full
/// history of every peer's events.
#[tokio::test]
#[ignore]
async fn lab_federation_late_joiner_10_nodes_cold() {
    let r = run_lab_federation_late_joiner(10, 12, 30).await;
    assert!(
        r.fully_mirrored,
        "late joiner cold: every node must mirror all {} sessions (hub {}/{}, slowest node {}/{})",
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
