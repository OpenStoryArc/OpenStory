//! M-06 / M-07 (server half): the tier-1 ops endpoints. Each acts only on
//! what is derived, records an `ops.command.<hand>` event with its result,
//! honours an idempotency key, and refuses while the node is not serving.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod helpers;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use helpers::recording_bus::{test_state_with_bus, RecordingBus};
use helpers::{body_json, make_event, send_request};
use open_story::server::SharedState;
use open_story_server::boot;
use serde_json::{json, Value};

/// The boot phase is process-wide; specs that set it must not overlap.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

async fn post(state: &SharedState, hand: &str, body: Value) -> (u16, Value) {
    let req = Request::post(format!("/api/ops/{hand}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = send_request(state.clone(), req).await;
    let status = resp.status().as_u16();
    (status, body_json(resp).await)
}

async fn seed(state: &SharedState, session: &str, n: usize) -> Vec<Value> {
    let vals: Vec<Value> = (0..n)
        .map(|i| {
            let mut ce = make_event("message.user.prompt", session);
            ce.id = format!("{session}-e{i}");
            serde_json::to_value(ce).unwrap()
        })
        .collect();
    let store = state.read().await.store.event_store.clone();
    store.insert_batch(session, &vals).await.unwrap();
    vals
}

mod when_replaying {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_refuses_with_503() {
        let _serial = serial();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        boot::set_replaying(2, 9, 100);
        let (status, body) = post(
            &state,
            "reproject",
            json!({"session_id": "s", "idempotency_key": "k"}),
        )
        .await;
        boot::set_serving();
        assert_eq!(status, 503, "{body}");
        let err = body["error"].as_str().unwrap();
        assert!(
            err.contains("replaying") && err.contains("serving"),
            "{err}"
        );
        assert!(
            bus.under("ops.").is_empty(),
            "nothing acted, nothing recorded"
        );
    }
}

mod when_reproject_is_called {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_rebuilds_the_session_and_records_a_command() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        seed(&state, "sess-1", 3).await;
        assert!(
            state.read().await.store.projections.get("sess-1").is_none(),
            "cold before"
        );

        let (status, body) = post(
            &state,
            "reproject",
            json!({"session_id": "sess-1", "idempotency_key": "k-1", "author": "mcp", "evidence": ["projections_stale"]}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["hand"], "reproject");
        assert_eq!(body["idempotency_key"], "k-1");
        assert_eq!(body["replayed"], false);
        assert_eq!(body["result"]["sessions_reprojected"], 1, "{body}");
        assert_eq!(body["result"]["events_applied"], 3);
        assert_eq!(body["command_subject"], "ops.command.reproject");
        assert!(
            state.read().await.store.projections.get("sess-1").is_some(),
            "warm after"
        );

        let commands = bus.under("ops.command.");
        assert_eq!(commands.len(), 1, "{commands:?}");
        let (subject, batch) = &commands[0];
        assert_eq!(subject, "ops.command.reproject");
        let ce = &batch.events[0];
        assert_eq!(ce.subtype.as_deref(), Some("ops.command.reproject"));
        assert_eq!(ce.agent.as_deref(), Some("openstory"));
        assert_eq!(ce.data.raw["idempotency_key"], "k-1");
        assert_eq!(ce.data.raw["author"], "mcp");
        assert_eq!(ce.data.raw["evidence"], json!(["projections_stale"]));
        assert_eq!(ce.data.raw["ok"], true);
        assert_eq!(ce.data.raw["result"]["sessions_reprojected"], 1);
        assert!(!subject.starts_with("events."), "never the observed stream");
    }
}

mod when_the_same_key_is_sent_twice {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_does_not_act_again() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        seed(&state, "sess-2", 1).await;
        let args = json!({"session_id": "sess-2", "idempotency_key": "k-twice"});
        let (s1, first) = post(&state, "reproject", args.clone()).await;
        let (s2, second) = post(&state, "reproject", args).await;
        assert_eq!((s1, s2), (200, 200), "{first} {second}");
        assert_eq!(first["replayed"], false);
        assert_eq!(second["replayed"], true, "{second}");
        assert_eq!(
            second["result"], first["result"],
            "the recorded result comes back"
        );
        assert_eq!(
            bus.under("ops.command.").len(),
            1,
            "one command for one key"
        );
    }
}

mod when_verify_is_called {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_compares_store_jsonl_and_fts() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        let vals = seed(&state, "sess-3", 2).await;
        // JSONL has both; FTS has one: the store and the backup agree, FTS is short.
        let data_dir = state.read().await.store.data_dir.clone();
        let jsonl = open_story_store::persistence::SessionStore::new(&data_dir).unwrap();
        jsonl
            .append_batch("sess-3", &vals.iter().collect::<Vec<_>>())
            .unwrap();
        let store = state.read().await.store.event_store.clone();
        store
            .index_fts("sess-3-e0", "sess-3", "message.user.prompt", "hello")
            .await
            .unwrap();

        let (status, body) = post(
            &state,
            "verify",
            json!({"session_id": "sess-3", "idempotency_key": "k-v"}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let r = &body["result"];
        assert_eq!(r["store_events"], 2, "{r}");
        assert_eq!(r["jsonl_lines"], 2);
        assert_eq!(r["fts_documents"], 1);
        // FTS holds only records with text, so it is reported, never the
        // judge: the store and its backup agree, and that is what "agree"
        // means. Found on the live node, 2026-09-24: a session whose store
        // and JSONL matched exactly read as disagreeing.
        assert_eq!(r["agree"], true, "store and JSONL match: {r}");
        assert_eq!(r["fts_unindexed"], 1, "how many events FTS has no text for: {r}");
        assert_eq!(r["session_id"], "sess-3");
        let (_, again) = post(
            &state,
            "verify",
            json!({"session_id": "sess-4", "idempotency_key": "k-v2"}),
        )
        .await;
        assert_eq!(again["result"]["store_events"], 0);
        assert_eq!(again["result"]["jsonl_lines"], 0);
        assert_eq!(
            again["result"]["agree"], true,
            "nothing everywhere still agrees: {again}"
        );
        assert_eq!(bus.under("ops.command.verify").len(), 2);
    }
}

mod when_prune_is_called {
    use super::*;
    use open_story_store::event_store::SessionRow;

    /// A fleet-mirrored session row (host elsewhere) last active at `last`.
    fn row(id: &str, last: &str) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            project_id: None,
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: 1,
            first_event: Some(last.to_string()),
            last_event: Some(last.to_string()),
            host: Some("elsewhere".to_string()),
            user: None,
            origin_agent: None,
            person_id: None,
            principal_id: None,
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_deletes_old_sessions() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        let store = state.read().await.store.event_store.clone();
        for (id, last) in [
            ("old-1", "2025-01-01T00:00:00.000Z"),
            ("old-2", "2025-02-01T00:00:00.000Z"),
        ] {
            seed(&state, id, 1).await;
            store.upsert_session(&row(id, last)).await.unwrap();
        }
        seed(&state, "fresh", 1).await;
        store
            .upsert_session(&row("fresh", &chrono::Utc::now().to_rfc3339()))
            .await
            .unwrap();

        let (status, body) = post(
            &state,
            "prune",
            json!({"older_than_days": 30, "idempotency_key": "k-p"}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["deleted"], 2, "{body}");
        assert_eq!(body["result"]["older_than_days"], 30);
        let left: Vec<String> = store
            .list_sessions()
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(left, ["fresh"]);
        assert_eq!(bus.under("ops.command.prune").len(), 1);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_refuses_zero_days() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_bus(&tmp, Arc::new(RecordingBus::default()));
        let (status, body) = post(
            &state,
            "prune",
            json!({"older_than_days": 0, "idempotency_key": "k-0"}),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert!(body["error"].as_str().unwrap().contains("older_than_days"));
    }
}

mod when_catch_up_has_no_peer {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_says_so() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        let (status, body) = post(&state, "catch_up", json!({"idempotency_key": "k-c"})).await;
        assert_eq!(status, 400, "{body}");
        assert!(body["error"].as_str().unwrap().contains("peer"), "{body}");
        assert!(bus.under("ops.command.").is_empty(), "no act, no command");
    }
}

mod when_the_hand_is_unknown {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_is_404_and_names_the_four() {
        let _serial = serial();
        boot::set_serving();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_bus(&tmp, Arc::new(RecordingBus::default()));
        let (status, body) = post(&state, "restart_node", json!({"idempotency_key": "k"})).await;
        assert_eq!(status, 404, "{body}");
        let err = body["error"].as_str().unwrap();
        for hand in ["reproject", "verify", "catch_up", "prune"] {
            assert!(err.contains(hand), "{err}");
        }
    }
}

/// D-06: the node serves the DORA JSON `scripts/dora.py --write` leaves in
/// its data directory, so the Admin tab can show the four keys.
mod when_dora_is_read {
    use super::*;
    use helpers::test_state;

    async fn get(state: &SharedState) -> (u16, Value) {
        let req = Request::get("/api/dora").body(Body::empty()).unwrap();
        let resp = send_request(state.clone(), req).await;
        let status = resp.status().as_u16();
        (status, body_json(resp).await)
    }

    #[tokio::test]
    async fn it_serves_the_written_file_and_says_how_to_make_one() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let (status, body) = get(&state).await;
        assert_eq!(status, 404, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("dora.py"),
            "{body}"
        );

        let data_dir = state.read().await.store.data_dir.clone();
        let written = json!({
            "generated_at": "2026-09-24T01:00:00Z",
            "windows": {"7": {"window_days": 7, "deployments": 3}, "30": {"window_days": 30, "deployments": 9}}
        });
        std::fs::write(data_dir.join("dora.json"), written.to_string()).unwrap();
        let (status, body) = get(&state).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, written);
    }
}
