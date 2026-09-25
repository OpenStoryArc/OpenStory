//! B-10: the boot-memory gate as a container integration test
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! The production image boots against a synthetic store inside a 512 MiB
//! cgroup, with the bus on a NATS sidecar, and must reach serving without
//! an OOM kill or a restart, with the read model bounded below the session
//! count, the cgroup limit visible on health, jemalloc as the allocator,
//! and no `memory_pressure` finding in the verdict. The store is sized so
//! the pre-B-09 node (every projection resident, ≈ 0.9 B resident per JSONL
//! byte) would need more than the limit; the numbers are in the loop log.
//!
//! Opt-in like the other container tests: needs Docker and the image from
//! `just docker-build`; runs under `just test-container`; skips with a
//! message when Docker is not running. The fixture comes from the B-00
//! harness (`scripts/boot_memory.py --build-only`) in a temp dir, or from
//! `BMGATE_FIXTURE_DIR` to reuse one across runs. Containers are named
//! `bmgate-*` on their own network and removed at the end; the node's SQLite
//! is never shared between two running boots (the two boots here are
//! sequential: the first ingests the JSONL, the second is the measured
//! DB-present boot the fleet's restarts look like).
//!
//! Run with: cargo test -p open-story --test test_boot_memory_gate -- --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

const IMAGE: &str = "open-story:test";
const NATS_IMAGE: &str = "nats:2-alpine";
/// The cgroup limit the node boots under. Docker's `--memory 512m` is MiB.
const MEMORY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
/// The fixture: 150 sessions, 8 of them large (4000 events), 0.8 GB of JSONL.
const SESSIONS: u32 = 150;
const LARGE: u32 = 8;
const LARGE_EVENTS: u32 = 4000;
const TOTAL_GB: &str = "0.8";
/// A boot that has not served after this long fails the gate.
const SERVE_TIMEOUT: Duration = Duration::from_secs(900);
/// After serving, the consumers start (B-05) — keep watching this long.
const SETTLE: Duration = Duration::from_secs(20);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// Run `docker <args>`; stdout on success, the combined output on failure.
fn docker(args: &[&str]) -> Result<String, String> {
    let out = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| format!("docker {}: {e}", args.join(" ")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "docker {} exited {}: {}{}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

fn docker_running() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"]).is_ok()
}

/// The synthetic store, built by the B-00 harness into `root/data`.
struct Fixture {
    root: PathBuf,
    files: u64,
    events: u64,
    jsonl_bytes: u64,
}

fn build_fixture(root: &Path) -> Fixture {
    let script = repo_root().join("scripts/boot_memory.py");
    let out = Command::new("python3")
        .arg(&script)
        .args(["--fixture", root.to_str().unwrap()])
        .args(["--sessions", &SESSIONS.to_string()])
        .args(["--large", &LARGE.to_string()])
        .args(["--large-events", &LARGE_EVENTS.to_string()])
        .args(["--total-gb", TOTAL_GB])
        .arg("--build-only")
        .output()
        .expect("python3 runs scripts/boot_memory.py");
    assert!(
        out.status.success(),
        "fixture build failed ({}):\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).unwrap())
            .unwrap();
    // The container runs as uid 1000 and writes the SQLite store next to the
    // JSONL; on a Linux host the temp dir belongs to whoever runs the test.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in ["data", "watch"] {
            let p = root.join(dir);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
    }
    Fixture {
        root: root.to_path_buf(),
        files: manifest["files"].as_u64().unwrap(),
        events: manifest["events"].as_u64().unwrap(),
        jsonl_bytes: manifest["jsonl_bytes"].as_u64().unwrap(),
    }
}

/// The two containers and their network; removed on drop, whatever happened.
struct Stack {
    tag: String,
}

impl Stack {
    fn net(&self) -> String {
        format!("bmgate-net-{}", self.tag)
    }
    fn nats(&self) -> String {
        format!("bmgate-nats-{}", self.tag)
    }
    fn node(&self) -> String {
        format!("bmgate-node-{}", self.tag)
    }
    fn up_nats(&self) {
        let _ = docker(&["network", "create", &self.net()]);
        docker(&[
            "run",
            "-d",
            "--name",
            &self.nats(),
            "--network",
            &self.net(),
            NATS_IMAGE,
            "--jetstream",
            "--store_dir",
            "/tmp/js",
        ])
        .expect("nats sidecar starts");
    }
    fn down_node(&self) {
        let _ = docker(&["rm", "-f", &self.node()]);
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = docker(&["rm", "-f", &self.node()]);
        let _ = docker(&["rm", "-f", &self.nats()]);
        let _ = docker(&["network", "rm", &self.net()]);
    }
}

/// What one boot did, as health and `docker inspect` saw it.
#[derive(Debug)]
struct Boot {
    serving: Option<Value>,
    settled: Option<Value>,
    oom_killed: bool,
    restarts: u64,
    running: bool,
    exit_code: i64,
    serving_after: Option<Duration>,
    peak_rss: u64,
    max_count_during_replay: u64,
    log_tail: String,
}

#[derive(Debug, Default)]
struct Inspect {
    running: bool,
    exit_code: i64,
    oom_killed: bool,
    restarts: u64,
}

fn inspect(name: &str) -> Inspect {
    let s = docker(&[
        "inspect",
        "-f",
        "{{.State.Running}} {{.State.ExitCode}} {{.State.OOMKilled}} {{.RestartCount}}",
        name,
    ])
    .unwrap_or_default();
    let f: Vec<&str> = s.split_whitespace().collect();
    if f.len() < 4 {
        return Inspect::default();
    }
    Inspect {
        running: f[0] == "true",
        exit_code: f[1].parse().unwrap_or(-1),
        oom_killed: f[2] == "true",
        restarts: f[3].parse().unwrap_or(0),
    }
}

fn host_port(name: &str) -> u16 {
    let s = docker(&["port", name, "3002/tcp"]).expect("published port");
    s.lines()
        .find_map(|l| l.rsplit(':').next()?.trim().parse().ok())
        .expect("a host port")
}

async fn health(client: &reqwest::Client, url: &str) -> (Option<u16>, Value) {
    match client.get(url).send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let body = r.json::<Value>().await.unwrap_or(Value::Null);
            (Some(status), body)
        }
        Err(_) => (None, Value::Null),
    }
}

