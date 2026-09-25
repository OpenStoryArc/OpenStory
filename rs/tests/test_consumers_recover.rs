//! Consumers survive losing their JetStream consumer (regression after the
//! kindle x consistency merge, 5d04863).
//!
//! Seen on three nodes: after the boot phase flips to serving, health says
//! every consumer actor is `running`, lag 0, restarts 0, yet NATS holds no
//! consumer on `events` or `presence` and the store stops advancing. The
//! bus's consumers are ephemeral push consumers; when the node's NATS
//! connection loses interest for longer than the server's inactivity
//! threshold (a stall or a slow-consumer disconnect while four actors
//! drain the whole stream at once), the server deletes them. The forwarder
//! reading the deleted consumer then waits forever on a subscription that
//! will never deliver again, holding the actor's channel open, so the
//! supervisor never sees the run end.
//!
//! This spec boots a real node on a scratch `nats-server` (consumers start
//! at serving, a leaf URL configured as on the real nodes), waits for
//! serving, deletes the node's consumers the way the server does after a
//! stall, publishes an event, and asks that it lands in the store.
//!
//! Needs `nats-server` on PATH or at /opt/homebrew/bin; skips otherwise.
//! Run with: cargo test -p open-story --test test_consumers_recover

mod helpers;

use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use open_story::server::{run_server, Config, Role};
use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};

const NATS_PORT: u16 = 4651;
const API_PORT: u16 = 3651;

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
        .args(["-a", "127.0.0.1", "-p", &NATS_PORT.to_string(), "-js", "-sd"])
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

/// Where the node and NATS keep their files: `REGRESS_DIR` when set (a
/// scratch dir left behind for inspection), a temp dir otherwise.
fn scratch_root() -> tempfile::TempDir {
    match std::env::var_os("REGRESS_DIR") {
        Some(dir) => {
            std::fs::create_dir_all(&dir).expect("create REGRESS_DIR");
            tempfile::tempdir_in(dir).expect("scratch dir")
        }
        None => tempfile::tempdir().expect("scratch dir"),
    }
}

async fn consumer_names(bus: &NatsBus, stream: &str) -> Vec<String> {
    let mut s = bus.jetstream().get_stream(stream).await.expect("stream");
    let mut names = Vec::new();
    let mut listed = s.consumer_names();
    while let Some(Ok(n)) = listed.next().await {
        names.push(n);
    }
    names
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

async fn session_event_ids(client: &reqwest::Client, sid: &str) -> Vec<String> {
    let Ok(r) = client
        .get(format!("http://127.0.0.1:{API_PORT}/api/sessions/{sid}/events"))
        .send()
        .await
    else {
        return vec![];
    };
    let body: serde_json::Value = r.json().await.unwrap_or_default();
    body.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Publish one prompt for `sid` and wait for it to land in the store.
async fn lands(bus: &NatsBus, client: &reqwest::Client, sid: &str, within: Duration) -> bool {
    let id = format!("{sid}-evt");
    let batch = IngestBatch {
        session_id: sid.to_string(),
        project_id: "regress".to_string(),
        events: vec![helpers::make_user_prompt(sid, &id)],
    };
    bus.publish(&format!("events.regress.{sid}.main"), &batch)
        .await
        .expect("publish");
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if session_event_ids(client, sid).await.contains(&id) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

mod when_the_node_loses_its_jetstream_consumers_after_serving {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn it_resubscribes_and_the_store_keeps_advancing() {
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

        // The node's own bus, as `serve` builds it (solo: no hub domain).
        let node_bus = NatsBus::connect(&nats_url).await.expect("connect");
        node_bus.ensure_streams().await.expect("streams");
        // The test's own connection: publishes and inspects JetStream.
        let probe = NatsBus::connect(&nats_url).await.expect("connect probe");

        let watch = watch_dir.to_string_lossy().to_string();
        let config = Config {
            role: Role::Consumer,
            host: "127.0.0.1".into(),
            port: API_PORT,
            data_dir: data_dir.to_string_lossy().to_string(),
            nats_url: nats_url.clone(),
            // As on the real nodes; only health reads it in-process.
            nats_leaf_url: "nats://127.0.0.1:4659".into(),
            consumers_start: "serving".into(),
            claude_watch_dir: watch.clone(),
            codex_watch_dir: watch.clone(),
            grok_watch_dir: watch.clone(),
            watch_dir: watch.clone(),
            presence_interval_secs: 1,
            ..Config::default()
        };
        let server = tokio::spawn({
            let data_dir = data_dir.clone();
            let watch_dir = watch_dir.clone();
            async move {
                let bus: std::sync::Arc<dyn Bus> = std::sync::Arc::new(node_bus);
                run_server(
                    "127.0.0.1",
                    API_PORT,
                    &data_dir,
                    None,
                    &[watch_dir],
                    bus,
                    config,
                )
                .await
            }
        });

        let client = reqwest::Client::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let serving = health(&client)
                .await
                .is_some_and(|h| h["boot"]["phase"] == "serving");
            if serving {
                break;
            }
            assert!(Instant::now() < deadline, "node never reached serving");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // Serving: the actors have attached and the store advances.
        assert!(
            lands(&probe, &client, "regress-before", Duration::from_secs(10)).await,
            "an event published once serving lands in the store"
        );
        let events_before = consumer_names(&probe, "events").await;
        let presence_before = consumer_names(&probe, "presence").await;
        assert!(!events_before.is_empty(), "consumers exist on events");
        assert!(!presence_before.is_empty(), "a consumer exists on presence");

        // What the server does to ephemeral push consumers whose connection
        // lost interest past the inactivity threshold.
        for stream in ["events", "local", "presence"] {
            let s = probe.jetstream().get_stream(stream).await.unwrap();
            for name in consumer_names(&probe, stream).await {
                let _ = s.delete_consumer(&name).await;
            }
        }
        assert!(consumer_names(&probe, "events").await.is_empty());

        // The actors must notice, resubscribe, and the store keep advancing.
        let landed = lands(&probe, &client, "regress-after", Duration::from_secs(20)).await;
        let h = health(&client).await.unwrap_or_default();
        let events_after = consumer_names(&probe, "events").await;
        let presence_after = consumer_names(&probe, "presence").await;
        server.abort();
        assert!(
            landed,
            "the store stopped advancing after the consumers were lost; \
             consumers on events={events_after:?} presence={presence_after:?}; \
             health consumers={}",
            h["consumers"]
        );
        assert!(
            !events_after.is_empty(),
            "consumers re-attached on events"
        );
        assert!(
            !presence_after.is_empty(),
            "a consumer re-attached on presence"
        );
    }
}
