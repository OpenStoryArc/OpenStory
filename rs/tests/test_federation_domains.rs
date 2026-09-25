//! F-02 (three hubs): JetStream domains on a leaf and a hub, proven in the
//! lab on one machine with scratch nats-servers, no containers.
//!
//! Topology: a hub nats-server (`domain: hub`, a leafnode listener) and a
//! leaf nats-server (`domain: leaf-a`, a leafnode remote to the hub), each
//! with an in-process OpenStory node: the hub node through `connect_hub`
//! (it creates `events-agg`), the leaf node in leaf mode (it creates
//! `events-mirror` and registers its `events` as a source on the hub's
//! aggregate). A second leaf nats-server (`domain: leaf-b`) with a bare
//! leaf bus proves the mirror carries another node's history, not only a
//! reflection of this one's: through the aggregate hop the mirror holds
//! every leaf's events, its own included (the source header names the
//! aggregate, so JetStream's self-origin rule does not drop them), and the
//! node's event-id dedup keeps the store honest.
//!
//! What it proves:
//! 1. the hub's `events-agg` gains a source per leaf domain, and each
//!    source's cursor moves as its leaf publishes (lag returns to 0 and the
//!    aggregate's count grows);
//! 2. the leaf's `events-mirror` fills from the hub with every leaf's
//!    events, and the leaf node reads them (its sessions list the other
//!    leaf's session; its own session is not double-counted);
//! 3. health on both nodes lists the mirror and the aggregate with their
//!    caps and sources, and the node's `jetstream` block names its domain
//!    and the server's `max_file`; `node_streams` through the MCP shows
//!    the same;
//! 4. after the leaf's nats-server is killed and restarted from the same
//!    store, the mirror refills the gap by cursor with no catch-up: no
//!    `OPEN_STORY_CATCH_UP_PEER`, and the `ops` stream stays empty.
//!
//! Ports: hub 4510 (client) / 6510 (leafnodes) / 8510 (monitor); leaf-a
//! 4511 / 8511; leaf-b 4512 / 8512. Store dirs live under
//! `OPEN_STORY_LAB_DIR` when set, else a temp dir.
//!
//!   OPEN_STORY_LAB_DIR=/path/to/scratch cargo test -p open-story \
//!     --test test_federation_domains -- --ignored --nocapture
//!
//! #[ignore]: needs `nats-server` on PATH (or OPEN_STORY_NATS_BIN) and the
//! six loopback ports free.

mod helpers;

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use helpers::make_event_with_time;
use open_story::server::config::Role;
use open_story::server::{run_server, Config};
use open_story_bus::nats_bus::{Federation, FederationPeers, NatsBus};
use open_story_bus::{Bus, IngestBatch};
use serde_json::{json, Value};

const HUB_PORT: u16 = 4510;
const HUB_LEAF_PORT: u16 = 6510;
const HUB_MONITOR: u16 = 8510;
const LEAF_A_PORT: u16 = 4511;
const LEAF_A_MONITOR: u16 = 8511;
const LEAF_B_PORT: u16 = 4512;
const LEAF_B_MONITOR: u16 = 8512;
// JetStream reserves every stream's max_bytes against max_file, and the
// node declares ~2.5 GB of caps (events 1 GB, local 1 GB, patterns 256 MB,
// the 64 MB families), so the lab runs the managed 4GB; the file is sparse.
const HUB_MAX_FILE: i64 = 4_294_967_296;
const LEAF_MAX_FILE: i64 = 4_294_967_296;

fn which(bin: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OPEN_STORY_NATS_BIN") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|c| c.is_file())
    })
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn hub_conf(store: &Path) -> String {
    format!(
        "listen: 127.0.0.1:{HUB_PORT}\nhttp_port: {HUB_MONITOR}\nmax_payload: 8MB\n\
         jetstream {{ store_dir: \"{}\", max_mem: 64MB, max_file: 4GB, domain: hub }}\n\
         leafnodes {{ listen: \"127.0.0.1:{HUB_LEAF_PORT}\" }}\n",
        store.display()
    )
}

