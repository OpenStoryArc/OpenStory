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
        Self::start_on(PORT)
    }

    fn start_on(port: u16) -> Option<Self> {
        let bin = which("nats-server")?;
        let dir = tempfile::tempdir().ok()?;
        let child = Command::new(bin)
            .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-js", "-sd"])
            .arg(dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], port).into(),
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

// H-04: per-stream bytes, messages, and the configured cap, with percent.
mod when_streams_exist {
    use super::*;
    use open_story_bus::IngestBatch;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};

    fn event() -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("x".repeat(2_000));
        let data = EventData::with_payload(
            serde_json::json!({}),
            0,
            "s".into(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/s".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some("e1".into()),
            Some("2026-09-24T00:00:00Z".into()),
            None,
            None,
            Some("claude-code".into()),
        )
    }

    #[tokio::test]
    async fn it_reports_bytes_against_caps() {
        let Some(_server) = Scratch::start_on(4398) else {
            eprintln!("skipping: nats-server not on PATH");
            return;
        };
        let bus = NatsBus::connect("nats://127.0.0.1:4398")
            .await
            .expect("connect");
        bus.ensure_streams()
            .await
            .expect("declare the node's streams");
        let batch = IngestBatch {
            session_id: "s".into(),
            project_id: "p".into(),
            events: vec![event()],
        };
        bus.publish("events.host.p.s.main", &batch)
            .await
            .expect("publish");

        let stats = bus.stream_stats().await;
        let events = stats
            .iter()
            .find(|s| s.name == "events")
            .unwrap_or_else(|| panic!("events stream reported: {stats:?}"));
        assert_eq!(events.messages, 1);
        assert!(
            events.bytes > 2_000,
            "bytes reflect the stored batch: {}",
            events.bytes
        );
        assert_eq!(
            events.max_bytes,
            Some(1_073_741_824),
            "the 1 GiB cap the code declares"
        );
        let pct = events.percent().expect("capped streams have a percent");
        assert!(pct > 0.0 && pct < 1.0, "{pct}");
        let names: Vec<&str> = stats.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"patterns") && names.contains(&"ui"),
            "{names:?}"
        );
        assert!(
            stats
                .iter()
                .find(|s| s.name == "ui")
                .unwrap()
                .percent()
                .is_none(),
            "uncapped streams have no percent"
        );
    }
}
