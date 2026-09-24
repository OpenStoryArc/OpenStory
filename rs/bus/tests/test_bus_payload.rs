//! A batch larger than the server's max_payload is published in pieces the
//! server accepts, whatever that limit is. A managed standalone nats-server
//! runs the 1 MiB default; the split budget must follow it, not assume 8 MB.
//! Needs `nats-server` on PATH; skips otherwise.

use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 4396;

struct Scratch {
    child: std::process::Child,
    _dir: tempfile::TempDir,
}

impl Scratch {
    fn start_on(port: u16) -> Option<Self> {
        let bin = which("nats-server")?;
        let dir = tempfile::tempdir().ok()?;
        // Flags only, like `--manage-nats` standalone: the payload limit is the default.
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|c| c.is_file())
    })
}

fn event(i: usize) -> CloudEvent {
    let data = EventData::new(
        serde_json::json!({"i": i, "text": "x".repeat(5_000)}),
        0,
        "s".into(),
    );
    CloudEvent::new(
        "arc://test/s".into(),
        "io.arc.event".into(),
        data,
        Some("message.user.prompt".into()),
        Some(format!("e{i}")),
        Some("2026-09-24T00:00:00Z".into()),
        None,
        None,
        Some("claude-code".into()),
    )
}

mod when_a_batch_exceeds_the_servers_limit {
    use super::*;

    #[tokio::test]
    async fn it_arrives_whole_in_pieces_the_server_accepts() {
        let Some(_server) = Scratch::start_on(PORT) else {
            eprintln!("skipping: nats-server not on PATH");
            return;
        };
        let bus = NatsBus::connect(&format!("nats://127.0.0.1:{PORT}"))
            .await
            .expect("connect");
        bus.ensure_streams().await.expect("streams");
        let mut sub = bus.subscribe("events.>").await.expect("subscribe");

        // About 2 MB against a 1 MiB default limit.
        let batch = IngestBatch {
            session_id: "s".into(),
            project_id: "p".into(),
            events: (0..400).map(event).collect(),
        };
        assert!(
            serde_json::to_vec(&batch).unwrap().len() > 1_500_000,
            "the batch is bigger than the default limit"
        );
        bus.publish("events.h.p.s.main", &batch)
            .await
            .expect("publishes in pieces");

        let mut got = 0;
        let deadline = Instant::now() + Duration::from_secs(10);
        while got < 400 && Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), sub.receiver.recv()).await {
                Ok(Some(piece)) => got += piece.events.len(),
                _ => break,
            }
        }
        assert_eq!(got, 400, "every event arrives, across pieces");
    }
}