fn leaf_conf(port: u16, monitor: u16, domain: &str, store: &Path) -> String {
    format!(
        "listen: 127.0.0.1:{port}\nhttp_port: {monitor}\nmax_payload: 8MB\n\
         jetstream {{ store_dir: \"{}\", max_mem: 64MB, max_file: 4GB, domain: \"{domain}\" }}\n\
         leafnodes {{ remotes [ {{ url: \"nats://127.0.0.1:{HUB_LEAF_PORT}\" }} ] }}\n",
        store.display()
    )
}

/// A scratch nats-server from a config file, restartable from the same
/// store, killed on drop.
struct ScratchNats {
    conf: PathBuf,
    port: u16,
    child: Option<Child>,
}

impl ScratchNats {
    fn start(dir: &Path, name: &str, port: u16, conf: String) -> ScratchNats {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        let conf_path = root.join("nats.conf");
        std::fs::write(&conf_path, conf).unwrap();
        let mut s = ScratchNats { conf: conf_path, port, child: None };
        s.launch();
        s
    }

    fn launch(&mut self) {
        let bin = which("nats-server").expect("nats-server on PATH or OPEN_STORY_NATS_BIN");
        let log = std::fs::File::create(self.conf.with_file_name("nats.log")).unwrap();
        let child = Command::new(bin)
            .arg("-c")
            .arg(&self.conf)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("spawn nats-server");
        self.child = Some(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], self.port).into(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("nats-server on {} never listened", self.port);
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !port_free(self.port) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for ScratchNats {
    fn drop(&mut self) {
        self.kill();
    }
}

/// An in-process node on the given bus: consumer role (no watchers, so no
/// transcript of this machine is read), a free HTTP port, its own data dir.
async fn boot_node(bus: Arc<dyn Bus>, nats_url: &str, monitor: u16, data_dir: &Path) -> String {
    let port = free_port();
    let mut config = Config::default();
    config.role = Role::Consumer;
    config.host = "127.0.0.1".into();
    config.port = port;
    config.nats_url = nats_url.to_string();
    config.nats_monitor_url = format!("http://127.0.0.1:{monitor}");
    config.presence_interval_secs = 2;
    let data_dir = data_dir.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = run_server("127.0.0.1", port, &data_dir, None, &[], bus, config).await {
            eprintln!("node on {port} exited: {e}");
        }
    });
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(r) = reqwest::get(format!("{base}/health")).await {
            if r.status().is_success() {
                return base;
            }
        }
        assert!(Instant::now() < deadline, "node on {port} never answered /health");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn batch(host: &str, session: &str, i: usize) -> IngestBatch {
    let mut ce = make_event_with_time(
        "io.arc.event",
        session,
        &format!("2026-09-25T12:00:{i:02}.000Z"),
    );
    ce.id = format!("{session}-{i}");
    IngestBatch {
        session_id: session.to_string(),
        project_id: "lab".to_string(),
        events: vec![ce.with_host(host)],
    }
}

async fn publish(bus: &NatsBus, host: &str, session: &str, from: usize, n: usize) {
    for i in from..from + n {
        bus.publish(&format!("events.{host}.lab.{session}.main"), &batch(host, session, i))
            .await
            .unwrap_or_else(|e| panic!("publish {i} on {host}: {e}"));
    }
}

/// Raw stream info through the JetStream API (the config's sources carry
/// the external api prefix, the state carries each source's lag).
async fn stream_info(bus: &NatsBus, name: &str) -> Value {
    let js = bus.jetstream();
    js.request(format!("STREAM.INFO.{name}"), &json!({}))
        .await
        .unwrap_or_else(|e| panic!("STREAM.INFO.{name}: {e}"))
}

fn messages(info: &Value) -> u64 {
    info["state"]["messages"].as_u64().unwrap_or(0)
}

/// (`/api/digests` counts the events the store holds per session; the
/// sessions row's `event_count` is a projection snapshot taken at upsert
/// time and can trail by a batch under the actors' eventual consistency.)
/// The (domain, lag) of each source, from `external.api` = `$JS.<d>.API`.
fn sources(info: &Value) -> Vec<(String, u64)> {
    info["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let api = s["external"]["api"].as_str().unwrap_or("");
            let domain = api
                .strip_prefix("$JS.")
                .and_then(|r| r.strip_suffix(".API"))
                .unwrap_or("")
                .to_string();
            (domain, s["lag"].as_u64().unwrap_or(0))
        })
        .collect()
}

