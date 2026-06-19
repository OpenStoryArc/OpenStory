//! Scientific validation: OpenStory event federation rides a **Tailscale tailnet**,
//! and the **tag-based ACL** is the real permission boundary.
//!
//! This is the Rust/testcontainers encoding of the hermetic harness at
//! `docs/research/tailnet-federation/harness/run.sh`, which passed 12/12 on
//! native Linux (a1). Method: **direct path observation + falsifiable negative
//! controls (ablations)** — a "B received it" assertion alone proves nothing, so
//! every claim is paired with a control that breaks federation iff the claim holds.
//!
//! Topology (Headscale control server, so it's hermetic — no real Tailscale
//! account, no secrets):
//!   hs-control   headscale + embedded DERP
//!   os-node-a    tailscale sidecar, tag:os-peer -> 100.64.0.1   (nats-hub  shares netns)
//!   os-node-b    tailscale sidecar, tag:os-peer -> 100.64.0.2   (nats-leaf shares netns)
//! The leaf dials `100.64.0.1:7422` — a CGNAT IP routable ONLY via tailscale0.
//!
//! Prerequisites: Docker with `/dev/net/tun` + `NET_ADMIN` (kernel-mode Tailscale).
//! Native Linux CI runners qualify; macOS Docker Desktop also exposes tun.
//! Run: `cargo test -p open-story --test test_tailnet_federation -- --ignored --nocapture`

use serde_json::Value;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const HS: &str = "headscale/headscale:latest";
const TS: &str = "tailscale/tailscale:latest";
const NATS: &str = "nats:2.10-alpine";
const BOX: &str = "natsio/nats-box:latest";
const CURL: &str = "curlimages/curl:latest";
const NET: &str = "os-tailnet-rs"; // distinct from the bash harness's network

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tailnet")
}

/// Run a docker command; return (success, stdout).
fn docker<I, S>(args: I) -> (bool, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Command::new("docker").args(args).output().expect("spawn docker");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
}
fn docker_ok<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    docker(args).0
}

