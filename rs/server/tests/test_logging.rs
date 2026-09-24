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
            tracing::warn!(
                event = "stream_near_cap",
                stream = "events",
                percent = 91.5,
                "near cap"
            );
            tracing::debug!(event = "hidden", "filtered out at info");
        });

        let lines = cap.lines();
        assert_eq!(
            lines.len(),
            2,
            "one line per emitted record above the filter: {lines:?}"
        );

        let first: serde_json::Value = serde_json::from_str(&lines[0]).expect("line 1 is JSON");
        assert_eq!(first["level"], "INFO");
        assert_eq!(first["event"], "replay_done");
        assert_eq!(first["sessions"], 3);
        assert_eq!(first["message"], "replay finished");
        assert!(
            first["target"]
                .as_str()
                .unwrap()
                .starts_with("test_logging"),
            "{first}"
        );
        let ts = first["ts"].as_str().expect("ts present");
        assert!(
            chrono::DateTime::parse_from_rfc3339(ts).is_ok(),
            "ts is RFC 3339: {ts}"
        );

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
        assert!(
            serde_json::from_str::<serde_json::Value>(&lines[0]).is_err(),
            "text, not JSON"
        );
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
            tracing::info!(
                event = "session_persisted",
                session_id = "s1",
                count = 4,
                "persisted"
            );
            {
                let inner = tracing::info_span!("batch", subject = "events.host.s1.main");
                let _g2 = inner.enter();
                tracing::warn!(event = "index_failed", "fts index write failed");
            }
        });
        let lines = cap.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");

        let first: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(
            first["actor"], "persist",
            "span field rides on the line: {first}"
        );
        assert_eq!(first["event"], "session_persisted");
        assert_eq!(first["session_id"], "s1");
        assert_eq!(first["count"], 4);

        let second: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(
            second["actor"], "persist",
            "outer span still applies: {second}"
        );
        assert_eq!(
            second["subject"], "events.host.s1.main",
            "inner span field too"
        );
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
        assert_eq!(
            line["event"], "unnamed",
            "an unnamed line is findable, not silent: {line}"
        );
        assert_eq!(line["message"], "a line someone forgot to name");
    }
}

// L-03: a line about a session carries `session_id` as a field.
mod when_persist_logs_a_session {
    use super::*;
    use dashmap::DashMap;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use open_story_server::consumers::persist::PersistConsumer;
    use open_story_store::event_store::EventStore;
    use open_story_store::persistence::SessionStore;
    use open_story_store::plan_store::PlanStore;
    use open_story_store::projection_cache::ProjectionCache;
    use open_story_store::sqlite_store::SqliteStore;

    fn consumer(dir: &std::path::Path) -> PersistConsumer {
        let session_store = SessionStore::new(dir).unwrap();
        let event_store: Arc<dyn EventStore> = Arc::new(SqliteStore::new(dir).unwrap());
        let plan_store = PlanStore::new(&dir.join("plans")).unwrap();
        PersistConsumer::new(
            event_store,
            session_store,
            Arc::new(ProjectionCache::new(u64::MAX, 0)),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
            plan_store,
        )
    }

    fn event(id: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("hello".to_string());
        let data = EventData::with_payload(
            serde_json::json!({}),
            0,
            "sess-log-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/sess-log-1".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(id.to_string()),
            Some("2026-09-23T22:00:00Z".to_string()),
            None,
            None,
            Some("claude-code".into()),
        )
    }

    #[tokio::test]
    async fn it_carries_session_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut persist = consumer(tmp.path());
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Json, "info", cap.clone());
        let _guard = tracing::subscriber::set_default(sub);

        let result = persist
            .process_batch("sess-log-1", &[event("e1"), event("e2")], Some("proj-x"))
            .await;
        assert_eq!(result.persisted, 2);

        let lines: Vec<serde_json::Value> = cap
            .lines()
            .iter()
            .map(|l| serde_json::from_str(l).expect("json line"))
            .collect();
        let batch = lines
            .iter()
            .find(|l| l["event"] == "batch_persisted")
            .unwrap_or_else(|| panic!("no batch_persisted line in {lines:?}"));
        assert_eq!(batch["session_id"], "sess-log-1");
        assert_eq!(batch["persisted"], 2);
        assert_eq!(batch["skipped"], 0);
        assert_eq!(batch["project_id"], "proj-x");
    }
}

// L-04: `log_event` flows through tracing, and the text formatter keeps
// the terminal shape people know: `HH:MM:SS  category  message`, fields after.
mod when_log_format_is_text_after_l04 {
    use super::*;
    use open_story_server::logging::log_event;