async fn eventually<F, Fut>(what: &str, secs: u64, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let last = match check().await {
            Ok(()) => return,
            Err(e) => e,
        };
        assert!(Instant::now() < deadline, "{what} never held within {secs}s; last: {last}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn get(base: &str, path: &str) -> Value {
    reqwest::get(format!("{base}{path}"))
        .await
        .unwrap_or_else(|e| panic!("GET {path}: {e}"))
        .json()
        .await
        .unwrap_or(Value::Null)
}

fn stream_named<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"] == name)
}

mod when_a_leaf_and_a_hub_run_their_own_domains {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn it_aggregates_up_mirrors_down_and_refills_a_gap_by_cursor() {
        // ── the lab ────────────────────────────────────────────────────
        // The leaf node's host token (subjects and the leaf domain).
        std::env::set_var("OPEN_STORY_HOST", "leaf-a");
        std::env::remove_var("OPEN_STORY_CATCH_UP_PEER");
        for p in [HUB_PORT, HUB_LEAF_PORT, HUB_MONITOR, LEAF_A_PORT, LEAF_A_MONITOR, LEAF_B_PORT, LEAF_B_MONITOR] {
            assert!(port_free(p), "port {p} is taken; the lab needs it");
        }
        let tmp = tempfile::tempdir().unwrap();
        let lab: PathBuf = std::env::var_os("OPEN_STORY_LAB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| tmp.path().to_path_buf())
            .join(format!("lab-{}", std::process::id()));
        std::fs::create_dir_all(&lab).unwrap();
        eprintln!("  lab dir: {}", lab.display());

        let _hub_nats = ScratchNats::start(&lab, "nats-hub", HUB_PORT, hub_conf(&lab.join("nats-hub/js")));
        let mut leaf_a_nats = ScratchNats::start(
            &lab,
            "nats-leaf-a",
            LEAF_A_PORT,
            leaf_conf(LEAF_A_PORT, LEAF_A_MONITOR, "leaf-a", &lab.join("nats-leaf-a/js")),
        );
        let _leaf_b_nats = ScratchNats::start(
            &lab,
            "nats-leaf-b",
            LEAF_B_PORT,
            leaf_conf(LEAF_B_PORT, LEAF_B_MONITOR, "leaf-b", &lab.join("nats-leaf-b/js")),
        );
        let hub_url = format!("nats://127.0.0.1:{HUB_PORT}");
        let leaf_a_url = format!("nats://127.0.0.1:{LEAF_A_PORT}");
        let leaf_b_url = format!("nats://127.0.0.1:{LEAF_B_PORT}");

        // The hub node: OPEN_STORY_HUB_DOMAIN=hub on a consumer, as the CLI wires it.
        let hub_bus = Arc::new(NatsBus::connect_hub(&hub_url, "hub").await.expect("hub connect"));
        hub_bus.ensure_streams().await.expect("hub streams");
        hub_bus.ensure_aggregate(&[]).await.expect("hub aggregate");
        let hub = boot_node(hub_bus.clone(), &hub_url, HUB_MONITOR, &lab.join("node-hub")).await;

        // The leaf node: OPEN_STORY_HUB_DOMAIN=hub on a leaf whose NATS has domain leaf-a.
        let leaf_a_bus = Arc::new(
            NatsBus::connect_federation(
                &leaf_a_url,
                Federation { host: "leaf-a".into(), peers: FederationPeers::Hub { hub_domain: "hub".into() } },
            )
            .await
            .expect("leaf-a connect"),
        );
        leaf_a_bus.ensure_streams().await.expect("leaf-a streams (mirror + self-registration)");
        let leaf_a = boot_node(leaf_a_bus.clone(), &leaf_a_url, LEAF_A_MONITOR, &lab.join("node-leaf-a")).await;

        // The second leaf: a bare bus, no node; it only publishes.
        let leaf_b_bus = NatsBus::connect_federation(
            &leaf_b_url,
            Federation { host: "leaf-b".into(), peers: FederationPeers::Hub { hub_domain: "hub".into() } },
        )
        .await
        .expect("leaf-b connect");
        leaf_b_bus.ensure_streams().await.expect("leaf-b streams");

        // ── 1. the aggregate gains a source per leaf, with a moving cursor ──
        let agg = stream_info(&hub_bus, "events-agg").await;
        let mut domains: Vec<String> = sources(&agg).into_iter().map(|(d, _)| d).collect();
        domains.sort();
        assert_eq!(domains, ["leaf-a", "leaf-b"], "self-registration named both leaves: {agg}");

        publish(&leaf_b_bus, "leaf-b", "sess-b", 0, 3).await;
        eventually("events-agg holds leaf-b's 3 with lag 0", 30, || async {
            let agg = stream_info(&hub_bus, "events-agg").await;
            let lag_b = sources(&agg).into_iter().find(|(d, _)| d == "leaf-b").map(|(_, l)| l);
            if messages(&agg) == 3 && lag_b == Some(0) { Ok(()) } else { Err(format!("messages={} sources={:?}", messages(&agg), sources(&agg))) }
        })
        .await;
        publish(&leaf_a_bus, "leaf-a", "sess-a", 0, 2).await;
        eventually("events-agg holds 5 with leaf-a's cursor caught up", 30, || async {
            let agg = stream_info(&hub_bus, "events-agg").await;
            let lag_a = sources(&agg).into_iter().find(|(d, _)| d == "leaf-a").map(|(_, l)| l);
            if messages(&agg) == 5 && lag_a == Some(0) { Ok(()) } else { Err(format!("messages={} sources={:?}", messages(&agg), sources(&agg))) }
        })
        .await;

        // ── 2. the leaf's mirror fills from the hub: both leaves' events, own included ──
        eventually("events-mirror on leaf-a holds the aggregate's 5", 30, || async {
            let m = stream_info(&leaf_a_bus, "events-mirror").await;
            if messages(&m) == 5 { Ok(()) } else { Err(format!("messages={}", messages(&m))) }
        })
        .await;
        let mirror = stream_info(&leaf_a_bus, "events-mirror").await;
        assert_eq!(sources(&mirror).into_iter().map(|(d, _)| d).collect::<Vec<_>>(), ["hub"], "{mirror}");
        eventually("the leaf node lists sess-b through its mirror, sess-a once", 30, || async {
            let s = get(&leaf_a, "/api/digests").await;
            let count = |id: &str| s["sessions"].as_array().into_iter().flatten().find(|x| x["session_id"] == id).and_then(|x| x["count"].as_u64());
            if count("sess-b") == Some(3) && count("sess-a") == Some(2) { Ok(()) } else { Err(format!("sess-b={:?} sess-a={:?}", count("sess-b"), count("sess-a"))) }
        })
        .await;

        // ── 3. health on both nodes lists the mirror and the aggregate with caps and sources ──
        let hub_health = get(&hub, "/api/health").await;
        let agg_h = stream_named(&hub_health, "events-agg").unwrap_or_else(|| panic!("hub health lists events-agg: {hub_health}"));
        assert!(agg_h["max_bytes"].as_i64().unwrap_or(0) > 0, "the aggregate has a cap: {agg_h}");
        let mut agg_src: Vec<&str> = agg_h["sources"].as_array().into_iter().flatten().filter_map(|s| s["domain"].as_str()).collect();
        agg_src.sort();
        assert_eq!(agg_src, ["leaf-a", "leaf-b"], "{agg_h}");
        assert!(agg_h["sources"][0]["lag"].is_u64(), "each source carries its lag: {agg_h}");
        assert_eq!(hub_health["jetstream"]["domain"], "hub", "{}", hub_health["jetstream"]);
        assert_eq!(hub_health["jetstream"]["max_file"], HUB_MAX_FILE, "{}", hub_health["jetstream"]);

        let leaf_health = get(&leaf_a, "/api/health").await;
        let mirror_h = stream_named(&leaf_health, "events-mirror").unwrap_or_else(|| panic!("leaf health lists events-mirror: {leaf_health}"));
        assert!(mirror_h["max_bytes"].as_i64().unwrap_or(0) > 0, "{mirror_h}");
        assert_eq!(mirror_h["sources"][0]["name"], "events-agg", "{mirror_h}");
        assert_eq!(mirror_h["sources"][0]["domain"], "hub", "{mirror_h}");
        assert_eq!(leaf_health["jetstream"]["domain"], "leaf-a", "{}", leaf_health["jetstream"]);
        assert_eq!(leaf_health["jetstream"]["max_file"], LEAF_MAX_FILE, "{}", leaf_health["jetstream"]);
        assert_eq!(leaf_health["verdict"]["level"], "ok", "a healthy federated leaf: {}", leaf_health["verdict"]);

        // node_streams through the MCP shows the same streams with their sources.
        let ns = open_story_mcp::tools::ops::node_streams(&leaf_a, json!({})).await.expect("node_streams");
        let m = stream_named(&ns, "events-mirror").unwrap_or_else(|| panic!("node_streams lists the mirror: {ns}"));
        assert_eq!(m["level"], "ok", "{m}");
        assert_eq!(m["sources"][0]["domain"], "hub", "{m}");
        let ns = open_story_mcp::tools::ops::node_streams(&hub, json!({})).await.expect("node_streams");
        let a = stream_named(&ns, "events-agg").unwrap_or_else(|| panic!("node_streams lists the aggregate: {ns}"));
        assert_eq!(a["sources"].as_array().map(|v| v.len()), Some(2), "{a}");

        // ── 4. a leaf outage: the mirror refills the gap by cursor, no catch-up ──
        leaf_a_nats.kill();
        publish(&leaf_b_bus, "leaf-b", "sess-b", 3, 4).await;
        eventually("events-agg holds 9 while leaf-a is dark", 30, || async {
            let agg = stream_info(&hub_bus, "events-agg").await;
            if messages(&agg) == 9 { Ok(()) } else { Err(format!("messages={}", messages(&agg))) }
        })
        .await;
        leaf_a_nats.launch();
        eventually("events-mirror on leaf-a refills to 9 by cursor", 60, || async {
            let m = stream_info(&leaf_a_bus, "events-mirror").await;
            if messages(&m) == 9 { Ok(()) } else { Err(format!("messages={}", messages(&m))) }
        })
        .await;
        // And the aggregate keeps sourcing the returned leaf.
        publish(&leaf_a_bus, "leaf-a", "sess-a", 2, 1).await;
        eventually("events-agg holds 10 after leaf-a returns", 30, || async {
            let agg = stream_info(&hub_bus, "events-agg").await;
            let lag_a = sources(&agg).into_iter().find(|(d, _)| d == "leaf-a").map(|(_, l)| l);
            if messages(&agg) == 10 && lag_a == Some(0) { Ok(()) } else { Err(format!("messages={} sources={:?}", messages(&agg), sources(&agg))) }
        })
        .await;
        // No hand ran: nothing on either node's ops stream.
        for (name, base) in [("hub", &hub), ("leaf-a", &leaf_a)] {
            let h = get(base, "/api/health").await;
            let ops = stream_named(&h, "ops").unwrap_or_else(|| panic!("{name} lists ops: {h}"));
            assert_eq!(ops["messages"], 0, "no catch-up or other hand ran on {name}: {ops}");
        }
        // The leaf node read the refilled gap: sess-b has all 7 events, and
        // sess-a's own 3 are counted once though they came back through
        // the mirror too.
        eventually("the leaf node holds sess-b's 7 and sess-a's 3 after the outage", 60, || async {
            let s = get(&leaf_a, "/api/digests").await;
            let count = |id: &str| s["sessions"].as_array().into_iter().flatten().find(|x| x["session_id"] == id).and_then(|x| x["count"].as_u64());
            if count("sess-b") == Some(7) && count("sess-a") == Some(3) { Ok(()) } else { Err(format!("sess-b={:?} sess-a={:?}", count("sess-b"), count("sess-a"))) }
        })
        .await;
    }
}
