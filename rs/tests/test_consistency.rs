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
use serde_json::{json, Value};

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

// ── C-03: the report, pure ─────────────────────────────────────────────────
//
// `consistency::report(local, peers, interval_secs)` reads snapshots taken
// from health and presence bodies and answers in the verdict's shape:
// `{level, findings: [{id, level, text}]}`, so the header dot, the probe,
// and an agent read it the way they read health.

mod when_the_report_is_computed {
    use super::*;
    use open_story_server::consistency::{report, Snapshot};

    fn snap(host: &str, digests: &[PlacedDigest]) -> Snapshot {
        Snapshot {
            host: host.to_string(),
            rollup: Some(rollup(digests)),
            ..Snapshot::default()
        }
    }

    fn finding_ids(v: &Value) -> Vec<String> {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap().to_string())
            .collect()
    }

    fn finding<'a>(v: &'a Value, id: &str) -> &'a Value {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id)
            .unwrap_or_else(|| panic!("finding {id} in {v}"))
    }

    #[test]
    fn it_is_all_clear_when_sets_and_watermarks_agree() {
        let mut local = snap("node-a", &fleet());
        local
            .watermarks
            .insert("node-a".into(), "2026-09-25T10:00:00.000Z".into());
        let mut peer = snap("node-b", &fleet());
        peer.watermarks
            .insert("node-a".into(), "2026-09-25T10:00:00.000Z".into());
        let v = report(&local, &[peer], 15);
        assert_eq!(v["level"], "ok", "{v}");
        assert_eq!(v["findings"], json!([]));
        assert_eq!(v["host"], "node-a");
        assert_eq!(v["peers"][0]["host"], "node-b");
        assert_eq!(v["peers"][0]["compared"], true);
        assert_eq!(v["peers"][0]["differing_projects"], 0);
    }

    #[test]
    fn it_names_diverged_hosts_with_project_counts() {
        let local = snap("node-a", &fleet());
        let mut grown = fleet();
        grown[1] = placed("node-a", "proj-1", "s2", &["e3", "e3b"]);
        let one_off = snap("node-b", &grown);
        let v = report(&local, &[one_off], 15);
        assert_eq!(finding_ids(&v), ["diverged:node-b"], "{v}");
        let f = finding(&v, "diverged:node-b");
        assert_eq!(f["level"], "warn", "one project apart is warn: {f}");
        assert!(f["text"].as_str().unwrap().contains("1 project"), "{f}");
        assert_eq!(v["level"], "warn");
        assert_eq!(v["peers"][0]["differing_projects"], 1);

        // Past the threshold it is critical: every project differs.
        let other: Vec<PlacedDigest> = fleet()
            .into_iter()
            .map(|mut d| {
                d.digest = digest_event_ids(&ids(&["elsewhere"]));
                d
            })
            .collect();
        let mut far = other;
        far.push(placed("node-c", "proj-9", "s9", &["e9"]));
        far.push(placed("node-c", "proj-8", "s8", &["e8"]));
        let v = report(&local, &[snap("node-b", &far)], 15);
        let f = finding(&v, "diverged:node-b");
        assert_eq!(f["level"], "critical", "{f}");
        assert!(f["text"].as_str().unwrap().contains("5 project"), "{f}");
        assert_eq!(v["level"], "critical");

        // A peer with no roll-up (an older build) is not compared, and says so.
        let mut bare = snap("node-d", &fleet());
        bare.rollup = None;
        let v = report(&local, &[bare], 15);
        assert_eq!(
            v["findings"],
            json!([]),
            "nothing to compare, nothing claimed"
        );
        assert_eq!(v["peers"][0]["compared"], false);
    }

    #[test]
    fn it_names_a_peer_behind_us_with_the_gap() {
        let mut local = snap("node-a", &fleet());
        local
            .watermarks
            .insert("node-a".into(), "2026-09-25T10:00:00.000Z".into());
        let mut behind = snap("node-b", &fleet());
        behind
            .watermarks
            .insert("node-a".into(), "2026-09-25T09:58:00.000Z".into());
        let v = report(&local, &[behind], 15);
        assert_eq!(finding_ids(&v), ["behind:node-b"], "{v}");
        let f = finding(&v, "behind:node-b");
        assert_eq!(f["level"], "warn");
        assert!(f["text"].as_str().unwrap().contains("120 s"), "{f}");

        // Within two beats is the ordinary lag of a 15 s beat, not a finding.
        let mut close = snap("node-b", &fleet());
        close
            .watermarks
            .insert("node-a".into(), "2026-09-25T09:59:45.000Z".into());
        let v = report(&local, &[close], 15);
        assert_eq!(v["findings"], json!([]), "{v}");

        // A peer that has never seen us is not behind, it is unknown.
        let never = snap("node-b", &fleet());
        let v = report(&local, &[never], 15);
        assert_eq!(v["findings"], json!([]), "{v}");
    }

    #[test]
    fn it_flags_consumer_lag_past_one_batch() {
        let mut local = snap("node-a", &fleet());
        local.consumers.insert("persist".into(), 5);
        local.consumers.insert("patterns".into(), 1);
        let v = report(&local, &[], 15);
        assert_eq!(finding_ids(&v), ["lag:persist"], "{v}");
        let f = finding(&v, "lag:persist");
        assert_eq!(f["level"], "warn");
        assert!(f["text"].as_str().unwrap().contains("5 batches"), "{f}");
    }

    #[test]
    fn it_flags_unverified_as_critical() {
        let mut local = snap("node-a", &fleet());
        local.verify = Some(json!({"agree": false, "fts_unindexed": 3, "session_id": "s1"}));
        let v = report(&local, &[], 15);
        assert_eq!(finding_ids(&v), ["unverified"], "{v}");
        assert_eq!(finding(&v, "unverified")["level"], "critical");
        assert_eq!(v["level"], "critical");

        let mut local = snap("node-a", &fleet());
        local.verify = Some(json!({"agree": true, "fts_unindexed": 0}));
        assert_eq!(report(&local, &[], 15)["findings"], json!([]));
    }

    #[test]
    fn it_flags_a_stale_peer_and_ranks_worst_first() {
        let local = snap("node-a", &fleet());
        let mut stale = snap("node-b", &fleet());
        stale.stale = true;
        stale.age_secs = Some(600);
        let mut local_bad = local.clone();
        local_bad.verify = Some(json!({"agree": false}));
        let v = report(&local_bad, &[stale], 15);
        assert_eq!(
            finding_ids(&v),
            ["unverified", "stale_snapshot:node-b"],
            "{v}"
        );
        let f = finding(&v, "stale_snapshot:node-b");
        assert_eq!(f["level"], "warn");
        assert!(f["text"].as_str().unwrap().contains("600 s"), "{f}");
    }

    #[test]
    fn it_reads_snapshots_from_health_and_presence_bodies() {
        let r = rollup(&fleet());
        let health = json!({
            "host": "node-a",
            "rollup": serde_json::to_value(&r).unwrap(),
            "watermarks": {"node-a": "2026-09-25T10:00:00.000Z"},
            "verify": {"agree": true, "fts_unindexed": 0},
            "consumers": {"persist": {"alive": true, "restarts": 0, "lag": 4}},
        });
        let s = Snapshot::from_health(&health);
        assert_eq!(s.host, "node-a");
        assert_eq!(s.rollup.as_ref().unwrap().digest, r.digest);
        assert_eq!(s.watermarks["node-a"], "2026-09-25T10:00:00.000Z");
        assert_eq!(s.verify.as_ref().unwrap()["agree"], true);
        assert_eq!(s.consumers["persist"], 4);
        assert!(!s.stale);

        let node = json!({
            "host": "node-b", "age_secs": 700, "stale": true,
            "body": {"host": "node-b", "rollup": serde_json::to_value(&r).unwrap()},
        });
        let p = Snapshot::from_presence(&node);
        assert_eq!(p.host, "node-b");
        assert!(p.stale);
        assert_eq!(p.age_secs, Some(700));
        assert_eq!(p.rollup.as_ref().unwrap().digest, r.digest);
    }
}

