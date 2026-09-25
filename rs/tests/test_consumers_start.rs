//! B-05: consumers start at serving (docs/research/openstory-as-node/
//! 2026-09-25-boot-pass-memory.md).
//!
//! Config `consumers_start = "serving"` (default) holds every supervised
//! consumer until the boot phase flips to serving; `"boot"` is today's
//! behaviour. Health shows `consumers.<name>.state = pending_start` while a
//! supervisor holds, and the verdict does not call a pending actor dead.
//!
//! Run with: cargo test -p open-story --test test_consumers_start

mod helpers;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story_server::boot::{self, ConsumersStart};
use open_story_server::consumers::supervision::supervise_after;
use open_story::server::Config;

/// The boot phase is process-wide; the specs that move it must not overlap.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A supervised consumer whose factory counts its starts and then runs
/// forever; returns the counter and the task.
fn counting_consumer(
    actor: &'static str,
    mode: ConsumersStart,
) -> (Arc<AtomicU32>, tokio::task::JoinHandle<()>) {
    let starts = Arc::new(AtomicU32::new(0));
    let starts_in = starts.clone();
    let task = tokio::spawn(supervise_after(
        actor,
        boot::consumer_gate(mode),
        move || {
            starts_in.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        },
        |_d| async {},
    ));
    (starts, task)
}

async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
}

async fn consumer_state(state: &open_story::server::SharedState, actor: &str) -> serde_json::Value {
    let req = Request::get("/api/health").body(Body::empty()).unwrap();
    let body = body_json(send_request(state.clone(), req).await).await;
    body["consumers"][actor].clone()
}

mod when_the_mode_is_configured {
    use super::*;

    #[test]
    fn it_defaults_to_serving_and_parses_both_values() {
        assert_eq!(Config::default().consumers_start, "serving");
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "consumers_start = \"boot\"\n").unwrap();
        assert_eq!(Config::from_file(&path).consumers_start, "boot");
        assert_eq!("serving".parse::<ConsumersStart>(), Ok(ConsumersStart::Serving));
        assert_eq!("boot".parse::<ConsumersStart>(), Ok(ConsumersStart::Boot));
        assert!(
            "later".parse::<ConsumersStart>().is_err(),
            "an unknown mode is an error, not a silent default"
        );
    }
}

mod when_consumers_start_at_serving {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_holds_every_supervisor_until_the_phase_flips() {
        let _serial = serial();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        boot::set_starting();

        let (starts, task) = counting_consumer("b05-held", ConsumersStart::Serving);
        settle().await;
        assert_eq!(
            starts.load(Ordering::SeqCst),
            0,
            "no subscription before the phase flips"
        );
        let held = consumer_state(&state, "b05-held").await;
        assert_eq!(held["state"], "pending_start", "{held}");
        assert_eq!(held["alive"], false);

        boot::set_replaying(1, 2, 10);
        settle().await;
        assert_eq!(starts.load(Ordering::SeqCst), 0, "replaying is not serving");

        boot::set_serving();
        settle().await;
        assert_eq!(starts.load(Ordering::SeqCst), 1, "started once the node serves");
        let running = consumer_state(&state, "b05-held").await;
        assert_eq!(running["state"], "running", "{running}");
        assert_eq!(running["alive"], true);
        task.abort();
    }
}

mod when_consumers_start_at_boot {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_starts_at_once_as_today() {
        let _serial = serial();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        boot::set_starting();

        let (starts, task) = counting_consumer("b05-eager", ConsumersStart::Boot);
        settle().await;
        assert_eq!(starts.load(Ordering::SeqCst), 1, "boot mode does not wait");
        let running = consumer_state(&state, "b05-eager").await;
        assert_eq!(running["state"], "running", "{running}");
        assert_eq!(running["alive"], true);

        boot::set_serving();
        task.abort();
    }
}

mod when_the_verdict_sees_a_pending_consumer {
    use super::*;
    use open_story_server::node_health::verdict;
    use serde_json::json;

    fn ids(v: &serde_json::Value) -> Vec<String> {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn it_does_not_call_it_dead() {
        let body = json!({
            "boot": {"phase": "replaying", "replay": {"done": 1, "total": 4, "elapsed_ms": 9}},
            "bus": {"connected": true},
            "consumers": {
                "persist": {"alive": false, "state": "pending_start", "restarts": 0, "lag": 0},
                "patterns": {"alive": false, "state": "backoff", "restarts": 2, "lag": 0},
            },
        });
        let v = verdict(&body);
        assert_eq!(
            ids(&v),
            ["consumer_dead:patterns", "replaying"],
            "a pending actor is not a finding; a dead one still is: {v}"
        );
        assert_eq!(v["level"], "critical");

        let mut serving = body.clone();
        serving["boot"]["phase"] = json!("serving");
        serving["consumers"]["patterns"] = json!({"alive": true, "state": "running", "restarts": 0, "lag": 0});
        let v = verdict(&serving);
        assert_eq!(ids(&v), ["consumer_pending:persist"], "{v}");
        assert_eq!(
            v["level"], "warn",
            "a consumer still pending once the node serves is worth a look, not an alarm"
        );
    }
}
