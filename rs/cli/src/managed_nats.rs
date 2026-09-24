//! Managed JetStream NATS for the single-command install path.
//!
//! OpenStory requires a JetStream-enabled NATS, but Homebrew's `nats-server`
//! service runs the bare binary (no JetStream) and ignores any config file.
//! So when run under the brew service (or with `--manage-nats`), `open-story
//! serve` brings NATS up itself: it probes the configured URL and, only if
//! nothing is listening, spawns and supervises a `nats-server -js` child.
//!
//! This preserves the "NATS is load-bearing, fail fast" contract — it never
//! falls back to a degraded bus; it just *starts a real one* when absent. Dev
//! flows (`just nats`, an already-running NATS) are untouched: the probe finds
//! the existing server and reuses it.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;

/// Holds a spawned `nats-server` child (if we started one) and kills it on
/// drop. When we reused an existing server, `child` is `None` and drop is a
/// no-op.
pub struct NatsGuard {
    child: Option<std::process::Child>,
}

impl Drop for NatsGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Ensure a JetStream NATS is reachable at `nats_url`. If one is already
/// listening, reuse it. Otherwise spawn a managed `nats-server` (file store
/// under `store_dir`) and wait until it's reachable. Returns a guard that stops
/// any child we started when dropped.
///
/// `leaf_url` selects the networking posture, and the default is deliberately
/// closed: when it's `None`/empty the managed server is a **loopback-only
/// standalone** (`-js -a 127.0.0.1`), the install's no-networking default.
/// When set to a hub URL (e.g. `nats://<token>@hub:7422`) the managed server is
/// launched from a generated **leaf-node config** that federates this machine's
/// events up to that hub and replays everyone else's down — turning the local
/// dashboard into a shared, multi-machine view. Same binary, same store; the URL
/// is the one switch.
pub fn ensure_nats(
    nats_url: &str,
    store_dir: &Path,
    nats_bin: Option<&str>,
    leaf_url: Option<&str>,
) -> Result<NatsGuard> {
    let (host, port) = parse_host_port(nats_url);
    let leaf_url = leaf_url.map(str::trim).filter(|s| !s.is_empty());

    if tcp_reachable(&host, port, Duration::from_millis(500)) {
        eprintln!(
            "  \x1b[2mNATS:\x1b[0m         reusing server already listening on {host}:{port}"
        );
        return Ok(NatsGuard { child: None });
    }

    let bin = find_nats_binary(nats_bin).ok_or_else(|| {
        anyhow::anyhow!(
            "managed NATS requested but no `nats-server` binary found.\n\
             Install it with `brew install nats-server`, or pass --nats-bin <path> \
             (env OPEN_STORY_NATS_BIN)."
        )
    })?;

    std::fs::create_dir_all(store_dir).map_err(|e| {
        anyhow::anyhow!("cannot create NATS store dir {}: {e}", store_dir.display())
    })?;

    let mut command = std::process::Command::new(&bin);
    match leaf_url {
        Some(url) => {
            // Networking on: write a leaf-node config and launch from it. The
            // config carries the hub remote, which bare args can't express.
            let conf_path = store_dir.join("leaf.conf");
            std::fs::write(&conf_path, render_leaf_config(&host, port, store_dir, url)).map_err(
                |e| anyhow::anyhow!("cannot write leaf config {}: {e}", conf_path.display()),
            )?;
            eprintln!(
                "  \x1b[2mNATS:\x1b[0m         starting {} as JetStream leaf on {host}:{port} → hub {}",
                bin.display(),
                redact_url(url),
            );
            command.arg("-c").arg(&conf_path);
        }
        None => {
            eprintln!(
                "  \x1b[2mNATS:\x1b[0m         starting {} with JetStream on {host}:{port} (loopback)",
                bin.display()
            );
            command
                .args(["-js", "-a", &host, "-p", &port.to_string(), "-sd"])
                .arg(store_dir);
        }
    }
    let child = spawn_logged(command, store_dir)
        .map_err(|e| anyhow::anyhow!("failed to spawn nats-server ({}): {e}", bin.display()))?;

    // Wait for the spawned server to start accepting connections.
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if tcp_reachable(&host, port, Duration::from_millis(300)) {
            return Ok(NatsGuard { child: Some(child) });
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // Never came up — don't leave an orphan behind.
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
    anyhow::bail!("managed nats-server did not become reachable on {host}:{port} within 15s")
}

/// Parse a `nats://host:port` URL into `(host, port)`. Defaults port to 4222
/// and normalizes `localhost`/empty host to `127.0.0.1` (so we bind the
/// spawned server to loopback rather than all interfaces).
/// Rotate `nats.log` past this many bytes. One generation is kept as
/// `nats.log.1`; a node that needs more ships its logs elsewhere.
pub const NATS_LOG_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// Open `<store_dir>/nats.log` for appending, rotating it to `nats.log.1`
/// first when it is already past `max_bytes`. The managed NATS child's
/// stdout and stderr go here so a leaf that cannot dial its hub, a config
/// the server rejects, or a JetStream storage error is readable (L-05).
pub fn open_child_log(store_dir: &Path, max_bytes: u64) -> std::io::Result<std::fs::File> {
    let path = store_dir.join("nats.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > max_bytes {
            std::fs::rename(&path, store_dir.join("nats.log.1"))?;
        }
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Spawn `command` with both output streams captured to the store's
/// `nats.log` (see `open_child_log`). Never `Stdio::null()`.
pub fn spawn_logged(
    mut command: std::process::Command,
    store_dir: &Path,
) -> std::io::Result<std::process::Child> {
    let out = open_child_log(store_dir, NATS_LOG_MAX_BYTES)?;
    let err = out.try_clone()?;
    command
        .stdout(std::process::Stdio::from(out))
        .stderr(std::process::Stdio::from(err))
        .spawn()
}

pub fn parse_host_port(nats_url: &str) -> (String, u16) {
    let s = nats_url.strip_prefix("nats://").unwrap_or(nats_url);
    let s = s.split('/').next().unwrap_or(s); // drop any trailing path
                                              // Drop `user:password@` userinfo: credentials belong to the client
                                              // connection, never to the server's listen address.
    let s = s.rsplit_once('@').map_or(s, |(_, host_port)| host_port);
    match s.rsplit_once(':') {
        Some((host, port)) => (normalize_host(host), port.parse().unwrap_or(4222)),
        None => (normalize_host(s), 4222),
    }
}

/// Render a JetStream leaf-node `nats-server` config: a local client listener
/// on `host:port` (where Open Story and local agents connect), local JetStream
/// for replay, and a leafnode remote to `leaf_url` — the shared hub. This is the
/// networking the default install omits: the bare `-js -a` arg form can't
/// express a leafnode remote, so enabling federation means launching from a
/// config file instead. The JetStream budgets mirror `deploy/nats-leaf.conf`
/// (>=1.3GB file store to cover the streams `NatsBus::ensure_streams()` creates).
pub fn render_leaf_config(host: &str, port: u16, store_dir: &Path, leaf_url: &str) -> String {
    format!(
        "# Generated by `open-story serve` — managed JetStream leaf node.\n\
         # Networking is ON because nats_leaf_url is set. Delete the URL to revert\n\
         # to a loopback-only standalone server (the no-networking default).\n\
         listen: {host}:{port}\n\
         \n\
         jetstream {{\n    \
             store_dir: \"{store}\"\n    \
             max_mem: 256MB\n    \
             max_file: 4GB\n\
         }}\n\
         \n\
         max_payload: 8MB\n\
         http_port: 8222\n\
         \n\
         leafnodes {{\n    \
             remotes [\n        \
                 {{ url: \"{leaf_url}\" }}\n    \
             ]\n\
         }}\n",
        store = store_dir.display(),
    )
}

/// Redact the auth token from a `nats://token@host:port` URL for log output, so
/// the leaf token never lands in stdout/stderr or a captured service log.
fn redact_url(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end + 3 => {
            format!("{}***@{}", &url[..scheme_end + 3], &url[at + 1..])
        }
        _ => url.to_string(),
    }
}

fn normalize_host(host: &str) -> String {
    if host.is_empty() || host == "localhost" {
        "127.0.0.1".to_string()
    } else {
        host.to_string()
    }
}

fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let addr = format!("{host}:{port}");
    match addr.to_socket_addrs() {
        Ok(mut addrs) => addrs
            .next()
            .map(|a| TcpStream::connect_timeout(&a, timeout).is_ok())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Locate the `nats-server` binary: an explicit path first (the brew service
/// passes the resolved keg path, since launchd's PATH is minimal), then PATH,
/// then the common Homebrew locations.
fn find_nats_binary(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("nats-server");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    for p in [
        "/opt/homebrew/opt/nats-server/bin/nats-server",
        "/opt/homebrew/bin/nats-server",
        "/usr/local/opt/nats-server/bin/nats-server",
        "/usr/local/bin/nats-server",
    ] {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_port_handles_standard_url() {
        assert_eq!(
            parse_host_port("nats://localhost:4222"),
            ("127.0.0.1".to_string(), 4222)
        );
        assert_eq!(
            parse_host_port("nats://127.0.0.1:4222"),
            ("127.0.0.1".to_string(), 4222)
        );
        assert_eq!(
            parse_host_port("nats://example.com:5555"),
            ("example.com".to_string(), 5555)
        );
    }

    #[test]
    fn parse_host_port_defaults_missing_port() {
        assert_eq!(
            parse_host_port("nats://localhost"),
            ("127.0.0.1".to_string(), 4222)
        );
        assert_eq!(
            parse_host_port("localhost"),
            ("127.0.0.1".to_string(), 4222)
        );
    }

    #[test]
    fn parse_host_port_tolerates_no_scheme_and_trailing_slash() {
        assert_eq!(
            parse_host_port("127.0.0.1:4300/"),
            ("127.0.0.1".to_string(), 4300)
        );
    }

    /// A node using the accounts feature carries `user:password@` in its
    /// nats_url. The credentials are not part of the listen address: rendering
    /// them into `listen:` makes nats-server refuse the config ("could not
    /// parse address string"), and managed mode dies after 15 s with the
    /// child's stderr discarded. Seen on the owner's laptop, 2026-09-23.
    #[test]
    fn parse_host_port_strips_userinfo() {
        assert_eq!(
            parse_host_port("nats://29911d8f-3893:29911d8f-3893-local-dev@localhost:4222"),
            ("127.0.0.1".to_string(), 4222)
        );
        assert_eq!(
            parse_host_port("nats://user:p%40ss@example.com:5555"),
            ("example.com".to_string(), 5555)
        );
        assert_eq!(
            parse_host_port("user:pass@localhost"),
            ("127.0.0.1".to_string(), 4222)
        );
    }

    #[test]
    fn find_nats_binary_prefers_explicit_existing_path() {
        // A real existing file stands in for the binary; resolution returns it.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        assert_eq!(
            find_nats_binary(Some(&path)),
            Some(tmp.path().to_path_buf())
        );
    }

    #[test]
    fn find_nats_binary_ignores_nonexistent_explicit_path() {
        // A bogus explicit path must not be returned verbatim; it falls through
        // to PATH / known locations (which may or may not have nats-server, so
        // we only assert it's not the bogus path).
        let bogus = "/no/such/dir/nats-server";
        assert_ne!(
            find_nats_binary(Some(bogus)),
            Some(PathBuf::from(bogus)),
            "must not return a path that doesn't exist"
        );
    }

    #[test]
    fn leaf_config_binds_local_listener_and_remotes_to_hub() {
        let conf = render_leaf_config(
            "127.0.0.1",
            4222,
            Path::new("/var/openstory/nats"),
            "nats://sekret@hub.example:7422",
        );
        // Local clients (Open Story + local agents) still reach it on loopback.
        assert!(conf.contains("listen: 127.0.0.1:4222"), "conf was: {conf}");
        // Local JetStream for replay, anchored at the managed store dir.
        assert!(conf.contains("jetstream {"));
        assert!(conf.contains("store_dir: \"/var/openstory/nats\""));
        // The federation switch: a leafnode remote pointing at the hub URL.
        assert!(conf.contains("leafnodes {"));
        assert!(conf.contains("url: \"nats://sekret@hub.example:7422\""));
    }

    #[test]
    fn leaf_config_jetstream_budget_covers_ensure_streams() {
        // ensure_streams() hardcodes events=1GB + patterns=256MB; the leaf file
        // store must exceed that or local stream creation crashes on boot.
        let conf = render_leaf_config("127.0.0.1", 4222, Path::new("/tmp/x"), "nats://h:7422");
        assert!(conf.contains("max_file: 4GB"), "conf was: {conf}");
    }

    #[test]
    fn redact_url_hides_token_but_keeps_host() {
        assert_eq!(
            redact_url("nats://deadbeefcafe@debian-16gb-ash-1:7422"),
            "nats://***@debian-16gb-ash-1:7422"
        );
        // No credentials → nothing to redact.
        assert_eq!(redact_url("nats://hub:7422"), "nats://hub:7422");
    }

    // L-05: the child's output is never discarded. It lands in
    // <store_dir>/nats.log, rotated once past a byte limit.
    mod when_child_writes_stderr {
        use super::super::*;

        #[test]
        fn it_lands_in_nats_log() {
            let tmp = tempfile::tempdir().unwrap();
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", "echo out-line; echo err-line 1>&2"]);
            let mut child = spawn_logged(cmd, tmp.path()).expect("spawn");
            child.wait().unwrap();
            let log =
                std::fs::read_to_string(tmp.path().join("nats.log")).expect("nats.log exists");
            assert!(log.contains("out-line"), "stdout captured: {log:?}");
            assert!(log.contains("err-line"), "stderr captured: {log:?}");
        }

        #[test]
        fn it_rotates_the_log_once_past_the_limit() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("nats.log");
            std::fs::write(&path, "x".repeat(20)).unwrap();
            let mut f = open_child_log(tmp.path(), 10).expect("open");
            use std::io::Write;
            writeln!(f, "fresh").unwrap();
            drop(f);
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("nats.log.1")).unwrap(),
                "x".repeat(20)
            );
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "fresh\n",
                "new log starts empty"
            );
            let mut f = open_child_log(tmp.path(), 10).expect("reopen below limit");
            writeln!(f, "again").unwrap();
            drop(f);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "fresh\nagain\n",
                "appends when under the limit"
            );
        }
    }

    // E-07: the managed child's death is noticed within 5 s, logged with its
    // exit code, and the bus reports itself down.
    mod when_child_exits {
        use super::super::*;
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        struct W(Arc<Mutex<Vec<u8>>>);
        impl Write for W {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
            type Writer = W;
            fn make_writer(&'a self) -> W {
                W(self.0.clone())
            }
        }

        #[test]
        fn it_is_noticed() {
            let cap = Capture::default();
            let sub = open_story_server::logging::build_subscriber(
                open_story_server::logging::LogFormat::Json,
                "info",
                cap.clone(),
            );
            let _g = tracing::subscriber::set_default(sub);
            let tmp = tempfile::tempdir().unwrap();
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", "exit 3"]);
            let child = spawn_logged(cmd, tmp.path()).expect("spawn");
            let guard = NatsGuard::watch(child);
            assert!(guard.alive(), "just started");

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while guard.alive() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            assert!(!guard.alive(), "death noticed within 5 s");
            assert!(!open_story_bus::health::nats_child_alive(), "the bus knows");

            let text = String::from_utf8(cap.0.lock().unwrap().clone()).unwrap();
            let line = text
                .lines()
                .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                .find(|l| l["event"] == "nats_child_exited")
                .unwrap_or_else(|| panic!("no nats_child_exited line in {text:?}"));
            assert_eq!(line["level"], "ERROR");
            assert_eq!(line["code"], 3);
            open_story_bus::health::set_nats_child_alive(true); // leave the process flag as we found it
        }
    }
}