/// `docker exec <c> <cmd...>` returning (success, stdout).
fn dexec(c: &str, cmd: &[&str]) -> (bool, String) {
    let mut a = vec!["exec".to_string(), c.to_string()];
    a.extend(cmd.iter().map(|s| s.to_string()));
    docker(a)
}
/// Detached `docker exec -d`.
fn dexec_d(c: &str, cmd: &[&str]) {
    let mut a = vec!["exec".to_string(), "-d".to_string(), c.to_string()];
    a.extend(cmd.iter().map(|s| s.to_string()));
    let _ = docker(a);
}
/// `docker logs` with stdout + stderr combined (nats CLI writes to both).
fn docker_logs(name: &str) -> String {
    let out = Command::new("docker").args(["logs", name]).output().expect("logs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ── stack lifecycle ────────────────────────────────────────────────────────
struct Stack;

impl Drop for Stack {
    fn drop(&mut self) {
        teardown();
    }
}

fn teardown() {
    let _ = docker([
        "rm", "-f", "hs-control", "os-node-a", "os-node-b", "nats-hub", "nats-leaf", "os-sub1",
    ]);
    let _ = docker(["volume", "rm", "hs-data-rs"]);
    let _ = docker(["network", "rm", NET]);
}

/// Bring up the full stack with the given ACL fixture file. Mirrors `run.sh up`.
fn up(acl_file: &str) -> Stack {
    teardown();
    assert!(docker_ok(["network", "create", NET]), "create network {NET}");

    let fx = fixtures();
    let cfg_mount = format!("{}:/etc/headscale/config.yaml:ro", fx.join("config.yaml").display());
    let acl_mount = format!("{}:/etc/headscale/acl.json:ro", fx.join(acl_file).display());
    assert!(
        docker_ok(vec![
            "run".into(), "-d".into(), "--name".into(), "hs-control".into(),
            "--network".into(), NET.into(),
            "-v".into(), cfg_mount, "-v".into(), acl_mount,
            "-v".into(), "hs-data-rs:/var/lib/headscale".into(),
            HS.into(), "serve".into(),
        ] as Vec<String>),
        "headscale up"
    );
    sleep(Duration::from_secs(4));
    let _ = dexec("hs-control", &["headscale", "users", "create", "club"]);

    // Mint a preauth key WITH tag:os-peer stamped on it -> nodes register
    // pre-tagged and inherit the restrictive filter from birth.
    let (_, key_json) = dexec(
        "hs-control",
        &["headscale", "preauthkeys", "create", "--user", "1", "--reusable", "--tags", "tag:os-peer", "--expiration", "24h", "-o", "json"],
    );
    let key = serde_json::from_str::<Value>(&key_json)
        .ok()
        .and_then(|v| v.get("key").and_then(|k| k.as_str()).map(String::from))
        .expect("mint tagged preauth key");

    for n in ["a", "b"] {
        let name = format!("os-node-{n}");
        let authkey = format!("TS_AUTHKEY={key}");
        let extra = "TS_EXTRA_ARGS=--login-server=http://hs-control:8080 --accept-routes".to_string();
        let hostenv = format!("TS_HOSTNAME=os-node-{n}");
        assert!(
            docker_ok(vec![
                "run".into(), "-d".into(), "--name".into(), name.clone(),
                "--network".into(), NET.into(), "--hostname".into(), name,
                "--cap-add".into(), "NET_ADMIN".into(), "--device".into(), "/dev/net/tun".into(),
                "-e".into(), authkey, "-e".into(), extra, "-e".into(), hostenv,
                "-e".into(), "TS_USERSPACE=false".into(), "-e".into(), "TS_STATE_DIR=/var/lib/tailscale".into(),
                TS.into(),
            ] as Vec<String>),
            "tailscale node {n} up"
        );
    }
    sleep(Duration::from_secs(6));

    let hub_mount = format!("{}:/etc/nats/hub.conf:ro", fx.join("hub.conf").display());
    let leaf_mount = format!("{}:/etc/nats/leaf.conf:ro", fx.join("leaf.conf").display());
    assert!(
        docker_ok(vec![
            "run".into(), "-d".into(), "--name".into(), "nats-hub".into(),
            "--network".into(), "container:os-node-a".into(),
            "-v".into(), hub_mount, NATS.into(), "-c".into(), "/etc/nats/hub.conf".into(), "-js".into(),
        ] as Vec<String>),
        "nats hub up"
    );
    assert!(
        docker_ok(vec![
            "run".into(), "-d".into(), "--name".into(), "nats-leaf".into(),
            "--network".into(), "container:os-node-b".into(),
            "-v".into(), leaf_mount, NATS.into(), "-c".into(), "/etc/nats/leaf.conf".into(), "-js".into(),
        ] as Vec<String>),
        "nats leaf up"
    );
    Stack
}

// ── observation helpers (the "instruments") ────────────────────────────────
fn leafz() -> Value {
    let (_, out) = docker([
        "run", "--rm", "--network", "container:os-node-a", CURL, "-s", "http://127.0.0.1:8222/leafz",
    ]);
    serde_json::from_str(&out).unwrap_or(Value::Null)
}
fn leaf_count() -> u64 {
    leafz().get("leafnodes").and_then(Value::as_u64).unwrap_or(0)
}
/// The remote IP the hub sees for its leaf — its own testimony of the path.
fn leaf_ip() -> String {
    leafz()
        .get("leafs")
        .and_then(|l| l.get(0))
        .and_then(|x| x.get("ip"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
fn leaf_ip_is_cgnat() -> bool {
    let ip = leaf_ip();
    let o: Vec<u8> = ip.split('.').filter_map(|p| p.parse().ok()).collect();
    o.len() == 4 && o[0] == 100 && (64..=127).contains(&o[1]) // 100.64.0.0/10
}
/// Live ESTABLISHED socket to 100.64.0.1:7422 in node-b's netns.
/// /proc/net/tcp: 100.64.0.1 -> hex 01004064, port 7422 -> 1CFE, state 01.
fn estab_to_hub() -> bool {
    let (_, out) = dexec("os-node-b", &["cat", "/proc/net/tcp"]);
    out.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 3 && f[2].eq_ignore_ascii_case("01004064:1CFE") && f[3] == "01"
    })
}
fn tcp_over_tailnet(port: u16) -> bool {
    let inner = format!("timeout 4 nc -w 3 100.64.0.1 {port} </dev/null");
    dexec("os-node-b", &["sh", "-c", &inner]).0
}
/// node-a's compiled packet filter is exactly TCP/UDP to :7422 and nothing else.
fn filter_is_7422_only() -> bool {
    let (_, nm) = dexec("os-node-a", &["tailscale", "debug", "netmap"]);
    let v: Value = match serde_json::from_str(&nm) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut ports: HashSet<(Option<u64>, Option<u64>)> = HashSet::new();
    if let Some(pf) = v.get("PacketFilter").and_then(Value::as_array) {
        for r in pf {
            if let Some(ds) = r.get("Dsts").and_then(Value::as_array) {
                for x in ds {
                    if let Some(po) = x.get("Ports") {
                        ports.insert((
                            po.get("First").and_then(Value::as_u64),
                            po.get("Last").and_then(Value::as_u64),
                        ));
                    }
                }
            }
        }
    }
    ports.len() == 1 && ports.contains(&(Some(7422), Some(7422)))
}
/// Publish on the leaf (node-b), receive on the hub (node-a).
fn msg_crosses() -> bool {
    let _ = docker(["rm", "-f", "os-sub1"]);
    let _ = docker([
        "run", "-d", "--name", "os-sub1", "--network", "container:os-node-a", BOX, "sh", "-c",
        "nats sub 'events.>' --count=1 -s nats://127.0.0.1:4222",
    ]);
    sleep(Duration::from_secs(2));
    let _ = docker([
        "run", "--rm", "--network", "container:os-node-b", BOX, "nats", "pub", "events.probe", "x",
        "-s", "nats://127.0.0.1:4222",
    ]);
    sleep(Duration::from_secs(2));
    docker_logs("os-sub1").contains("events.probe")
}

// ── pollers (the system is eventually-consistent — poll, never fixed-sleep) ──
fn wait_leaf(want: u64, secs: u64) -> bool {
    for _ in 0..secs {
        if leaf_count() == want {
            return true;
        }
        sleep(Duration::from_secs(1));
    }
    leaf_count() == want
}
fn wait_no_estab(secs: u64) -> bool {
    for _ in 0..secs {
        if !estab_to_hub() {
            return true;
        }
        sleep(Duration::from_secs(1));
    }
    !estab_to_hub()
}

// ── the controlled experiment ──────────────────────────────────────────────
/// H1: federation rides the tailnet (not a bridge). H2: the tag-ACL is the
/// boundary. 12 assertions; every negative control must falsify federation
/// exactly when the tailnet (E3) or the ACL (E4) is removed.
#[test]
#[ignore] // requires Docker with /dev/net/tun + NET_ADMIN
fn tailnet_federation_controlled_experiment() {
    // ---- E1/E2/E3 on the clubhouse-ACL stack ----
    {
        let _stack = up("acl.json");
        assert!(wait_leaf(1, 45), "leaf never connected under the clubhouse ACL");

        // E1 POSITIVE — federation works and the path IS the tailnet
        assert_eq!(leaf_count(), 1, "E1a: hub should see exactly 1 leaf connection");
        assert!(leaf_ip_is_cgnat(), "E1b: hub should see leaf from a 100.64/10 CGNAT IP, got '{}'", leaf_ip());
        assert!(estab_to_hub(), "E1c: expected a live ESTAB socket to 100.64.0.1:7422 in node-b");
        assert!(msg_crosses(), "E1d: a published event should cross leaf->hub");

        // E2 NEGATIVE CONTROL — the filter is port-scoped (no lateral movement)
        dexec_d("os-node-a", &["sh", "-c", "while true; do echo OPEN | nc -l -p 9999; done"]);
        sleep(Duration::from_secs(1));
        assert!(tcp_over_tailnet(7422), "E2a: :7422 over the tailnet should connect");
        assert!(!tcp_over_tailnet(9999), "E2b: :9999 over the tailnet MUST be refused by the ACL");
        assert!(filter_is_7422_only(), "E2c: compiled packet filter should be TCP/UDP :7422 only");

        // E3 ABLATION (causal, reversible) — bidirectional partition of the hub IP.
        // tailscale-down is NOT used (it kills containerboot/the node); iptables is surgical.
        // We assert FUNCTIONAL severance (a message can't cross) rather than the hub's
        // leaf-count — NATS dead-connection bookkeeping is ping-timeout-lazy and
        // platform-variable; "does an event actually cross?" is the real capability and
        // is deterministic. The socket vanishing (E3a) is the immediate causal signal.
        // Block at the INTERFACE level (drop everything on tailscale0) — an
        // unambiguous, routing-independent severance of the entire tailnet. (A
        // dest-IP match on 100.64.0.1 worked on native Linux but flapped on macOS
        // Docker Desktop, where overlay-IP routing interacts oddly with iptables.)
        let _ = dexec("os-node-b", &["iptables", "-I", "OUTPUT", "-o", "tailscale0", "-j", "DROP"]);
        let _ = dexec("os-node-b", &["iptables", "-I", "INPUT", "-i", "tailscale0", "-j", "DROP"]);
        assert!(wait_no_estab(45), "E3a: the socket to 100.64.0.1:7422 must vanish when the tailnet path is cut");
        assert!(!msg_crosses(), "E3b: with the tailnet path cut, a published event must NOT cross (federation severed)");
        let _ = dexec("os-node-b", &["iptables", "-D", "OUTPUT", "-o", "tailscale0", "-j", "DROP"]);
        let _ = dexec("os-node-b", &["iptables", "-D", "INPUT", "-i", "tailscale0", "-j", "DROP"]);
        assert!(wait_leaf(1, 60), "E3c: the leaf must reconnect once the tailnet path returns");
        assert!(msg_crosses(), "E3d: after the path returns, events cross again (reversible tailnet switch)");
    }

    // ---- E4 on a rebuilt deny-:7422 stack ----
    {
        let _stack = up("acl-deny.json");
        // Give it the SAME chance to connect as the positive case; it must NOT.
        let connected = wait_leaf(1, 30);
        assert!(!connected, "E4a: the leaf must NEVER connect when the ACL denies :7422");
        assert_eq!(leaf_count(), 0, "E4a: leaf_count should be 0 under the deny policy");
        assert!(!tcp_over_tailnet(7422), "E4b: :7422 over the tailnet must be refused under the deny policy");
    }
}