/// Boot the node from `image` against the fixture under `memory`, watch it
/// until it serves (plus `SETTLE`), dies, or `SERVE_TIMEOUT` passes.
async fn boot(stack: &Stack, image: &str, fixture: &Fixture, memory: &str) -> Boot {
    stack.down_node();
    let data = fixture.root.join("data").canonicalize().unwrap();
    let watch = fixture.root.join("watch").canonicalize().unwrap();
    let nats_url = format!("nats://{}:4222", stack.nats());
    let node = stack.node();
    let args: Vec<String> = [
        "run",
        "-d",
        "--name",
        &node,
        "--network",
        &stack.net(),
        "--memory",
        memory,
        "--memory-swap",
        memory,
        "-v",
        &format!("{}:/data", data.display()),
        "-v",
        &format!("{}:/watch", watch.display()),
        "-p",
        "127.0.0.1::3002",
        "-e",
        "OPEN_STORY_CLAUDE_WATCH_DIR=/watch",
        "-e",
        "OPEN_STORY_CODEX_WATCH_DIR=/watch",
        "-e",
        "OPEN_STORY_GROK_WATCH_DIR=/watch",
        "-e",
        "OPEN_STORY_PI_WATCH_DIR=",
        "-e",
        "OPEN_STORY_HERMES_WATCH_DIR=",
        "-e",
        "OPEN_STORY_LOG_FORMAT=text",
        image,
        "serve",
        "--host",
        "0.0.0.0",
        "--port",
        "3002",
        "--data-dir",
        "/data",
        "--watch-dir",
        "/watch",
        "--nats-url",
        &nats_url,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    docker(&argv).expect("node container starts");
    let port = host_port(&node);
    let url = format!("http://127.0.0.1:{port}/api/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let t0 = Instant::now();
    let mut serving: Option<Value> = None;
    let mut serving_after = None;
    let mut peak_rss = 0u64;
    let mut max_count_during_replay = 0u64;
    let mut last_line = String::new();
    let (settled, state) = loop {
        let state = inspect(&node);
        if !state.running {
            break (None, state);
        }
        let (status, body) = health(&client, &url).await;
        let phase = match status {
            Some(200) => "serving",
            Some(503) => body["boot"]["phase"].as_str().unwrap_or("booting"),
            Some(_) => "other",
            None => "no-listen",
        };
        if let Some(rss) = body["process"]["rss_bytes"].as_u64() {
            peak_rss = peak_rss.max(rss);
        }
        if phase == "replaying" {
            if let Some(c) = body["projections"]["count"].as_u64() {
                max_count_during_replay = max_count_during_replay.max(c);
            }
        }
        let line = format!(
            "  t={:4}s {phase:<10} rss={:>7.1} MB projections={}/{} replay={}/{}",
            t0.elapsed().as_secs(),
            body["process"]["rss_bytes"].as_u64().unwrap_or(0) as f64 / 1e6,
            body["projections"]["count"],
            body["projections"]["sessions"],
            body["boot"]["replay"]["done"],
            body["boot"]["replay"]["total"],
        );
        if line[8..] != last_line[8.min(last_line.len())..] || t0.elapsed().as_secs() % 15 == 0 {
            eprintln!("{line}");
        }
        last_line = line;
        if status == Some(200) {
            if serving.is_none() {
                serving = Some(body.clone());
                serving_after = Some(t0.elapsed());
            } else if t0.elapsed() - serving_after.unwrap() >= SETTLE {
                break (Some(body), state);
            }
        } else if t0.elapsed() > SERVE_TIMEOUT {
            break (None, state);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    let logs = Command::new("docker")
        .args(["logs", "--tail", "40", &node])
        .output()
        .map(|o| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
        })
        .unwrap_or_default();
    let state = if state.running { inspect(&node) } else { state };
    Boot {
        serving,
        settled,
        oom_killed: state.oom_killed,
        restarts: state.restarts,
        running: state.running,
        exit_code: state.exit_code,
        serving_after,
        peak_rss,
        max_count_during_replay,
        log_tail: logs,
    }
}

fn finding_ids(body: &Value) -> Vec<String> {
    body["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect()
}

/// The gate's assertions on one boot that reached serving and settled.
fn assert_gate(label: &str, b: &Boot, fixture: &Fixture) {
    assert!(
        !b.oom_killed && b.running && b.restarts == 0,
        "{label}: the node must survive the boot inside {} MiB — oom_killed={} running={} \
         restarts={} exit_code={} serving_after={:?} peak_rss={} B\n--- docker logs ---\n{}",
        MEMORY_LIMIT_BYTES >> 20,
        b.oom_killed,
        b.running,
        b.restarts,
        b.exit_code,
        b.serving_after,
        b.peak_rss,
        b.log_tail
    );
    let serving = b.serving.as_ref().unwrap_or_else(|| {
        panic!(
            "{label}: never served within {SERVE_TIMEOUT:?}\n{}",
            b.log_tail
        )
    });
    let settled = b
        .settled
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: did not settle after serving\n{}", b.log_tail));

    assert_eq!(
        serving["process"]["memory_limit_bytes"].as_u64(),
        Some(MEMORY_LIMIT_BYTES),
        "{label}: health carries the cgroup limit (B-07): {}",
        serving["process"]
    );
    assert_eq!(
        serving["process"]["allocator"].as_str(),
        Some("jemalloc"),
        "{label}: the image runs jemalloc (B-06): {}",
        serving["process"]
    );

    let sessions = serving["projections"]["sessions"].as_u64().unwrap();
    let count = serving["projections"]["count"].as_u64().unwrap();
    assert!(
        sessions >= fixture.files,
        "{label}: the store knows every fixture session ({sessions} < {})",
        fixture.files
    );
    // 0.8 GB of JSONL is a read model several times the clamped budget
    // (40 % of 512 MiB ≈ 215 MB), so a bounded cache holds strictly fewer
    // sessions than the store has — at serving and through replay (B-09).
    assert!(
        count < sessions,
        "{label}: projections.count {count} must stay below the {sessions} sessions once the \
         store exceeds the clamped budget"
    );
    assert!(
        b.max_count_during_replay < sessions,
        "{label}: during replay the resident count peaked at {} of {sessions}",
        b.max_count_during_replay
    );

    for (when, body) in [("serving", serving), ("settled", settled)] {
        // The rule can only see pressure it can measure: rss must be a
        // number inside the image (B-07's `ps` is not in debian-slim).
        let rss = body["process"]["rss_bytes"].as_u64().unwrap_or_else(|| {
            panic!(
                "{label}: process.rss_bytes must be a number at {when}, got {}",
                body["process"]
            )
        });
        assert!(rss > 0, "{label}: rss_bytes {rss} at {when}");
        let ids = finding_ids(body);
        assert!(
            !ids.iter().any(|id| id == "memory_pressure"),
            "{label}: no memory_pressure finding at {when} (rss {} B of {} B): {:?}",
            body["process"]["rss_bytes"],
            MEMORY_LIMIT_BYTES,
            body["verdict"]
        );
    }
    eprintln!(
        "{label}: serving after {:?}; peak rss {:.1} MB; projections {count}/{sessions} at serving, \
         max {} during replay; verdict {} → {}",
        b.serving_after.unwrap(),
        b.peak_rss as f64 / 1e6,
        b.max_count_during_replay,
        serving["verdict"]["level"],
        settled["verdict"]["level"],
    );
}

mod when_the_production_container_boots_inside_512_mib {
    use super::*;

    /// Two sequential boots on one store: the first ingests the JSONL into
    /// SQLite, the second is the DB-present restart the fleet's nodes make.
    /// Both must pass the gate.
    #[tokio::test]
    async fn it_serves_with_the_read_model_bounded_and_no_oom_kill() {
        if !docker_running() {
            eprintln!("skipping: Docker is not running (this gate needs it)");
            return;
        }
        assert!(
            docker(&["image", "inspect", IMAGE]).is_ok(),
            "image {IMAGE} not found — build it with `just docker-build`"
        );

        let keep = std::env::var_os("BMGATE_FIXTURE_DIR").map(PathBuf::from);
        let tmp = tempfile::tempdir().unwrap();
        let root = keep
            .clone()
            .unwrap_or_else(|| tmp.path().join("boot-memory-gate"));
        let t_fixture = Instant::now();
        let fixture = build_fixture(&root);
        eprintln!(
            "fixture: {} sessions, {} events, {:.2} GB JSONL at {} ({:?}; {})",
            fixture.files,
            fixture.events,
            fixture.jsonl_bytes as f64 / 1e9,
            root.display(),
            t_fixture.elapsed(),
            if keep.is_some() { "kept" } else { "temp" }
        );

        let stack = Stack {
            tag: format!(
                "{}-{}",
                std::process::id(),
                t_fixture.elapsed().as_nanos() % 100_000
            ),
        };
        stack.up_nats();
        let memory = format!("{}m", MEMORY_LIMIT_BYTES >> 20);

        let first = boot(&stack, IMAGE, &fixture, &memory).await;
        assert_gate("boot 1 (ingest)", &first, &fixture);
        let second = boot(&stack, IMAGE, &fixture, &memory).await;
        assert_gate("boot 2 (DB present)", &second, &fixture);
    }
}
