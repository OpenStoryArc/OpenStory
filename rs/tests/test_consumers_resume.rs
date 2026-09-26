//! B-11 (a): consumer start resumes
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! Measured on the owner's mini, 2026-09-26: every boot replays within
//! budget, then seconds after serving all five consumer actors subscribe to
//! their whole stream from the start at once (ephemeral consumers,
//! `DeliverPolicy::All`), the backlog floods the process, and the kernel
//! kills it. Every restart pays for the whole stream again.
//!
//! This spec boots a real node (in-process `run_server`, its own runtime so
//! it can be stopped) on a scratch `nats-server`, with N events and a run of
//! presence beats already in the streams:
//!
//! 1. boot, reach serving, let the actors drain, stop the node;
//! 2. publish M more events while it is down, boot again: each event actor
//!    (persist, patterns, projections, broadcast) is handed exactly the M
//!    new batches, and presence only the beats published since the stop;
//!    the JetStream consumers on `events` carry the same names both times;
//! 3. delete the node's consumers on `events` (what the server does to a
//!    lost consumer), publish K more: the consumers come back under the
//!    same names and each event actor is handed exactly the K new batches.
//!
//! Deliveries are counted by the node itself (`consumers.<actor>.delivered`
//! on `/api/health`, process-wide, so the spec reads differences).
//!
//! Needs `nats-server` on PATH or at /opt/homebrew/bin; skips otherwise.
//! `B11_DIR` keeps the scratch files for inspection.
//! Run with: cargo test -p open-story --test test_consumers_resume -- --nocapture

mod helpers;

use std::collections::BTreeMap;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use open_story::server::{run_server, Config, Role};
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};

const NATS_PORT: u16 = 4711;
const API_PORT: u16 = 8711;
const EVENT_ACTORS: [&str; 4] = ["persist", "patterns", "projections", "broadcast"];
const N: usize = 40;
const M: usize = 7;
const K: usize = 5;
const PEER_BEATS: usize = 30;

struct ScratchNats {
    child: std::process::Child,
}

impl Drop for ScratchNats {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn nats_bin() -> Option<std::path::PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("nats-server"))
            .find(|c| c.is_file())
    });
    on_path.or_else(|| {
        let brew = std::path::PathBuf::from("/opt/homebrew/bin/nats-server");
        brew.is_file().then_some(brew)
    })
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

