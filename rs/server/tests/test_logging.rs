//! L-01: the server initialises `tracing` with a JSON-lines formatter behind
//! `log_format`. An agent reading a node's logs through the API needs one
//! JSON object per line with stable field names; a person at a terminal
//! keeps the text look. Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

use open_story_server::logging::{build_subscriber, LogFormat};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// A `MakeWriter` that appends to a shared buffer so a test can read back
/// exactly what the subscriber wrote.
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
    fn lines(&self) -> Vec<String> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

mod when_log_format_is_json {
    use super::*;

    #[test]
    fn it_emits_one_json_object_per_line() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Json, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            tracing::info!(event = "replay_done", sessions = 3, "replay finished");
            tracing::warn!(event = "stream_near_cap", stream = "events", percent = 91.5, "near cap");
            tracing::debug!(event = "hidden", "filtered out at info");
        });

        let lines = cap.lines();
        assert_eq!(lines.len(), 2, "one line per emitted record above the filter: {lines:?}");

        let first: serde_json::Value = serde_json::from_str(&lines[0]).expect("line 1 is JSON");
        assert_eq!(first["level"], "INFO");
        assert_eq!(first["event"], "replay_done");
        assert_eq!(first["sessions"], 3);
        assert_eq!(first["message"], "replay finished");
        assert!(first["target"].as_str().unwrap().starts_with("test_logging"), "{first}");
        let ts = first["ts"].as_str().expect("ts present");
        assert!(chrono::DateTime::parse_from_rfc3339(ts).is_ok(), "ts is RFC 3339: {ts}");

        let second: serde_json::Value = serde_json::from_str(&lines[1]).expect("line 2 is JSON");
        assert_eq!(second["level"], "WARN");
        assert_eq!(second["stream"], "events");
        assert_eq!(second["percent"], 91.5);
    }
}

mod when_log_format_is_text {
    use super::*;

    #[test]
    fn it_emits_a_human_line_not_json() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Text, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            tracing::info!(event = "boot", "serving on 3002");
        });
        let lines = cap.lines();
        assert_eq!(lines.len(), 1);
        assert!(serde_json::from_str::<serde_json::Value>(&lines[0]).is_err(), "text, not JSON");
        assert!(lines[0].contains("serving on 3002"), "{}", lines[0]);
    }
}

mod when_log_format_is_parsed_from_config {
    use super::*;

    #[test]
    fn it_accepts_text_and_json_case_insensitively_and_rejects_the_rest() {
        assert_eq!("json".parse::<LogFormat>().unwrap(), LogFormat::Json);
        assert_eq!("JSON".parse::<LogFormat>().unwrap(), LogFormat::Json);
        assert_eq!("text".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("".parse::<LogFormat>().unwrap(), LogFormat::Text);
        let err = "yaml".parse::<LogFormat>().unwrap_err();
        assert!(err.contains("yaml"), "error names the bad value: {err}");
    }
}

// L-02: every line carries `event`; inside a consumer it carries `actor`.
mod when_a_consumer_logs {
    use super::*;

    #[test]
    fn it_stamps_actor_and_event() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Json, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            let consumer = tracing::info_span!("consumer", actor = "persist");
            let _g = consumer.enter();
            tracing::info!(event = "session_persisted", session_id = "s1", count = 4, "persisted");
            {
                let inner = tracing::info_span!("batch", subject = "events.host.s1.main");
                let _g2 = inner.enter();
                tracing::warn!(event = "index_failed", "fts index write failed");
            }
        });
        let lines = cap.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");

        let first: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(first["actor"], "persist", "span field rides on the line: {first}");
        assert_eq!(first["event"], "session_persisted");
        assert_eq!(first["session_id"], "s1");
        assert_eq!(first["count"], 4);

        let second: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(second["actor"], "persist", "outer span still applies: {second}");
        assert_eq!(second["subject"], "events.host.s1.main", "inner span field too");
        assert_eq!(second["event"], "index_failed");
    }

    #[test]
    fn it_has_no_actor_outside_a_consumer_and_never_omits_event() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Json, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            tracing::info!("a line someone forgot to name");
        });
        let line: serde_json::Value = serde_json::from_str(&cap.lines()[0]).unwrap();
        assert!(line.get("actor").is_none(), "{line}");
        assert_eq!(line["event"], "unnamed", "an unnamed line is findable, not silent: {line}");
        assert_eq!(line["message"], "a line someone forgot to name");
    }
}
