//! O-01: metrics are on by default, and `/metrics` carries the node gauges
//! an operator or a dashboard needs: events ingested by agent, consumer
//! lag and restarts, stream bytes against caps, publish failures by
//! watcher, and the presence beat. Scoreboard: REQUIREMENTS.md

mod helpers;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use helpers::bus::TestActors;
use helpers::{make_event, test_router_with};
use open_story::server::consumers::supervision::{ConsumerHealth, ConsumerState, Driven};
use open_story::server::{presence, AppState, Config, SharedState};
use open_story_bus::{Bus, BusSubscription, IngestBatch, StreamStats};
use open_story_server::metrics;
use open_story_store::state::StoreState;
use tokio::sync::{broadcast, RwLock};

/// A bus that reports one stream and refuses every publish.
struct StatsBus;

#[async_trait]
impl Bus for StatsBus {
    async fn publish(&self, subject: &str, _batch: &IngestBatch) -> Result<()> {
        Err(anyhow::anyhow!("nats: connection closed")
            .context(format!("failed to publish to {subject}")))
    }
    async fn publish_bytes(&self, _subject: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    async fn subscribe(&self, _pattern: &str) -> Result<BusSubscription> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(BusSubscription { receiver: rx })
    }
    async fn replay(&self, _pattern: &str) -> Result<Vec<IngestBatch>> {
        Ok(vec![])
    }
    async fn stream_stats(&self) -> Vec<StreamStats> {
        vec![StreamStats::new("events", 1234, 5, 1_073_741_824)]
    }
}

fn state_with(bus: Arc<dyn Bus>, config: Config, tmp: &tempfile::TempDir) -> SharedState {
    let store = StoreState::new(tmp.path()).unwrap();
    let (broadcast_tx, _) = broadcast::channel(256);
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&watch_dir).unwrap();
    let topology = open_story::server::admin::compute_topology(
        "test-host",
        config.role,
        &open_story::server::admin::EnvInputs::default(),
        &[],
    );
    let (admin_topology_tx, _) = tokio::sync::watch::channel(topology);
    Arc::new(RwLock::new(AppState {
        store,
        transcript_states: HashMap::new(),
        watcher_diagnostics: open_story::server::watcher_diagnostics::WatcherDiagnostics::default(),
        broadcast_tx,
        bus,
        admin_topology_tx,
        config,
        watch_dir,
        account_config_writer: None,
        account_config_reloader: None,
        role_directory: Arc::new(open_story::server::directory::NoopRoleDirectory),
    }))
}

/// The value of `name{labels}` in Prometheus text, if present.
fn sample(text: &str, name: &str, labels: &str) -> Option<f64> {
    let prefix = if labels.is_empty() {
        format!("{name} ")
    } else {
        format!("{name}{{{labels}}} ")
    };
    text.lines()
        .find(|l| l.starts_with(&prefix))
        .and_then(|l| l[prefix.len()..].trim().parse().ok())
}

mod when_node_metrics_are_rendered {
    use super::*;