fn start_nats(store: &std::path::Path) -> Option<ScratchNats> {
    let bin = nats_bin()?;
    let child = Command::new(bin)
        .args([
            "-a",
            "127.0.0.1",
            "-p",
            &NATS_PORT.to_string(),
            "-js",
            "-sd",
        ])
        .arg(store)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if port_open(NATS_PORT) {
            return Some(ScratchNats { child });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn scratch_root() -> tempfile::TempDir {
    match std::env::var_os("B11_DIR") {
        Some(dir) => {
            std::fs::create_dir_all(&dir).expect("create B11_DIR");
            tempfile::tempdir_in(dir).expect("scratch dir")
        }
        None => tempfile::tempdir().expect("scratch dir"),
    }
}

/// One node run on its own runtime, so dropping the runtime stops every
/// task the node spawned (actors, forwarders, the listener) — a restart.
struct Node {
    rt: tokio::runtime::Runtime,
}

impl Node {
    fn start(nats_url: &str, data_dir: &std::path::Path, watch_dir: &std::path::Path) -> Node {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let nats_url = nats_url.to_string();
        let data_dir = data_dir.to_path_buf();
        let watch_dir = watch_dir.to_path_buf();
        rt.spawn(async move {
            let node_bus = NatsBus::connect(&nats_url).await.expect("connect");
            node_bus.ensure_streams().await.expect("streams");
            let watch = watch_dir.to_string_lossy().to_string();
            let config = Config {
                role: Role::Consumer,
                host: "127.0.0.1".into(),
                port: API_PORT,
                data_dir: data_dir.to_string_lossy().to_string(),
                nats_url: nats_url.clone(),
                consumers_start: "serving".into(),
                claude_watch_dir: watch.clone(),
                codex_watch_dir: watch.clone(),
                grok_watch_dir: watch.clone(),
                watch_dir: watch.clone(),
                // One beat at start, then quiet: the spec counts beats.
                presence_interval_secs: 3600,
                ..Config::default()
            };
            let bus: std::sync::Arc<dyn Bus> = std::sync::Arc::new(node_bus);
            let _ = run_server(
                "127.0.0.1",
                API_PORT,
                &data_dir,
                None,
                &[watch_dir],
                bus,
                config,
            )
            .await;
        });
        Node { rt }
    }

    fn stop(self) {
        self.rt.shutdown_timeout(Duration::from_secs(3));
        let deadline = Instant::now() + Duration::from_secs(10);
        while port_open(API_PORT) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

async fn health(client: &reqwest::Client) -> Option<serde_json::Value> {
    client
        .get(format!("http://127.0.0.1:{API_PORT}/api/health"))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()
}

async fn wait_serving(client: &reqwest::Client) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(h) = health(client).await {
            let running = h["consumers"]
                .as_object()
                .is_some_and(|c| c.values().filter(|a| a["state"] == "running").count() >= 5);
            if h["boot"]["phase"] == "serving" && running {
                return;
            }
        }
        assert!(Instant::now() < deadline, "node never reached serving");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Batches each actor has been handed since this process started.
async fn delivered(client: &reqwest::Client) -> BTreeMap<String, u64> {
    let h = health(client).await.unwrap_or_default();
    h["consumers"]
        .as_object()
        .map(|c| {
            c.iter()
                .map(|(k, v)| (k.clone(), v["delivered"].as_u64().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

fn diff(after: &BTreeMap<String, u64>, before: &BTreeMap<String, u64>, actor: &str) -> i64 {
    after.get(actor).copied().unwrap_or(0) as i64 - before.get(actor).copied().unwrap_or(0) as i64
}

async fn consumer_names(bus: &NatsBus, stream: &str) -> Vec<String> {
    let Ok(s) = bus.jetstream().get_stream(stream).await else {
        return vec![];
    };
    let mut names = Vec::new();
    let mut listed = s.consumer_names();
    while let Some(Ok(n)) = listed.next().await {
        names.push(n);
    }
    names.sort();
    names
}

/// Every consumer on the node's streams has nothing left to deliver and
/// nothing waiting for an ack. Gives up after `within` (the assertions that
/// follow then say what was not drained).
async fn drained(bus: &NatsBus, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        let mut all = true;
        let mut any = false;
        for stream in ["events", "local", "presence"] {
            let Ok(s) = bus.jetstream().get_stream(stream).await else {
                continue;
            };
            for name in consumer_names(bus, stream).await {
                any = true;
                match s.consumer_info(&name).await {
                    Ok(info) => {
                        if info.num_pending > 0 || info.num_ack_pending > 0 {
                            all = false;
                        }
                    }
                    Err(_) => all = false,
                }
            }
        }
        if any && all {
            // A last beat for the in-process counters to catch up.
            tokio::time::sleep(Duration::from_millis(500)).await;
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn publish_events(bus: &NatsBus, tag: &str, n: usize) {
    for i in 0..n {
        let sid = format!("b11-{tag}-{}", i % 3);
        let batch = IngestBatch {
            session_id: sid.clone(),
            project_id: "b11".to_string(),
            events: vec![helpers::make_user_prompt(&sid, &format!("b11-{tag}-{i}"))],
        };
        bus.publish(&format!("events.b11.{sid}.main"), &batch)
            .await
            .expect("publish");
    }
}

async fn last_seq(bus: &NatsBus, stream: &str) -> u64 {
    let mut s = bus.jetstream().get_stream(stream).await.expect("stream");
    s.info().await.expect("info").state.last_sequence
}

mod when_a_node_restarts_on_a_stream_it_has_already_read {
    use super::*;

    #[test]
    fn each_actor_is_handed_only_what_it_has_not_acknowledged() {
        if nats_bin().is_none() {
            eprintln!("skip: no nats-server");
            return;
        }
        assert!(
            !port_open(NATS_PORT) && !port_open(API_PORT),
            "ports {NATS_PORT}/{API_PORT} must be free"
        );
        let root = scratch_root();
        let nats_dir = root.path().join("nats");
        let data_dir = root.path().join("data");
        let watch_dir = root.path().join("watch");
        for d in [&nats_dir, &data_dir, &watch_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        let _nats = start_nats(&nats_dir).expect("scratch nats-server");
        let nats_url = format!("nats://127.0.0.1:{NATS_PORT}");

        let ctl = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let probe = ctl.block_on(async {
            let probe = NatsBus::connect(&nats_url).await.expect("connect probe");
            probe.ensure_streams().await.expect("streams");
            publish_events(&probe, "n", N).await;
            // Another node's beats, all on one subject, already in the stream.
            for _ in 0..PEER_BEATS {
                let beat = IngestBatch {
                    session_id: String::new(),
                    project_id: String::new(),
                    events: vec![],
                };
                probe
                    .publish("presence.b11-peer.p", &beat)
                    .await
                    .expect("beat");
            }
            probe
        });

        // ── Run 1: first boot, the actors read what they must and drain.
        let node = Node::start(&nats_url, &data_dir, &watch_dir);
        let (names_1, after_1, presence_at_stop) = ctl.block_on(async {
            wait_serving(&client).await;
            let ok = drained(&probe, Duration::from_secs(60)).await;
            eprintln!("run 1 drained: {ok}");
            let names = consumer_names(&probe, "events").await;
            let d = delivered(&client).await;
            eprintln!("run 1: consumers on events {names:?}; delivered {d:?}");
            (names, d, last_seq(&probe, "presence").await)
        });
        node.stop();

        // ── Down: M more events.
        ctl.block_on(publish_events(&probe, "m", M));

        // ── Run 2: the restart.
        let node = Node::start(&nats_url, &data_dir, &watch_dir);
        let (names_2, after_2, presence_since_stop) = ctl.block_on(async {
            wait_serving(&client).await;
            let ok = drained(&probe, Duration::from_secs(60)).await;
            eprintln!("run 2 drained: {ok}");
            let names = consumer_names(&probe, "events").await;
            let d = delivered(&client).await;
            let since = last_seq(&probe, "presence").await - presence_at_stop;
            eprintln!("run 2: consumers on events {names:?}; delivered {d:?}; presence since stop {since}");
            (names, d, since)
        });

        // ── Lost: the server deletes the node's consumers; K more events.
        let (names_3, after_3) = ctl.block_on(async {
            for stream in ["events", "local"] {
                let s = probe.jetstream().get_stream(stream).await.unwrap();
                for name in consumer_names(&probe, stream).await {
                    let _ = s.delete_consumer(&name).await;
                }
            }
            publish_events(&probe, "k", K).await;
            // The actors notice (a quiet probe, or the pull's "consumer
            // deleted"), end, and the supervisor resubscribes after backoff.
            let deadline = Instant::now() + Duration::from_secs(45);
            loop {
                let d = delivered(&client).await;
                let all_in = EVENT_ACTORS
                    .iter()
                    .all(|a| diff(&d, &after_2, a) >= K as i64);
                if all_in || Instant::now() > deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let ok = drained(&probe, Duration::from_secs(30)).await;
            eprintln!("run 3 drained: {ok}");
            let names = consumer_names(&probe, "events").await;
            let d = delivered(&client).await;
            eprintln!("after the loss: consumers on events {names:?}; delivered {d:?}");
            (names, d)
        });
        node.stop();

        let mut failures = Vec::new();
        if names_1.is_empty() {
            failures.push("run 1: no consumer on events".to_string());
        }
        if names_2 != names_1 {
            failures.push(format!(
                "the consumers on events are not the same across a restart: {names_1:?} → {names_2:?}"
            ));
        }
        for actor in EVENT_ACTORS {
            let got = diff(&after_2, &after_1, actor);
            if got != M as i64 {
                failures.push(format!(
                    "restart: {actor} was handed {got} batches, want the {M} published while it was down"
                ));
            }
        }
        let presence = diff(&after_2, &after_1, "presence");
        if presence != presence_since_stop as i64 {
            failures.push(format!(
                "restart: presence was handed {presence} batches, want the {presence_since_stop} \
                 published since the stop"
            ));
        }
        if names_3 != names_1 {
            failures.push(format!(
                "a deleted consumer comes back under its own name: {names_1:?} → {names_3:?}"
            ));
        }
        for actor in EVENT_ACTORS {
            let got = diff(&after_3, &after_2, actor);
            if got != K as i64 {
                failures.push(format!(
                    "recreated: {actor} was handed {got} batches, want the {K} published after the loss"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "B-11 (a):\n  {}",
            failures.join("\n  ")
        );
    }
}
