//! The property (consistency C-06).
//!
//! A recorded transcript is translated through the real translator and
//! replayed through two in-memory nodes under an injected partition: each
//! node observes a different, overlapping slice, in a different arrival
//! order. The partition heals and each node converges against the other.
//! Afterwards the roll-ups are equal, every session's projection is equal
//! (the fold is a function of the set, not of arrival order), the
//! consistency report reads all clear under a fake clock and reports the
//! peer stale when that clock moves on, no event outside the recording
//! exists on either node, and no hand published to `events.*`: the only
//! `events.*` publishers are the spec's own observation and the catch-up
//! re-injection, both carrying recorded history.

mod helpers;

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use helpers::two_nodes::Node;
use open_story::cloud_event::CloudEvent;
use open_story::server::presence;
use open_story_server::consistency;
use open_story_core::translate::{translate_line, TranscriptState};
use open_story_server::boot;
use open_story_store::event_store::PresenceRow;
use serde_json::{json, Value};

const SESSION: &str = "synth-origin";
const BATCH: usize = 10;

/// The boot phase is process-wide; specs that set it must not overlap.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The recorded sample through the real translator.
fn recorded() -> Vec<CloudEvent> {
    let text = std::fs::read_to_string(helpers::fixtures_dir().join("synth_origin.jsonl")).unwrap();
    let mut state = TranscriptState::new(SESSION.to_string());
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .flat_map(|v| translate_line(&v, &mut state))
        .collect()
}

fn ids(events: &[CloudEvent]) -> BTreeSet<String> {
    events.iter().map(|e| e.id.clone()).collect()
}

