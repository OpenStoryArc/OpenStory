//! B-11 (b): a stream backlog drains at bounded memory
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! The sibling of the B-10 gate. Production, 2026-09-26: the owner's mini
//! (3.5 GB limit, `events` stream full at ~2 GB) replays within budget and
//! reaches serving; seconds later the five consumer actors all subscribe to
//! their whole stream from the start at once, rss climbs from 1.5 GB to the
//! limit, `memory_pressure` fires, and the kernel kills it — every ~70 s.
//!
//! Here the production image boots inside a 512 MiB cgroup against the B-00
//! synthetic store (150 sessions, 0.8 GB of JSONL), with the same events
//! already sitting in the sidecar's `events` stream as a backlog several
//! times the node's projection budget (30 % of the box ≈ 161 MB) — the
//! upgrade case, a store that holds its history and a full stream. Twice on
//! one store: the first boot (ingest; no durable consumer exists yet), then
//! a restart after more events were published while it was down. Each boot
//! must reach serving, drain every JetStream consumer (nothing pending,
//! nothing waiting for an ack), and settle with no OOM kill, no restart,
//! and no `memory_pressure` finding; on the restart, persist is handed only
//! the batches published while it was down.
//!
//! Opt-in like B-10: needs Docker and the image (`just docker-build`); runs
//! under `just test-container`; skips when Docker is not running.
//! `B11_IMAGE` boots another image (the loop log's before numbers);
//! `B11_FIXTURE_DIR` reuses a fixture. Containers are `b11-*`, removed on
//! drop.
//!
//! Run with: cargo test -p open-story --test test_consumer_drain_gate -- --nocapture

mod helpers;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use helpers::boot_gate::{
    assert_gate, boot, build_fixture, docker, docker_running, BootOpts, Fixture, FixtureShape,
    Stack,
};
use open_story::cloud_event::CloudEvent;
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};

const MEMORY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const SHAPE: FixtureShape = FixtureShape {
    sessions: 150,
    large: 8,
    large_events: 4000,
    total_gb: "0.8",
};
/// Events per published batch, about what the watcher sends.
const BATCH: usize = 50;
/// Batches published while the node is down between the two boots.
const WHILE_DOWN: usize = 25;
/// The projection share of the box (B-10: 30 %).
const PROJECTION_BUDGET: u64 = MEMORY_LIMIT_BYTES * 30 / 100;

async fn connect(url: &str) -> NatsBus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match NatsBus::connect(url).await {
            Ok(bus) => return bus,
            Err(e) if Instant::now() > deadline => panic!("nats sidecar at {url}: {e}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
}

/// Publish every fixture session's events to the `events` stream, as the
/// fleet's watchers did before the node went down. Returns batches sent.
async fn load_backlog(bus: &NatsBus, fixture: &Fixture) -> usize {
    let mut sent = 0;
    let data = fixture.root.join("data");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&data)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            name.ends_with(".jsonl") && name != "events.jsonl" && name != "presence.jsonl"
        })
        .collect();
    files.sort();
    for path in files {
        let sid = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let events: Vec<CloudEvent> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        for chunk in events.chunks(BATCH) {
            let batch = IngestBatch {
                session_id: sid.clone(),
                project_id: "b11".to_string(),
                events: chunk.to_vec(),
            };
            bus.publish(&format!("events.b11.{sid}.main"), &batch)
                .await
                .expect("publish backlog");
            sent += 1;
        }
    }
    sent
}

async fn publish_new(bus: &NatsBus, n: usize) {
    for i in 0..n {
        let sid = format!("b11-while-down-{}", i % 5);
        let batch = IngestBatch {
            session_id: sid.clone(),
            project_id: "b11".to_string(),
            events: vec![helpers::make_user_prompt(
                &sid,
                &format!("b11-while-down-{i}"),
            )],
        };
        bus.publish(&format!("events.b11.{sid}.main"), &batch)
            .await
            .expect("publish");
    }
}

async fn events_stream(bus: &NatsBus) -> (u64, u64) {
    let mut s = bus.jetstream().get_stream("events").await.expect("events");
    let info = s.info().await.expect("info");
    (info.state.messages, info.state.bytes)
}

mod when_a_node_boots_on_a_stream_backlog_inside_512_mib {
    use super::*;

    #[tokio::test]
    async fn it_drains_at_bounded_memory_and_a_restart_resumes() {
        if !docker_running() {
            eprintln!("skipping: Docker is not running (this gate needs it)");
            return;
        }
        let image = std::env::var("B11_IMAGE").unwrap_or_else(|_| "open-story:test".into());
        assert!(
            docker(&["image", "inspect", &image]).is_ok(),
            "image {image} not found — build it with `just docker-build`"
        );

        let keep = std::env::var_os("B11_FIXTURE_DIR").map(PathBuf::from);
        let tmp = tempfile::tempdir().unwrap();
        let root = keep
            .clone()
            .unwrap_or_else(|| tmp.path().join("consumer-drain-gate"));
        let t0 = Instant::now();
        let fixture = build_fixture(&root, &SHAPE);
        eprintln!(
            "fixture: {} sessions, {} events, {:.2} GB JSONL at {} ({:?})",
            fixture.files,
            fixture.events,
            fixture.jsonl_bytes as f64 / 1e9,
            root.display(),
            t0.elapsed()
        );

        let stack = Stack {
            prefix: "b11",
            tag: format!(
                "{}-{}",
                std::process::id(),
                t0.elapsed().as_nanos() % 100_000
            ),
        };
        stack.up_nats();
        let probe = connect(&stack.nats_url()).await;
        probe.ensure_streams().await.expect("streams");
        let t_load = Instant::now();
        let batches = load_backlog(&probe, &fixture).await;
        let (messages, bytes) = events_stream(&probe).await;
        eprintln!(
            "backlog: {batches} batches published, events stream {messages} messages / \
             {:.2} GB ({:?}); projection budget {:.0} MB",
            bytes as f64 / 1e9,
            t_load.elapsed(),
            PROJECTION_BUDGET as f64 / 1e6
        );
        assert!(
            bytes > 3 * PROJECTION_BUDGET,
            "the backlog ({bytes} B) must be several times the projection budget"
        );

        let memory = format!("{}m", MEMORY_LIMIT_BYTES >> 20);
        let opts = BootOpts {
            image: &image,
            memory: &memory,
            // The node's name for itself, stable across container
            // recreations, as on the fleet's compose files.
            env: vec![("OPEN_STORY_HOST", "b11-node")],
            drain_via: Some(&probe),
        };

        let first = boot(&stack, &fixture, &opts).await;
        assert_gate(
            "boot 1 (ingest, full stream)",
            &first,
            &fixture,
            MEMORY_LIMIT_BYTES,
        );
        assert!(
            first.drained_after.is_some(),
            "boot 1 drained its consumers"
        );

        stack.down_node();
        publish_new(&probe, WHILE_DOWN).await;

        let second = boot(&stack, &fixture, &opts).await;
        assert_gate("boot 2 (restart)", &second, &fixture, MEMORY_LIMIT_BYTES);
        assert!(
            second.drained_after.is_some(),
            "boot 2 drained its consumers"
        );
        let settled = second.settled.as_ref().unwrap();
        assert_eq!(
            settled["consumers"]["persist"]["delivered"].as_u64(),
            Some(WHILE_DOWN as u64),
            "boot 2: persist is handed only the {WHILE_DOWN} batches published while the node \
             was down: {}",
            settled["consumers"]
        );
    }
}