    /// Pure: the node block renders one sample per label set, with HELP
    /// and TYPE lines, from the same facts /api/health reports.
    #[test]
    fn it_renders_one_sample_per_label_set() {
        let mut consumers = HashMap::new();
        consumers.insert(
            "persist",
            ConsumerHealth {
                alive: true,
                state: ConsumerState::Running,
                restarts: 2,
                last_restart: None,
                last_exit: None,
                lag: 7,
            },
        );
        let node = metrics::NodeMetrics {
            consumers,
            streams: vec![
                StreamStats::new("events", 1234, 5, 1_073_741_824),
                StreamStats::new("ui", 0, 0, -1),
            ],
            publish_failures: vec![("grok".to_string(), 16)],
            presence: presence::PresenceStats {
                beats: 40,
                failures: 3,
                last_error: Some("x".into()),
            },
        };
        let text = metrics::render_node_metrics(&node);
        assert_eq!(
            sample(&text, "openstory_consumer_lag", r#"actor="persist""#),
            Some(7.0),
            "{text}"
        );
        assert_eq!(
            sample(
                &text,
                "openstory_consumer_restarts_total",
                r#"actor="persist""#
            ),
            Some(2.0)
        );
        assert_eq!(
            sample(&text, "openstory_consumer_alive", r#"actor="persist""#),
            Some(1.0)
        );
        assert_eq!(
            sample(&text, "openstory_stream_bytes", r#"stream="events""#),
            Some(1234.0)
        );
        assert_eq!(
            sample(&text, "openstory_stream_max_bytes", r#"stream="events""#),
            Some(1_073_741_824.0)
        );
        assert_eq!(
            sample(&text, "openstory_stream_messages", r#"stream="ui""#),
            Some(0.0)
        );
        assert!(
            sample(&text, "openstory_stream_max_bytes", r#"stream="ui""#).is_none(),
            "no cap, no sample: {text}"
        );
        assert_eq!(
            sample(
                &text,
                "openstory_publish_failures_total",
                r#"watcher="grok""#
            ),
            Some(16.0)
        );
        assert_eq!(
            sample(
                &text,
                "openstory_publish_failures_total",
                r#"watcher="presence""#
            ),
            Some(3.0)
        );
        assert_eq!(
            sample(&text, "openstory_presence_beats_total", ""),
            Some(40.0)
        );
        assert!(
            text.contains("# TYPE openstory_consumer_lag gauge"),
            "{text}"
        );
        assert!(text.contains("# TYPE openstory_consumer_restarts_total counter"));
        assert!(text.contains("# HELP openstory_stream_bytes "));
    }
}

mod when_metrics_are_scraped {
    use super::*;

    #[tokio::test]
    async fn it_exposes_the_node_gauges() {
        let config = Config::default();
        assert!(config.metrics_enabled, "O-01: metrics are on by default");

        let tmp = tempfile::tempdir().unwrap();
        let state = state_with(Arc::new(StatsBus), config.clone(), &tmp);
        // The recorder is installed when the router is built; counters
        // before that go nowhere, so build first.
        let router = test_router_with(state.clone(), &config);

        // Events by agent: two Claude Code, one pi-mono, through persist.
        let other = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&other).await;
        let stamped = |subtype: &str, session: &str, agent: &str| {
            let mut ce = make_event(subtype, session);
            ce.agent = Some(agent.to_string());
            ce
        };
        let pi = stamped("message.user.prompt", "sess-pi", "pi-mono");
        actors
            .persist
            .process_batch(
                "sess-cc",
                &[
                    stamped("message.user.prompt", "sess-cc", "claude-code"),
                    stamped("message.assistant.text", "sess-cc", "claude-code"),
                ],
                Some("p"),
            )
            .await;
        actors
            .persist
            .process_batch("sess-pi", &[pi], Some("p"))
            .await;

        // Consumer lag: three batches queued, one taken, two behind it.
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        for _ in 0..3 {
            tx.send(IngestBatch {
                session_id: "s".into(),
                project_id: "p".into(),
                events: vec![],
            })
            .await
            .unwrap();
        }
        let mut driven = Driven::new("persist", rx);
        driven.next().await;

        // A failed beat, counted under its own watcher label.
        let before = presence::stats().failures;
        let _ = presence::publish_once(&state).await;
        assert_eq!(presence::stats().failures, before + 1);

        let req = Request::get("/metrics").body(Body::empty()).unwrap();
        let resp = tower::ServiceExt::oneshot(router, req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let bytes = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let text = String::from_utf8(bytes.to_vec()).unwrap();

        assert_eq!(
            sample(
                &text,
                "openstory_events_ingested_total",
                r#"agent="claude-code""#
            ),
            Some(2.0),
            "{text}"
        );
        assert_eq!(
            sample(
                &text,
                "openstory_events_ingested_total",
                r#"agent="pi-mono""#
            ),
            Some(1.0)
        );
        assert_eq!(
            sample(&text, "openstory_consumer_lag", r#"actor="persist""#),
            Some(2.0)
        );
        assert_eq!(
            sample(
                &text,
                "openstory_consumer_restarts_total",
                r#"actor="persist""#
            ),
            Some(0.0)
        );
        assert_eq!(
            sample(&text, "openstory_stream_bytes", r#"stream="events""#),
            Some(1234.0)
        );
        assert_eq!(
            sample(&text, "openstory_stream_max_bytes", r#"stream="events""#),
            Some(1_073_741_824.0)
        );
        assert!(
            sample(
                &text,
                "openstory_publish_failures_total",
                r#"watcher="presence""#
            )
            .unwrap_or(0.0)
                >= 1.0,
            "{text}"
        );
        assert!(
            text.contains("openstory_projection_cache_bytes "),
            "the cache gauges still ride along"
        );
    }
}

/// O-03: each event carries a span per stage, translate through persist,
/// with `session_id`, `subject`, and `actor`; sampled at 1 % by default and
/// fully under trace-level logging.
mod when_an_event_flows {
    use super::*;
    use helpers::synth::transcript_line;
    use open_story_core::trace;
    use std::sync::Mutex;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    #[derive(Debug, Default, Clone)]
    struct Seen {
        stage: String,
        event_id: String,
        session_id: String,
        actor: String,
        subject: String,
    }

    struct Fields<'a>(&'a mut Seen);
    impl Visit for Fields<'_> {
        fn record_str(&mut self, f: &Field, v: &str) {
            match f.name() {
                "stage" => self.0.stage = v.to_string(),
                "event_id" => self.0.event_id = v.to_string(),
                "session_id" => self.0.session_id = v.to_string(),
                "actor" => self.0.actor = v.to_string(),
                "subject" => self.0.subject = v.to_string(),
                _ => {}
            }
        }
        fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
            self.record_str(f, format!("{v:?}").trim_matches('"'));
        }
    }

    /// A layer that remembers every `event` span it sees.
    struct Catch(Arc<Mutex<Vec<Seen>>>);
    impl<S: tracing::Subscriber> Layer<S> for Catch {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            _ctx: Context<'_, S>,
        ) {
            if attrs.metadata().name() != "event" {
                return;
            }
            let mut seen = Seen::default();
            attrs.record(&mut Fields(&mut seen));
            self.0.lock().unwrap().push(seen);
        }
    }

    #[tokio::test]
    async fn it_produces_one_span_per_stage() {
        trace::set_sample_rate(1.0);
        let seen = Arc::new(Mutex::new(Vec::new()));
        // At INFO, the configured rate applies; a bare registry would read
        // as trace-level logging and sample everything.
        let sub = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::LevelFilter::INFO)
            .with(Catch(seen.clone()));
        let _g = tracing::subscriber::set_default(sub);

        // translate: the reader's dispatch, the one site every format passes.
        let uuid = "11111111-2222-4333-8444-555555555555";
        let line: serde_json::Value =
            serde_json::from_str(&transcript_line("user_prompt", uuid, None, "sess-t", 1, 64))
                .unwrap();
        let mut state = open_story_core::translate::TranscriptState::new("sess-t".into());
        let events = open_story_core::reader::translate_record(&line, &mut state);
        assert_eq!(events.len(), 1, "one event from one prompt line");
        let ce = &events[0];

        // publish: the watcher marks the batch under its egress subject.
        trace::mark_batch(
            "publish",
            &events,
            Some("events.h.p.sess-t.main"),
            "claude-code",
        );

        // persist: the store marks what landed.
        let tmp = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&tmp).await;
        let r = actors
            .persist
            .process_batch("sess-t", &events, Some("p"))
            .await;
        assert_eq!(r.persisted, 1);

        let spans: Vec<Seen> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.event_id == ce.id)
            .cloned()
            .collect();
        let stages: Vec<&str> = spans.iter().map(|s| s.stage.as_str()).collect();
        assert_eq!(stages, ["translate", "publish", "persist"], "{spans:?}");
        for s in &spans {
            assert_eq!(s.session_id, "sess-t", "{s:?}");
        }
        assert_eq!(
            spans[0].actor, "claude-code",
            "translate is attributed to the producing agent"
        );
        assert_eq!(spans[1].actor, "claude-code");
        assert_eq!(spans[1].subject, "events.h.p.sess-t.main");
        assert_eq!(spans[2].actor, "persist");
        assert!(
            spans[0].subject.is_empty(),
            "no subject before publish: {:?}",
            spans[0]
        );

        // Unsampled: no spans at all for a fresh event.
        trace::set_sample_rate(0.0);
        let before = seen.lock().unwrap().len();
        trace::mark_batch(
            "publish",
            &events,
            Some("events.h.p.sess-t.main"),
            "claude-code",
        );
        assert_eq!(seen.lock().unwrap().len(), before, "rate 0 marks nothing");
        trace::set_sample_rate(1.0);
    }

    #[test]
    fn the_sampler_is_deterministic_and_proportional() {
        assert!(!trace::sampled("any-id", 0.0));
        assert!(trace::sampled("any-id", 1.0));
        let ids: Vec<String> = (0..10_000).map(|i| format!("event-{i}")).collect();
        let hits = ids.iter().filter(|id| trace::sampled(id, 0.01)).count();
        assert!((50..=150).contains(&hits), "about 1 % of 10,000: {hits}");
        let again = ids.iter().filter(|id| trace::sampled(id, 0.01)).count();
        assert_eq!(hits, again, "the same ids sample the same way");
        let half = ids.iter().filter(|id| trace::sampled(id, 0.5)).count();
        assert!((4_500..=5_500).contains(&half), "about half: {half}");
    }

    #[test]
    fn trace_level_logging_samples_everything() {
        assert_eq!(trace::effective_rate(0.01, true), 1.0);
        assert_eq!(trace::effective_rate(0.01, false), 0.01);
        assert_eq!(trace::effective_rate(7.0, false), 1.0, "clamped");
        assert_eq!(trace::effective_rate(-1.0, false), 0.0, "clamped");
    }
}