async fn session_ids(node: &Node) -> Vec<String> {
    node.get("/api/digests").await["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap().to_string())
        .collect()
}

async fn summary(node: &Node, session: &str, now: chrono::DateTime<chrono::Utc>) -> Value {
    let s = node.state.read().await;
    let p = s
        .store
        .projections
        .get(session)
        .unwrap_or_else(|| panic!("{session} projected on {}", node.base));
    serde_json::to_value(p.value().summary(session, Some(now))).unwrap()
}

fn beat_row(host: &str, time: &str, body: Value) -> PresenceRow {
    PresenceRow {
        host: host.to_string(),
        principal_id: "dev".into(),
        person_id: None,
        time: time.to_string(),
        body,
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn replayed_history_converges_to_one_fold_and_no_hand_writes_history() {
    let _serial = serial();
    boot::set_serving();
    let events = recorded();
    assert!(
        events.len() >= 60,
        "the sample translates to a working set: {}",
        events.len()
    );
    let all = ids(&events);
    assert_eq!(all.len(), events.len(), "recorded ids are unique");
    let n = events.len();
    let (a_slice, b_slice) = (&events[..n * 2 / 3], &events[n / 3..]);

    // Partitioned: A observes the first two thirds in order; B observes the
    // last two thirds with its batches arriving newest first.
    let a = Node::start().await;
    let b = Node::start().await;
    a.partitioned.store(true, Ordering::SeqCst);
    b.partitioned.store(true, Ordering::SeqCst);
    for chunk in a_slice.chunks(BATCH) {
        a.publish(SESSION, "proj-1", chunk.to_vec()).await;
    }
    for chunk in b_slice.chunks(BATCH).rev() {
        b.publish(SESSION, "proj-1", chunk.to_vec()).await;
    }
    a.settle(a_slice.len() as u64).await;
    b.settle(b_slice.len() as u64).await;
    assert_ne!(a.rollup().await["digest"], b.rollup().await["digest"]);

    // Heal, converge each against the other.
    a.partitioned.store(false, Ordering::SeqCst);
    b.partitioned.store(false, Ordering::SeqCst);
    let (status, first) = a
        .post(
            "converge",
            json!({"peers": [b.base], "idempotency_key": "p-a", "settle_ms": 50}),
        )
        .await;
    assert_eq!(status, 200, "{first}");
    let (status, second) = b
        .post(
            "converge",
            json!({"peers": [a.base], "idempotency_key": "p-b", "settle_ms": 50}),
        )
        .await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["result"]["converged"], true, "{second}");
    a.settle(n as u64).await;
    b.settle(n as u64).await;

    // Property 1: the roll-ups are equal, and hold exactly the recording.
    let (ra, rb) = (a.rollup().await, b.rollup().await);
    assert_eq!(ra, rb, "equal sets, equal roll-ups");
    assert_eq!(ra["events"], n);
    for node in [&a, &b] {
        let held: BTreeSet<String> = node
            .get(&format!("/api/sessions/{SESSION}/events"))
            .await
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(held, all, "no event outside the recording on {}", node.base);
    }

    // Property 2: fold determinism. Every session's projection is the same
    // on both nodes, although the events arrived in different orders.
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let sessions = session_ids(&a).await;
    assert_eq!(sessions, session_ids(&b).await);
    assert!(!sessions.is_empty());
    for sid in &sessions {
        let (sa, sb) = (summary(&a, sid, now).await, summary(&b, sid, now).await);
        assert_eq!(sa, sb, "projection of {sid} differs between nodes");
        assert_eq!(sa["event_count"], n, "{sa}");
    }

    // Property 3: the report reads all clear under a fake clock, and
    // reports the peer stale once that clock moves past three beats.
    let (ha, hb) = (a.get("/api/health").await, b.get("/api/health").await);
    let local = consistency::Snapshot::from_health(&ha);
    let beat = "2026-09-25T12:00:00.000Z";
    let mut peer_body = hb.clone();
    peer_body["host"] = json!("node-b");
    let row = beat_row("node-b", beat, peer_body);
    let fake_now = |secs: i64| now + chrono::Duration::seconds(secs);
    let fresh = presence::fleet_view(&[row.clone()], fake_now(5), 15);
    let peer = consistency::Snapshot::from_presence(&fresh[0]);
    let report = consistency::report(&local, &[peer], 15);
    assert_eq!(report["level"], "ok", "{report}");
    assert_eq!(report["findings"], json!([]));
    assert_eq!(report["peers"][0]["differing_projects"], 0);
    let later = presence::fleet_view(&[row], fake_now(60), 15);
    let peer = consistency::Snapshot::from_presence(&later[0]);
    let report = consistency::report(&local, &[peer], 15);
    assert_eq!(
        report["findings"][0]["id"], "stale_snapshot:node-b",
        "{report}"
    );
    assert_eq!(
        ha["watermarks"], hb["watermarks"],
        "both persisted the same newest event"
    );

    // Property 4: no hand wrote history. Every events.* publish on either
    // node carries only recorded ids, and each node's events.* publishes
    // are the spec's observation plus catch-up re-injection, nothing else;
    // the hands left their record under ops.command.* only.
    for node in [&a, &b] {
        let published = node.bus.under("events.");
        for (subject, batch) in &published {
            assert_eq!(subject, &format!("events.{SESSION}"), "{subject}");
            assert!(
                batch.events.iter().all(|e| all.contains(&e.id)),
                "an events.* publish with an id outside the recording"
            );
        }
        let injected: usize = published
            .iter()
            .filter(|(_, b)| b.project_id == "proj-1")
            .count();
        assert_eq!(
            injected,
            published.len(),
            "every batch is placed under the recording's project"
        );
        assert!(!node.bus.under("ops.command.converge").is_empty());
        assert!(node
            .bus
            .under("ops.")
            .iter()
            .all(|(s, _)| !s.starts_with("events.")));
    }
    let observed_by_a = a_slice.chunks(BATCH).len();
    let a_published = a.bus.under("events.").len();
    assert!(
        a_published > observed_by_a,
        "catch-up re-injected on A: {a_published} publishes for {observed_by_a} observed batches"
    );

    // And the static audit stays green: history is published only by the
    // watcher egress and catch-up, never by a hand.
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let audit = std::process::Command::new("python3")
        .arg(repo.join("scripts/subject_publishers.py"))
        .output()
        .expect("python3 runs the audit");
    assert!(
        audit.status.success(),
        "subject_publishers.py: {}{}",
        String::from_utf8_lossy(&audit.stdout),
        String::from_utf8_lossy(&audit.stderr)
    );
}