    #[test]
    fn it_keeps_time_category_message_shape() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Text, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            {
                let span = tracing::info_span!("consumer", actor = "persist");
                let _g = span.enter();
                tracing::info!(event = "batch_persisted", session_id = "sess-1", persisted = 3, "persisted batch");
            }
            log_event("api", "GET /api/sessions");
        });
        let lines = cap.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        let time = regex::Regex::new(r"^\x1b\[2m\d{2}:\d{2}:\d{2}\x1b\[0m ").unwrap();

        assert!(time.is_match(&lines[0]), "starts with a dim HH:MM:SS: {:?}", lines[0]);
        assert!(lines[0].contains("persist"), "category is the actor: {:?}", lines[0]);
        assert!(lines[0].contains("persisted batch"), "{:?}", lines[0]);
        assert!(lines[0].contains("session_id=sess-1"), "fields follow the message: {:?}", lines[0]);
        assert!(lines[0].contains("persisted=3"), "{:?}", lines[0]);
        assert!(!lines[0].contains("event="), "the event name is the line's identity, not noise: {:?}", lines[0]);

        assert!(time.is_match(&lines[1]), "{:?}", lines[1]);
        assert!(lines[1].contains("api"), "category from log_event's first argument: {:?}", lines[1]);
        assert!(lines[1].contains("GET /api/sessions"), "{:?}", lines[1]);
        assert!(serde_json::from_str::<serde_json::Value>(&lines[1]).is_err());
    }

    #[test]
    fn it_routes_log_event_through_tracing_so_json_sees_it_too() {
        let cap = Capture::default();
        let sub = build_subscriber(LogFormat::Json, "info", cap.clone());
        tracing::subscriber::with_default(sub, || {
            log_event("watch", "watching /Users/x/.claude/projects");
        });
        let line: serde_json::Value = serde_json::from_str(&cap.lines()[0]).unwrap();
        assert_eq!(line["category"], "watch");
        assert_eq!(line["message"], "watching /Users/x/.claude/projects");
        assert_eq!(line["level"], "INFO");
    }
}

// L-07: boot replay reports progress every 10 % (and every 5 s) and a final done.
mod when_replay_runs {
    use super::*;
    use dashmap::DashMap;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
    use open_story_server::consumers::persist::PersistConsumer;
    use open_story_server::ingest::{replay_boot_sessions, ReplayContext, ReplayProgress};
    use open_story_store::event_store::EventStore;
    use open_story_store::payload_cache::PayloadCache;
    use open_story_store::persistence::SessionStore;
    use open_story_store::plan_store::PlanStore;
    use open_story_store::projection_cache::ProjectionCache;
    use open_story_store::sqlite_store::SqliteStore;
    use std::time::{Duration, Instant};

    fn event(session: &str, id: &str) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("hello".to_string());
        let data = EventData::with_payload(
            serde_json::json!({}),
            0,
            session.to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            format!("arc://test/{session}"),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(id.to_string()),
            Some("2026-09-23T22:00:00Z".to_string()),
            None,
            None,
            Some("claude-code".into()),
        )
    }

    #[tokio::test]
    async fn it_logs_progress_and_done() {
        let tmp = tempfile::tempdir().unwrap();
        let event_store: Arc<dyn EventStore> = Arc::new(SqliteStore::new(tmp.path()).unwrap());
        let mut persist = PersistConsumer::new(
            event_store.clone(),
            SessionStore::new(tmp.path()).unwrap(),
            Arc::new(ProjectionCache::new(u64::MAX, 0)),
            Arc::new(DashMap::new()),
            Arc::new(DashMap::new()),
            PlanStore::new(&tmp.path().join("plans")).unwrap(),
        );
        for i in 0..20 {
            let sid = format!("sess-{i:02}");
            persist.process_batch(&sid, &[event(&sid, &format!("e-{i}"))], None).await;
        }

        let ctx = ReplayContext {
            event_store,
            projections: Arc::new(ProjectionCache::new(u64::MAX, 0)),
            subagent_parents: Arc::new(DashMap::new()),
            session_children: Arc::new(DashMap::new()),
            full_payloads: Arc::new(PayloadCache::new(1_000_000)),
            session_projects: Arc::new(DashMap::new()),
            session_project_names: Arc::new(DashMap::new()),
        };
        let cap = Capture::default();
        let _guard = tracing::subscriber::set_default(build_subscriber(LogFormat::Json, "info", cap.clone()));

        replay_boot_sessions(&ctx).await;

        let lines: Vec<serde_json::Value> =
            cap.lines().iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        let progress: Vec<&serde_json::Value> =
            lines.iter().filter(|l| l["event"] == "replay_progress").collect();
        let done: Vec<u64> = progress.iter().map(|l| l["done"].as_u64().unwrap()).collect();
        assert_eq!(done, vec![2, 4, 6, 8, 10, 12, 14, 16, 18, 20], "one line per 10 % of 20 sessions: {lines:?}");
        assert!(progress.iter().all(|l| l["total"] == 20), "{progress:?}");
        assert_eq!(progress.last().unwrap()["percent"], 100);
        assert!(progress.iter().all(|l| l["elapsed_ms"].is_u64()));

        let finished: Vec<&serde_json::Value> = lines.iter().filter(|l| l["event"] == "replay_done").collect();
        assert_eq!(finished.len(), 1, "{lines:?}");
        assert_eq!(finished[0]["sessions"], 20);
        assert_eq!(finished[0]["events"], 20);
        assert!(finished[0]["elapsed_ms"].is_u64());
    }

    #[test]
    fn it_reports_on_time_even_without_a_percent_step() {
        let t0 = Instant::now();
        let mut p = ReplayProgress::new(1000, t0);
        assert!(p.observe(1, t0).is_none(), "1 of 1000 is under 10 % and under 5 s");
        assert!(p.observe(2, t0 + Duration::from_secs(4)).is_none());
        let r = p.observe(3, t0 + Duration::from_secs(6)).expect("5 s elapsed");
        assert_eq!((r.done, r.total, r.percent), (3, 1000, 0));
        assert!(p.observe(4, t0 + Duration::from_secs(7)).is_none(), "clock restarts after a report");
        let r = p.observe(100, t0 + Duration::from_secs(8)).expect("10 % step");
        assert_eq!(r.percent, 10);
    }
}