mod when_consistency_is_read {
    use super::*;
    use open_story_store::event_store::PresenceRow;

    #[tokio::test]
    async fn it_reports_against_every_other_beat() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let me = open_story_core::host::host();
        seed_placed(&state, me, "proj-1", "sess-1", 2).await;

        // Alone: all clear.
        let (status, body) = get(&state, "/api/consistency").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["level"], "ok", "{body}");
        assert_eq!(body["host"], me);
        assert_eq!(body["findings"], json!([]));

        // A peer that holds our session and one more (one of two projects
        // differs: warn), and our own beat (which is never a peer).
        let store = state.read().await.store.event_store.clone();
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let other = rollup(&[
            placed(me, "proj-1", "sess-1", &["sess-1-e0", "sess-1-e1"]),
            placed("node-b", "proj-1", "sess-b", &["x"]),
        ]);
        for (host, r) in [("node-b", other), (me, rollup(&[]))] {
            store
                .upsert_presence(&PresenceRow {
                    host: host.to_string(),
                    principal_id: "dev".into(),
                    person_id: None,
                    time: now.clone(),
                    body: json!({"host": host, "rollup": serde_json::to_value(&r).unwrap()}),
                })
                .await
                .unwrap();
        }
        let (_, body) = get(&state, "/api/consistency").await;
        assert_eq!(body["level"], "warn", "{body}");
        let ids: Vec<&str> = body["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["diverged:node-b"], "{body}");
        let f = &body["findings"][0];
        assert_eq!(f["level"], "warn");
        assert!(f["text"].as_str().unwrap().contains("node-b"), "{f}");
        let peers: Vec<&str> = body["peers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["host"].as_str().unwrap())
            .collect();
        assert_eq!(peers, ["node-b"], "our own beat is not a peer: {body}");
    }
}

