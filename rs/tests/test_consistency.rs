//! Consistency hands (docs/research/openstory-as-node/2026-09-25-consistency-hands.md).
//!
//! The store is a grow-only set of immutable events keyed by id; the merge of
//! two nodes' stores is set union; everything else is a fold over that set.
//! These specs pin the checks (pure functions over snapshots) and the repairs
//! (unions and re-folds) that give "consistent" a checkable meaning.
//!
//! C-01: roll-ups. A `DigestRollup` is the per-session digests folded up by
//! host and project, each level a hash of its children in sorted key order,
//! so two nodes with equal sets have equal roll-ups whatever the arrival
//! order, and one added event changes every digest above it.

mod helpers;

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use helpers::recording_bus::{test_state_with_bus, RecordingBus};
use helpers::{body_json, make_event, send_request, test_state};
use open_story::server::SharedState;
use open_story_server::fleet::{digest_event_ids, rollup, PlacedDigest};
use open_story_store::event_store::SessionRow;
use serde_json::Value;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// One session's digest placed under its host and project.
fn placed(host: &str, project: &str, session: &str, event_ids: &[&str]) -> PlacedDigest {
    PlacedDigest {
        host: host.to_string(),
        project: project.to_string(),
        session_id: session.to_string(),
        count: event_ids.len(),
        digest: digest_event_ids(&ids(event_ids)),
    }
}

/// Two hosts, two projects on one of them, four sessions.
fn fleet() -> Vec<PlacedDigest> {
    vec![
        placed("node-a", "proj-1", "s1", &["e1", "e2"]),
        placed("node-a", "proj-1", "s2", &["e3"]),
        placed("node-a", "proj-2", "s3", &["e4", "e5", "e6"]),
        placed("node-b", "proj-1", "s4", &["e7"]),
    ]
}

mod when_the_same_set_arrives_in_another_order {
    use super::*;

    #[test]
    fn it_rolls_up_to_the_same_digest() {
        let forward = fleet();
        let mut reversed = fleet();
        reversed.reverse();
        let a = rollup(&forward);
        let b = rollup(&reversed);
        assert_eq!(a, b, "arrival order must not change the roll-up");
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        assert_eq!(a.sessions, 4);
        assert_eq!(a.events, 7);
        assert_eq!(a.hosts["node-a"].sessions, 3);
        assert_eq!(a.hosts["node-a"].events, 6);
        assert_eq!(a.hosts["node-a"].projects["proj-1"].sessions, 2);
        assert_eq!(a.hosts["node-a"].projects["proj-1"].events, 3);
        assert_eq!(a.hosts["node-b"].projects["proj-1"].sessions, 1);
        assert_eq!(a.digest.len(), 16, "an FNV-1a hex digest: {}", a.digest);
    }
}

mod when_one_event_is_added {
    use super::*;

    #[test]
    fn it_changes_host_project_and_root_digests() {
        let before = rollup(&fleet());
        let mut grown = fleet();
        grown[1] = placed("node-a", "proj-1", "s2", &["e3", "e3b"]);
        let after = rollup(&grown);

        assert_ne!(after.digest, before.digest, "the root digest moves");
        assert_ne!(
            after.hosts["node-a"].digest, before.hosts["node-a"].digest,
            "the host that grew moves"
        );
        assert_ne!(
            after.hosts["node-a"].projects["proj-1"].digest,
            before.hosts["node-a"].projects["proj-1"].digest,
            "the project that grew moves"
        );
        assert_eq!(
            after.hosts["node-a"].projects["proj-2"].digest,
            before.hosts["node-a"].projects["proj-2"].digest,
            "a sibling project is untouched"
        );
        assert_eq!(
            after.hosts["node-b"].digest, before.hosts["node-b"].digest,
            "the other host is untouched"
        );
        assert_eq!(after.events, before.events + 1);
        assert_eq!(after.sessions, before.sessions);
    }
}

mod when_two_nodes_hold_equal_sets {
    use super::*;

    #[test]
    fn their_rollups_are_equal_and_a_missing_session_shows_where() {
        // Node A received s4 last; node B received it first. Same set.
        let node_a = fleet();
        let mut node_b = fleet();
        node_b.rotate_right(1);
        assert_eq!(rollup(&node_a), rollup(&node_b));

        // Node B lacks s4: only node-b's subtree and the root differ.
        let short: Vec<PlacedDigest> = fleet()
            .into_iter()
            .filter(|d| d.session_id != "s4")
            .collect();
        let a = rollup(&node_a);
        let b = rollup(&short);
        assert_ne!(a.digest, b.digest);
        assert_eq!(
            a.hosts["node-a"], b.hosts["node-a"],
            "node-a's subtree agrees"
        );
        assert!(
            !b.hosts.contains_key("node-b"),
            "no sessions, no host entry: {b:?}"
        );

        // An unplaced session (no host, no project) still counts, under `unknown`.
        let mut unplaced = placed("", "", "s9", &["e9"]);
        unplaced.host = String::new();
        unplaced.project = String::new();
        let r = rollup(&[unplaced]);
        assert_eq!(r.hosts["unknown"].projects["unknown"].sessions, 1);
    }
}

