//! E-05: a watcher publish failure is logged with the subject, the session,
//! and the whole error chain, and counted. Sixteen Grok publishes failed on
//! 2026-09-23 with a line that said only "failed to publish to <subject>",
//! the cause underneath dropped by `{e}`. Scoreboard: REQUIREMENTS.md.

mod helpers;

use open_story_server::logging::{build_subscriber, publish_failed, LogFormat};
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

mod when_publish_fails {
    use super::*;
    use anyhow::Context;

    #[test]
    fn it_logs_subject_and_error() {
        let cap = Capture::default();
        let _g = tracing::subscriber::set_default(build_subscriber(LogFormat::Json, "info", cap.clone()));
        let root = std::io::Error::other("maximum payload exceeded: 9437184 > 8388608");
        let err = anyhow::Error::from(root)
            .context("failed to publish to events.Maxs-Air.proj.sess-1.main");

        publish_failed("grok", "events.Maxs-Air.proj.sess-1.main", "sess-1", 100, &err);

        let text = String::from_utf8(cap.0.lock().unwrap().clone()).unwrap();
        let line: serde_json::Value = serde_json::from_str(text.lines().next().expect("one line")).unwrap();
        assert_eq!(line["event"], "publish_failed");
        assert_eq!(line["level"], "WARN");
        assert_eq!(line["actor"], "grok");
        assert_eq!(line["subject"], "events.Maxs-Air.proj.sess-1.main");
        assert_eq!(line["session_id"], "sess-1");
        assert_eq!(line["events"], 100);
        let error = line["error"].as_str().unwrap();
        assert!(error.contains("failed to publish to"), "outer context kept: {error}");
        assert!(error.contains("maximum payload exceeded"), "root cause kept, never dropped: {error}");
    }
}

mod when_health_is_read {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use helpers::{body_json, send_request, test_state};

    #[tokio::test]
    async fn it_reports_publish_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state, req).await).await;
        assert!(body["publish_failures"].is_u64(), "a count, even when zero: {body}");
    }
}
