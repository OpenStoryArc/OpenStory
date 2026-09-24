//! H-01: `NatsBus::is_active` reflects the real connection. The trait
//! default returned true forever, so `/api/health` said `bus.connected`
//! while the laptop had no NATS at all (2026-09-23).
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

use open_story_bus::nats_bus::NatsBus;
use open_story_bus::Bus;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 4399;

/// A throwaway nats-server on a scratch port, killed on drop.
struct Scratch {
    child: std::process::Child,
    _dir: tempfile::TempDir, // the store dir lives as long as the server
}

impl Scratch {
    fn start() -> Option<Self> {
        let bin = which("nats-server")?;
        let dir = tempfile::tempdir().ok()?;
        let child = Command::new(bin)
            .args(["-a", "127.0.0.1", "-p", &PORT.to_string(), "-js", "-sd"])
            .arg(dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], PORT).into(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return Some(Scratch { child, _dir: dir });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.kill();
    }
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|c| c.is_file())
    })
}

mod when_nats_drops {
    use super::*;

    #[tokio::test]
    async fn it_reports_disconnected() {
        let Some(mut server) = Scratch::start() else {
            eprintln!("skipping: nats-server not on PATH");
            return;
        };
        let bus = NatsBus::connect(&format!("nats://127.0.0.1:{PORT}"))
            .await
            .expect("connect to the scratch server");
        assert!(bus.is_active(), "connected while the server is up");

        server.kill();

        let deadline = Instant::now() + Duration::from_secs(5);
        while bus.is_active() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !bus.is_active(),
            "the bus says disconnected within 5 s of the server dying"
        );
    }
}
