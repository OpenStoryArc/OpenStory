//! E-02: a consumer whose subscription ends logs `event=consumer_ended`
//! with the reason and exits with an error, never silently.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod helpers;

use open_story_bus::IngestBatch;
use open_story_server::consumers::supervision::{drive, ConsumerExit};
use open_story_server::logging::{build_subscriber, LogFormat};
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);
impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}
impl Capture {
    fn json_lines(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

fn batch(session: &str) -> IngestBatch {
    IngestBatch {
        session_id: session.to_string(),
        project_id: String::new(),
        events: vec![],
    }
}

mod when_subscription_ends {
    use super::*;

    #[tokio::test]
    async fn it_logs_and_errors() {
        let cap = Capture::default();
        let _guard = tracing::subscriber::set_default(build_subscriber(LogFormat::Json, "info", cap.clone()));
        let (tx, rx) = tokio::sync::mpsc::channel::<IngestBatch>(8);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();

        let task = tokio::spawn(drive("persist", rx, async move |b: IngestBatch| {
            seen_in.lock().unwrap().push(b.session_id);
        }));
        tx.send(batch("s1")).await.unwrap();
        tx.send(batch("s2")).await.unwrap();
        drop(tx); // the subscription ends

        let exit = task.await.unwrap();
        assert_eq!(exit, Err(ConsumerExit::SubscriptionClosed { batches: 2 }));
        assert_eq!(*seen.lock().unwrap(), vec!["s1", "s2"], "every batch was handled before the end");

        let lines = cap.json_lines();
        let ended: Vec<&serde_json::Value> = lines.iter().filter(|l| l["event"] == "consumer_ended").collect();
        assert_eq!(ended.len(), 1, "{lines:?}");
        assert_eq!(ended[0]["actor"], "persist");
        assert_eq!(ended[0]["reason"], "subscription closed");
        assert_eq!(ended[0]["batches"], 2);
        assert_eq!(ended[0]["level"], "ERROR", "an ended consumer is an error, not information");
    }

    #[test]
    fn it_explains_itself_as_an_error() {
        let e = ConsumerExit::SubscriptionClosed { batches: 7 };
        assert_eq!(e.to_string(), "subscription closed after 7 batches");
    }
}
