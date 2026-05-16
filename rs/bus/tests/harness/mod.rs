//! Hermetic NATS server harness for permission tests.
//!
//! Spawns its own `nats-server` subprocess on a free port with a templated
//! auth config so each test is self-contained. The default `nats_integration`
//! suite assumes an externally-running server; permission tests can't share
//! that — every scenario needs its own auth block.
//!
//! Requires `nats-server` on PATH (`brew install nats-server`).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct NatsServer {
    child: Option<Child>,
    pub port: u16,
    config_path: PathBuf,
    store_dir: PathBuf,
}

impl NatsServer {
    /// Start a nats-server with JetStream enabled and the given auth config
    /// body. The body is appended to a generated header that sets `port` and
    /// `jetstream { store_dir }`, so callers should provide only the `authorization`
    /// (or `accounts`) block.
    pub fn start(auth_block: &str) -> Result<Self, String> {
        let port = pick_free_port()?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let tmp = std::env::temp_dir();
        let config_path = tmp.join(format!("openstory-natsperm-{id}.conf"));
        let store_dir = tmp.join(format!("openstory-natsperm-js-{id}"));
        std::fs::create_dir_all(&store_dir).map_err(|e| format!("create store_dir: {e}"))?;

        let header = format!(
            "port: {port}\n\
             http_port: -1\n\
             jetstream {{\n  store_dir: \"{}\"\n}}\n\n",
            store_dir.display()
        );
        let body = format!("{header}{auth_block}\n");
        std::fs::write(&config_path, body).map_err(|e| format!("write config: {e}"))?;

        let mut child = Command::new("nats-server")
            .arg("-c")
            .arg(&config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn nats-server: {e}"))?;

        // NATS logs to stderr by default. Watch for the "ready" line, but
        // also TCP-probe the port — the log line wording has changed across
        // versions, so the probe is the durable signal.
        let stderr = child.stderr.take().expect("piped stderr");
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                // Surface to test output for debugging permission errors.
                eprintln!("nats-server: {line}");
            }
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() > deadline {
                let _ = child.kill();
                return Err(format!("nats-server on port {port} did not become ready in 10s"));
            }
            if std::net::TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse().unwrap(),
                Duration::from_millis(100),
            )
            .is_ok()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Ok(Self {
            child: Some(child),
            port,
            config_path,
            store_dir,
        })
    }

    pub fn url(&self) -> String {
        format!("nats://127.0.0.1:{}", self.port)
    }

    #[allow(dead_code)]
    pub fn url_with(&self, user: &str, pass: &str) -> String {
        format!("nats://{user}:{pass}@127.0.0.1:{}", self.port)
    }
}

impl Drop for NatsServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.store_dir);
        let _ = std::fs::remove_file(&self.config_path);
    }
}

fn pick_free_port() -> Result<u16, String> {
    let l =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind ephemeral: {e}"))?;
    let port = l
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    drop(l);
    Ok(port)
}
