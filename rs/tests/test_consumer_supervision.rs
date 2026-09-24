//! E-02: a consumer whose subscription ends logs `event=consumer_ended`
//! with the reason and exits with an error, never silently.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod helpers;

use open_story_bus::IngestBatch;
use open_story_server::consumers::supervision::{ConsumerExit, Driven};
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
        let _guard = tracing::subscriber::set_default(build_subscriber(
            LogFormat::Json,
            "info",
            cap.clone(),
        ));
        let (tx, rx) = tokio::sync::mpsc::channel::<IngestBatch>(8);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();

        let task = tokio::spawn(async move {
            let mut driven = Driven::new("persist", rx);
            while let Some(b) = driven.next().await {
                seen_in.lock().unwrap().push(b.session_id);
            }
            driven.finish()
        });
        tx.send(batch("s1")).await.unwrap();
        tx.send(batch("s2")).await.unwrap();
        drop(tx); // the subscription ends

        let exit = task.await.unwrap();
        assert_eq!(exit, Err(ConsumerExit::SubscriptionClosed { batches: 2 }));
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["s1", "s2"],
            "every batch was handled before the end"
        );

        let lines = cap.json_lines();
        let ended: Vec<&serde_json::Value> = lines
            .iter()
            .filter(|l| l["event"] == "consumer_ended")
            .collect();
        assert_eq!(ended.len(), 1, "{lines:?}");
        assert_eq!(ended[0]["actor"], "persist");
        assert_eq!(ended[0]["reason"], "subscription closed");
        assert_eq!(ended[0]["batches"], 2);
        assert_eq!(
            ended[0]["level"], "ERROR",
            "an ended consumer is an error, not information"
        );
    }

    #[test]
    fn it_explains_itself_as_an_error() {
        let e = ConsumerExit::SubscriptionClosed { batches: 7 };
        assert_eq!(e.to_string(), "subscription closed after 7 batches");
    }
}

// E-03: a supervisor restarts a dead consumer with exponential backoff and
// logs each restart with its attempt; restart counts are kept for health.
mod when_a_consumer_dies {
    use super::*;
    use open_story_server::consumers::supervision::{backoff, stats, supervise};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn it_backs_off_exponentially_and_caps_at_thirty_seconds() {
        let secs: Vec<u64> = (1..=8).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[tokio::test]
    async fn it_is_restarted_with_backoff() {
        let cap = Capture::default();
        let _guard = tracing::subscriber::set_default(build_subscriber(
            LogFormat::Json,
            "info",
            cap.clone(),
        ));
        let starts = Arc::new(AtomicU32::new(0));
        let slept = Arc::new(Mutex::new(Vec::<Duration>::new()));
        let (starts_in, slept_in) = (starts.clone(), slept.clone());

        let task = tokio::spawn(supervise(
            "patterns",
            move || {
                let n = starts_in.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    if n <= 3 {
                        Err(ConsumerExit::SubscriptionClosed { batches: n as u64 })
                    } else {
                        std::future::pending::<Result<(), ConsumerExit>>().await
                    }
                })
            },
            move |d| {
                let slept = slept_in.clone();
                async move {
                    slept.lock().unwrap().push(d);
                }
            },
        ));

        // Three deaths, three restarts, then the fourth run stays up.
        for _ in 0..200 {
            if starts.load(Ordering::SeqCst) >= 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            starts.load(Ordering::SeqCst),
            4,
            "fourth run is alive and pending"
        );
        assert_eq!(
            *slept.lock().unwrap(),
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ],
            "backoff 1 s, 2 s, 4 s before each restart"
        );

        let lines = cap.json_lines();
        let restarts: Vec<&serde_json::Value> = lines
            .iter()
            .filter(|l| l["event"] == "consumer_restarted")
            .collect();
        assert_eq!(restarts.len(), 3, "{lines:?}");
        assert_eq!(
            restarts
                .iter()
                .map(|l| l["attempt"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(restarts[0]["actor"], "patterns");
        assert_eq!(restarts[0]["backoff_ms"], 1000);
        assert_eq!(restarts[2]["backoff_ms"], 4000);
        assert_eq!(restarts[0]["reason"], "subscription closed after 1 batches");
        assert_eq!(restarts[0]["level"], "WARN");

        let health = stats().snapshot();
        let patterns = health
            .get("patterns")
            .expect("stats for the supervised actor");
        assert_eq!(patterns.restarts, 3);
        assert!(patterns.alive, "the fourth run is up");
        assert!(patterns.last_restart.is_some());
        assert_eq!(
            patterns.last_exit.as_deref(),
            Some("subscription closed after 3 batches")
        );

        task.abort();
    }
}

// E-04: restart counts and timestamps per consumer are part of /api/health.
mod when_a_consumer_restarts {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use helpers::{body_json, send_request, test_state};
    use open_story_server::consumers::supervision::supervise;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn it_shows_in_health() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let starts = Arc::new(AtomicU32::new(0));
        let starts_in = starts.clone();
        // A distinct actor name: stats are process-wide and other specs in
        // this binary supervise "patterns".
        let task = tokio::spawn(supervise(
            "e04-probe",
            move || {
                let n = starts_in.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    if n == 1 {
                        Err(ConsumerExit::SubscribeFailed {
                            error: "bus said no".into(),
                        })
                    } else {
                        std::future::pending().await
                    }
                })
            },
            |_d| async {},
        ));
        for _ in 0..200 {
            if starts.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        let probe = &body["consumers"]["e04-probe"];
        assert_eq!(probe["restarts"], 1, "{body}");
        assert_eq!(probe["alive"], true);
        assert!(
            probe["last_restart"]
                .as_str()
                .is_some_and(|s| s.contains('T')),
            "RFC 3339: {probe}"
        );
        assert_eq!(probe["last_exit"], "subscribe failed: bus said no");
        task.abort();
    }
}

// H-05: per-consumer lag (batches waiting in the channel) on /api/health,
// next to alive, restarts, last_restart.
mod when_consumers_run {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use helpers::{body_json, send_request, test_state};

    #[tokio::test]
    async fn it_reports_alive_and_lag() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let (tx, rx) = tokio::sync::mpsc::channel::<IngestBatch>(8);
        for s in ["a", "b", "c"] {
            tx.send(batch(s)).await.unwrap();
        }
        let mut driven = Driven::new("h05-probe", rx);
        let first = driven.next().await.expect("one batch handed out");
        assert_eq!(first.session_id, "a");

        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state, req).await).await;
        let probe = &body["consumers"]["h05-probe"];
        assert_eq!(probe["lag"], 2, "two batches still queued: {body}");
        assert_eq!(probe["restarts"], 0);
        assert!(probe["alive"].is_boolean());
    }
}
