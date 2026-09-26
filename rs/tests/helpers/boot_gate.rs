//! The boot-memory gate's machinery (B-10, B-11), shared by
//! `test_boot_memory_gate.rs` and `test_consumer_drain_gate.rs`.
//!
//! The production image boots against the B-00 harness's synthetic store
//! inside a small cgroup, with the bus on a `nats:2-alpine` JetStream
//! sidecar on a private network. `boot` watches `docker inspect` and
//! `/api/health` once a second until the node serves, optionally until its
//! JetStream consumers have drained (B-11), then for a settle window.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::Value;

pub const NATS_IMAGE: &str = "nats:2-alpine";
/// A boot that has not served after this long fails the gate.
pub const SERVE_TIMEOUT: Duration = Duration::from_secs(900);
/// After serving (and draining, when asked), keep watching this long.
pub const SETTLE: Duration = Duration::from_secs(20);
/// A drain that has not finished after this long fails the gate.
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(900);

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/ has a parent")
        .to_path_buf()
}

/// Run `docker <args>`; stdout on success, the combined output on failure.
pub fn docker(args: &[&str]) -> Result<String, String> {
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

pub fn docker_running() -> bool {
    docker(&["info", "--format", "{{.ServerVersion}}"]).is_ok()
}

/// The synthetic store, built by the B-00 harness into `root/data`.
pub struct Fixture {
    pub root: PathBuf,
    pub files: u64,
    pub events: u64,
    pub jsonl_bytes: u64,
}

pub struct FixtureShape {
    pub sessions: u32,
    pub large: u32,
    pub large_events: u32,
    pub total_gb: &'static str,
}

pub fn build_fixture(root: &Path, shape: &FixtureShape) -> Fixture {
    let script = repo_root().join("scripts/boot_memory.py");
    let out = Command::new("python3")
        .arg(&script)
        .args(["--fixture", root.to_str().unwrap()])
        .args(["--sessions", &shape.sessions.to_string()])
        .args(["--large", &shape.large.to_string()])
        .args(["--large-events", &shape.large_events.to_string()])
        .args(["--total-gb", shape.total_gb])
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
pub struct Stack {
    /// Name prefix: `bmgate` (B-10), `b11` (B-11).
    pub prefix: &'static str,
    pub tag: String,
}

impl Stack {
    pub fn net(&self) -> String {
        format!("{}-net-{}", self.prefix, self.tag)
    }
    pub fn nats(&self) -> String {
        format!("{}-nats-{}", self.prefix, self.tag)
    }
    pub fn node(&self) -> String {
        format!("{}-node-{}", self.prefix, self.tag)
    }
    pub fn up_nats(&self) {
        let _ = docker(&["network", "create", &self.net()]);
        docker(&[
            "run",
            "-d",
            "--name",
            &self.nats(),
            "--network",
            &self.net(),
            // Reachable from the test too (B-11 loads a backlog and reads
            // the consumers' state); loopback only.
            "-p",
            "127.0.0.1::4222",
            NATS_IMAGE,
            "--jetstream",
            "--store_dir",
            "/tmp/js",
        ])
        .expect("nats sidecar starts");
    }
    /// The sidecar's client port on the host.
    pub fn nats_url(&self) -> String {
        let s = docker(&["port", &self.nats(), "4222/tcp"]).expect("nats port");
        let port: u16 = s
            .lines()
            .find_map(|l| l.rsplit(':').next()?.trim().parse().ok())
            .expect("a host port for nats");
        format!("nats://127.0.0.1:{port}")
    }
    pub fn down_node(&self) {
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
pub struct Boot {
    pub serving: Option<Value>,
    pub settled: Option<Value>,
    pub oom_killed: bool,
    pub restarts: u64,
    pub running: bool,
    pub exit_code: i64,
    pub serving_after: Option<Duration>,
    /// From serving to drained (B-11); `None` when not asked or never.
    pub drained_after: Option<Duration>,
    pub peak_rss: u64,
    /// Peak rss from the flip to serving on: the consumers' drain.
    pub peak_rss_after_serving: u64,
    /// The worst `memory_pressure` level health reported after serving.
    pub worst_pressure_after_serving: Option<String>,
    pub max_count_during_replay: u64,
    pub log_tail: String,
}

#[derive(Debug, Default)]
pub struct Inspect {
    pub running: bool,
    pub exit_code: i64,
    pub oom_killed: bool,
    pub restarts: u64,
}

pub fn inspect(name: &str) -> Inspect {
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

pub fn host_port(name: &str) -> u16 {
    let s = docker(&["port", name, "3002/tcp"]).expect("published port");
    s.lines()
        .find_map(|l| l.rsplit(':').next()?.trim().parse().ok())
        .expect("a host port")
}

pub async fn health(client: &reqwest::Client, url: &str) -> (Option<u16>, Value) {
    match client.get(url).send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let body = r.json::<Value>().await.unwrap_or(Value::Null);
            (Some(status), body)
        }
        Err(_) => (None, Value::Null),
    }
}

pub fn finding_ids(body: &Value) -> Vec<String> {
    body["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect()
}

fn pressure_level(body: &Value) -> Option<String> {
    body["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["id"] == "memory_pressure")
        .and_then(|f| f["level"].as_str().map(str::to_string))
}

/// How one boot is run.
pub struct BootOpts<'a> {
    pub image: &'a str,
    pub memory: &'a str,
    /// Extra `-e` pairs for the node.
    pub env: Vec<(&'a str, &'a str)>,
    /// When set, the settle window starts only once every JetStream
    /// consumer on these streams has drained (B-11).
    pub drain_via: Option<&'a open_story_bus::nats_bus::NatsBus>,
}

/// Every consumer on `events`, `local`, and `presence` has nothing left to
/// deliver and nothing waiting for an ack; `None` while there are none yet.
pub async fn consumers_drained(bus: &open_story_bus::nats_bus::NatsBus) -> Option<bool> {
    let mut any = false;
    for stream in ["events", "local", "presence"] {
        let Ok(s) = bus.jetstream().get_stream(stream).await else {
            continue;
        };
        let mut names = Vec::new();
        let mut listed = s.consumer_names();
        while let Some(Ok(n)) = listed.next().await {
            names.push(n);
        }
        for name in names {
            any = true;
            match s.consumer_info(&name).await {
                Ok(info) if info.num_pending == 0 && info.num_ack_pending == 0 => {}
                _ => return Some(false),
            }
        }
    }
    any.then_some(true)
}

/// Boot the node against the fixture and watch it until it serves (plus
/// the drain, when asked, plus `SETTLE`), dies, or a timeout passes.
pub async fn boot(stack: &Stack, fixture: &Fixture, opts: &BootOpts<'_>) -> Boot {
    stack.down_node();
    let data = fixture.root.join("data").canonicalize().unwrap();
    let watch = fixture.root.join("watch").canonicalize().unwrap();
    let nats_url = format!("nats://{}:4222", stack.nats());
    let node = stack.node();
    let data_mount = format!("{}:/data", data.display());
    let watch_mount = format!("{}:/watch", watch.display());
    let mut args: Vec<String> = [
        "run",
        "-d",
        "--name",
        &node,
        "--network",
        &stack.net(),
        "--memory",
        opts.memory,
        "--memory-swap",
        opts.memory,
        "-v",
        &data_mount,
        "-v",
        &watch_mount,
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (k, v) in &opts.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }
    args.extend(
        [
            opts.image,
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
        .map(|s| s.to_string()),
    );
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
    let mut drained_at: Option<Duration> = None;
    let mut peak_rss = 0u64;
    let mut peak_rss_after_serving = 0u64;
    let mut worst_pressure: Option<String> = None;
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
        let rss = body["process"]["rss_bytes"].as_u64();
        if let Some(rss) = rss {
            peak_rss = peak_rss.max(rss);
        }
        if phase == "replaying" {
            if let Some(c) = body["projections"]["count"].as_u64() {
                max_count_during_replay = max_count_during_replay.max(c);
            }
        }
        if serving.is_some() {
            peak_rss_after_serving = peak_rss_after_serving.max(rss.unwrap_or(0));
            if let Some(level) = pressure_level(&body) {
                if worst_pressure.as_deref() != Some("critical") {
                    worst_pressure = Some(level);
                }
            }
        }
        let lags: Vec<String> = body["consumers"]
            .as_object()
            .map(|c| {
                let mut v: Vec<String> = c
                    .iter()
                    .map(|(k, h)| format!("{}:{}", &k[..2.min(k.len())], h["lag"]))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
        let line = format!(
            "  t={:4}s {phase:<10} rss={:>7.1} MB projections={}/{} replay={}/{} lag={}",
            t0.elapsed().as_secs(),
            rss.unwrap_or(0) as f64 / 1e6,
            body["projections"]["count"],
            body["projections"]["sessions"],
            body["boot"]["replay"]["done"],
            body["boot"]["replay"]["total"],
            lags.join(","),
        );
        if line[8..] != last_line[8.min(last_line.len())..] || t0.elapsed().as_secs() % 15 == 0 {
            eprintln!("{line}");
        }
        last_line = line;
        if status == Some(200) {
            if serving.is_none() {
                serving = Some(body.clone());
                serving_after = Some(t0.elapsed());
            }
            // The settle window starts at serving, or at the drain.
            let settle_from = match opts.drain_via {
                None => serving_after,
                Some(bus) => {
                    if drained_at.is_none() && consumers_drained(bus).await == Some(true) {
                        drained_at = Some(t0.elapsed());
                        eprintln!(
                            "  t={:4}s consumers drained ({:?} after serving)",
                            t0.elapsed().as_secs(),
                            t0.elapsed() - serving_after.unwrap()
                        );
                    }
                    if drained_at.is_none() && t0.elapsed() - serving_after.unwrap() > DRAIN_TIMEOUT
                    {
                        break (None, state);
                    }
                    drained_at
                }
            };
            if let Some(from) = settle_from {
                if t0.elapsed() - from >= SETTLE {
                    break (Some(body), state);
                }
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
        drained_after: drained_at.zip(serving_after).map(|(d, s)| d - s),
        peak_rss,
        peak_rss_after_serving,
        worst_pressure_after_serving: worst_pressure,
        max_count_during_replay,
        log_tail: logs,
    }
}

/// The B-10 gate's assertions on one boot that reached serving and settled.
pub fn assert_gate(label: &str, b: &Boot, fixture: &Fixture, memory_limit_bytes: u64) {
    assert!(
        !b.oom_killed && b.running && b.restarts == 0,
        "{label}: the node must survive the boot inside {} MiB — oom_killed={} running={} \
         restarts={} exit_code={} serving_after={:?} drained_after={:?} peak_rss={} B \
         peak_rss_after_serving={} B\n--- docker logs ---\n{}",
        memory_limit_bytes >> 20,
        b.oom_killed,
        b.running,
        b.restarts,
        b.exit_code,
        b.serving_after,
        b.drained_after,
        b.peak_rss,
        b.peak_rss_after_serving,
        b.log_tail
    );
    let serving = b.serving.as_ref().unwrap_or_else(|| {
        panic!(
            "{label}: never served within {SERVE_TIMEOUT:?}\n{}",
            b.log_tail
        )
    });
    let settled = b.settled.as_ref().unwrap_or_else(|| {
        panic!(
            "{label}: did not settle after serving (drained_after={:?}, peak rss after serving \
             {} B)\n{}",
            b.drained_after, b.peak_rss_after_serving, b.log_tail
        )
    });

    assert_eq!(
        serving["process"]["memory_limit_bytes"].as_u64(),
        Some(memory_limit_bytes),
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
    // 0.8 GB of JSONL is a read model several times the clamped budget, so
    // a bounded cache holds strictly fewer sessions than the store has — at
    // serving and through replay (B-09).
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

    // The rule can only see pressure it can measure: rss must be a number
    // inside the image. Pressure is judged as the fleet would feel it: at
    // the flip to serving nothing may be critical; settled, there is no
    // memory_pressure finding at any level.
    for (when, body) in [("serving", serving), ("settled", settled)] {
        let rss = body["process"]["rss_bytes"].as_u64().unwrap_or_else(|| {
            panic!(
                "{label}: process.rss_bytes must be a number at {when}, got {}",
                body["process"]
            )
        });
        assert!(rss > 0, "{label}: rss_bytes {rss} at {when}");
    }
    let critical_at_flip = serving["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|f| f["id"] == "memory_pressure" && f["level"] == "critical");
    assert!(
        !critical_at_flip,
        "{label}: memory_pressure is critical at the flip to serving (rss {} B of {} B): {:?}",
        serving["process"]["rss_bytes"], memory_limit_bytes, serving["verdict"]
    );
    let settled_ids = finding_ids(settled);
    assert!(
        !settled_ids.iter().any(|id| id == "memory_pressure"),
        "{label}: no memory_pressure finding once settled (rss {} B of {} B): {:?}",
        settled["process"]["rss_bytes"],
        memory_limit_bytes,
        settled["verdict"]
    );
    eprintln!(
        "{label}: serving after {:?}; drained {:?} after serving; peak rss {:.1} MB \
         ({:.1} MB after serving, worst pressure after serving {:?}); projections \
         {count}/{sessions} at serving, max {} during replay; verdict {} → {}",
        b.serving_after.unwrap(),
        b.drained_after,
        b.peak_rss as f64 / 1e6,
        b.peak_rss_after_serving as f64 / 1e6,
        b.worst_pressure_after_serving,
        b.max_count_during_replay,
        serving["verdict"]["level"],
        settled["verdict"]["level"],
    );
}
