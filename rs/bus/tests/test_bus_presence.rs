//! P-04 (broker round trip): a presence beat published on its own subject
//! lands on the `presence` stream and reaches a `presence` subscription,
//! never the events stream. Needs `nats-server` on PATH; skips otherwise.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

use open_story_bus::nats_bus::NatsBus;
use open_story_bus::{Bus, IngestBatch};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 4397;

struct Scratch {
    child: std::process::Child,
    _dir: tempfile::TempDir,
}

impl Scratch {
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
            if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200))
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

fn beat() -> IngestBatch {
    let data = EventData::new(
        serde_json::json!({"status": "ok", "host": "node-a"}),
        0,
        "presence:node-a".into(),
    );
    let ce = CloudEvent::new(
        "openstory-node".into(),
        "io.arc.event".into(),
        data,
        Some("node.presence".into()),
        None,
        None,
        None,
        None,
        Some("openstory".into()),
    )
    .with_host("node-a")
    .with_principal_id("dev");
    IngestBatch {
        session_id: "presence:node-a".into(),
        project_id: "openstory-node".into(),
        events: vec![ce],
    }
}

mod when_a_beat_is_published {
    use super::*;

    #[tokio::test]
    async fn it_reaches_the_presence_subscription_and_never_the_events_stream() {
        let Some(_server) = Scratch::start_on(PORT) else {
            eprintln!("skipping: nats-server not on PATH");
            return;
        };
        let bus = NatsBus::connect(&format!("nats://127.0.0.1:{PORT}"))
            .await
            .expect("connect");
        bus.ensure_streams().await.expect("declare streams");

        let mut presence = bus
            .subscribe_stream("presence", "presence.>")
            .await
            .expect("subscribe to presence");
        let mut events = bus.subscribe("events.>").await.expect("subscribe to events");

        bus.publish("presence.node-a.dev", &beat())
            .await
            .expect("publish the beat");

        let got = tokio::time::timeout(Duration::from_secs(5), presence.receiver.recv())
            .await
            .expect("a beat within 5 s")
            .expect("channel open");
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].subtype.as_deref(), Some("node.presence"));
        assert_eq!(got.events[0].host.as_deref(), Some("node-a"));

        assert!(
            tokio::time::timeout(Duration::from_millis(500), events.receiver.recv())
                .await
                .is_err(),
            "nothing on the events subscription: presence is its own family"
        );

        let stats = bus.stream_stats().await;
        let presence_stream = stats
            .iter()
            .find(|s| s.name == "presence")
            .unwrap_or_else(|| panic!("presence stream reported: {stats:?}"));
        assert_eq!(presence_stream.messages, 1);
        assert_eq!(presence_stream.max_bytes, Some(67_108_864), "the 64 MB cap");
        let events_stream = stats.iter().find(|s| s.name == "events").expect("events stream");
        assert_eq!(events_stream.messages, 0, "the beat never landed on events");
    }
}