/// Seed `n` events for `session` and place the session under `host` / `project`.
async fn seed_placed(state: &SharedState, host: &str, project: &str, session: &str, n: usize) {
    let vals: Vec<Value> = (0..n)
        .map(|i| {
            let mut ce = make_event("message.user.prompt", session);
            ce.id = format!("{session}-e{i}");
            serde_json::to_value(ce).unwrap()
        })
        .collect();
    let store = state.read().await.store.event_store.clone();
    store.insert_batch(session, &vals).await.unwrap();
    store
        .upsert_session(&SessionRow {
            id: session.to_string(),
            project_id: Some(project.to_string()),
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: n as u64,
            first_event: Some("2026-01-01T00:00:00.000Z".to_string()),
            last_event: Some("2026-01-01T00:00:01.000Z".to_string()),
            host: Some(host.to_string()),
            user: None,
            origin_agent: None,
            person_id: None,
            principal_id: None,
        })
        .await
        .unwrap();
}

async fn get(state: &SharedState, path: &str) -> (u16, Value) {
    let req = Request::get(path).body(Body::empty()).unwrap();
    let resp = send_request(state.clone(), req).await;
    let status = resp.status().as_u16();
    (status, body_json(resp).await)
}

mod when_digests_are_read_with_rollup {
    use super::*;

    #[tokio::test]
    async fn it_serves_the_rollup_beside_the_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        seed_placed(&state, "node-a", "proj-1", "sess-a1", 2).await;
        seed_placed(&state, "node-a", "proj-2", "sess-a2", 3).await;
        seed_placed(&state, "node-b", "proj-1", "sess-b1", 1).await;

        let (status, plain) = get(&state, "/api/digests").await;
        assert_eq!(status, 200, "{plain}");
        assert_eq!(plain["sessions"].as_array().unwrap().len(), 3);
        assert!(
            plain.get("rollup").is_none(),
            "without the flag the body is what it always was: {plain}"
        );

        let (status, body) = get(&state, "/api/digests?rollup=1").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["sessions"].as_array().unwrap().len(), 3);
        let r = &body["rollup"];
        assert_eq!(r["sessions"], 3, "{r}");
        assert_eq!(r["events"], 6);
        assert_eq!(r["hosts"]["node-a"]["sessions"], 2);
        assert_eq!(r["hosts"]["node-a"]["events"], 5);
        assert_eq!(r["hosts"]["node-a"]["projects"]["proj-1"]["sessions"], 1);
        assert_eq!(r["hosts"]["node-a"]["projects"]["proj-1"]["events"], 2);
        assert_eq!(r["hosts"]["node-b"]["projects"]["proj-1"]["events"], 1);

        // The served roll-up is the pure fold of the served session digests.
        let expected = rollup(&[
            placed("node-a", "proj-1", "sess-a1", &["sess-a1-e0", "sess-a1-e1"]),
            placed(
                "node-a",
                "proj-2",
                "sess-a2",
                &["sess-a2-e0", "sess-a2-e1", "sess-a2-e2"],
            ),
            placed("node-b", "proj-1", "sess-b1", &["sess-b1-e0"]),
        ]);
        assert_eq!(r["digest"], expected.digest, "{r}");
        assert_eq!(
            r["hosts"]["node-a"]["digest"],
            expected.hosts["node-a"].digest
        );
        assert_eq!(*r, serde_json::to_value(&expected).unwrap());
    }
}

mod when_the_node_beats {
    use super::*;
    use open_story::server::presence;

    #[tokio::test]
    async fn the_body_carries_the_rollup_without_session_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let bus = Arc::new(RecordingBus::default());
        let state = test_state_with_bus(&tmp, bus.clone());
        seed_placed(&state, "node-a", "proj-1", "sess-a1", 2).await;
        seed_placed(&state, "node-b", "proj-1", "sess-b1", 1).await;

        presence::publish_once(&state).await.expect("one beat");
        let beats = bus.under("presence.");
        assert_eq!(beats.len(), 1, "{beats:?}");
        let body = &beats[0].1.events[0].data.raw;
        let r = &body["rollup"];
        assert!(r.is_object(), "the beat carries the roll-up: {body}");
        assert_eq!(r["sessions"], 2, "{r}");
        assert_eq!(r["events"], 3);
        assert_eq!(r["hosts"]["node-a"]["projects"]["proj-1"]["events"], 2);
        assert_eq!(r["hosts"]["node-b"]["projects"]["proj-1"]["events"], 1);
        assert!(
            !r.to_string().contains("sess-a1"),
            "per-host and per-project digests only, no session rows: {r}"
        );

        // The same digest `/api/digests?rollup=1` serves: one fold, two readers.
        let (_, served) = get(&state, "/api/digests?rollup=1").await;
        assert_eq!(served["rollup"]["digest"], r["digest"]);
        assert_eq!(served["rollup"], *r);

        // And `/api/health` carries it too, since the beat is the health body.
        let (_, health) = get(&state, "/api/health").await;
        assert_eq!(
            health["rollup"]["digest"], r["digest"],
            "{}",
            health["rollup"]
        );
    }
}