// ── C-02: watermarks ───────────────────────────────────────────────────────
//
// Per origin host, the newest event time this node has persisted. The
// persist consumer writes the sessions row only after the events are
// durable, and `last_event` merges with MAX, so folding the rows by host
// is the consumer's acknowledged position, keyed by origin, and it
// survives a restart by construction. (Not a bus sequence: the persist
// consumer never sees a subject or a sequence; see the loop log.)

mod when_batches_from_two_hosts_are_persisted {
    use super::*;
    use helpers::bus::TestActors;
    use helpers::make_event_with_time;
    use open_story::server::presence;
    use open_story_server::fleet::watermarks;

    fn stamped(
        host: &str,
        session: &str,
        id: &str,
        time: &str,
    ) -> open_story::cloud_event::CloudEvent {
        let mut ce = make_event_with_time("io.arc.event", session, time);
        ce.id = id.to_string();
        ce.with_host(host)
    }

    #[test]
    fn it_folds_rows_to_the_newest_time_per_host() {
        let row = |id: &str, host: Option<&str>, last: Option<&str>| SessionRow {
            id: id.to_string(),
            project_id: None,
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: 1,
            first_event: None,
            last_event: last.map(str::to_string),
            host: host.map(str::to_string),
            user: None,
            origin_agent: None,
            person_id: None,
            principal_id: None,
        };
        let rows = vec![
            row("s1", Some("node-a"), Some("2026-09-25T10:00:00.000Z")),
            row("s2", Some("node-a"), Some("2026-09-25T10:00:05.000Z")),
            row("s3", Some("node-b"), Some("2026-09-25T09:00:00.000Z")),
            row("s4", Some("node-b"), None),
            row("s5", None, Some("2026-09-25T08:00:00.000Z")),
        ];
        let w = watermarks(&rows);
        assert_eq!(w["node-a"], "2026-09-25T10:00:05.000Z", "{w:?}");
        assert_eq!(w["node-b"], "2026-09-25T09:00:00.000Z");
        assert_eq!(
            w["unknown"], "2026-09-25T08:00:00.000Z",
            "an unplaced row counts"
        );
        assert_eq!(w.len(), 3);
        assert!(watermarks(&[]).is_empty());
    }

    #[tokio::test]
    async fn it_marks_the_newest_time_per_host_and_a_restart_resumes_from_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut actors = TestActors::new(&tmp).await;
        // node-a's newer event lands first, its older one second: the
        // watermark is the newest persisted, not the last arrived.
        actors
            .drive_batch(
                "sess-a",
                &[stamped(
                    "node-a",
                    "sess-a",
                    "a-2",
                    "2026-09-25T10:00:05.000Z",
                )],
                Some("proj-1"),
            )
            .await;
        actors
            .drive_batch(
                "sess-a",
                &[stamped(
                    "node-a",
                    "sess-a",
                    "a-1",
                    "2026-09-25T10:00:00.000Z",
                )],
                Some("proj-1"),
            )
            .await;
        actors
            .drive_batch(
                "sess-b",
                &[stamped(
                    "node-b",
                    "sess-b",
                    "b-1",
                    "2026-09-25T09:00:00.000Z",
                )],
                Some("proj-1"),
            )
            .await;

        let (status, health) = get(&actors.state, "/api/health").await;
        assert_eq!(status, 200, "{health}");
        let w = &health["watermarks"];
        assert_eq!(w["node-a"], "2026-09-25T10:00:05.000Z", "{w}");
        assert_eq!(w["node-b"], "2026-09-25T09:00:00.000Z");
        assert_eq!(w.as_object().unwrap().len(), 2);

        // A restart: a fresh node over the same data dir answers the same,
        // with nothing re-ingested.
        let bus = Arc::new(RecordingBus::default());
        let restarted = test_state_with_bus(&tmp, bus.clone());
        let (_, after) = get(&restarted, "/api/health").await;
        assert_eq!(after["watermarks"], *w, "the watermark is durable: {after}");

        // The beat carries it, being the health body.
        presence::publish_once(&restarted).await.expect("one beat");
        let beat = &bus.under("presence.")[0].1.events[0].data.raw;
        assert_eq!(beat["watermarks"], *w, "{beat}");

        // And the consistency snapshot reads it, so `behind` can be judged.
        let snap = open_story_server::consistency::Snapshot::from_health(&after);
        assert_eq!(snap.watermarks["node-a"], "2026-09-25T10:00:05.000Z");
    }
}
