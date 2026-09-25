//! catch-up — application-level anti-entropy: periodically reconcile this node
//! against a **peer** by digest-diffing and pulling whatever it's missing.
//!
//! Why: the cross-leaf convergence race (federation-bidirectional-mirror-gap).
//! On a cold fleet boot, NATS core interest-propagation can permanently drop a
//! peer's events, and no JetStream source backfills them. Rather than fight
//! NATS replication timing, this realizes the `catch-up` operation from the
//! state-management design: compare `/api/digests` with a peer, fetch the events
//! for any session we lack or that diverges, and re-inject them into the LOCAL
//! bus (PK-deduped on ingest). Transport- and topology-agnostic, robust to any
//! boot timing.
//!
//! Crucially **peer-generic, not hub-specific**: the peer is just a URL. A hub
//! is the convenient default (always-on, has everything), but nothing here
//! requires one — the same code supports a hub-less peer mesh. Enabled via the
//! `OPEN_STORY_CATCH_UP_PEER` env var (prototype flag; should graduate to a
//! config field + multi-peer). See docs/research/state-management-interface.md.

use std::sync::Arc;
use std::time::Duration;

use open_story_bus::{Bus, IngestBatch};
use open_story_core::cloud_event::CloudEvent;
use open_story_store::event_store::EventStore;
use serde_json::Value;

use crate::fleet::{diff_digests, digest_event_ids, PlacedDigest, SessionDigest};

const CATCH_UP_INTERVAL: Duration = Duration::from_secs(10);

/// This node's per-session digests, each placed under the host that
/// produced the session and its project (from the sessions table). The one
/// read behind `/api/digests`, the roll-up on the beat, and catch-up. Ids
/// only cross the store boundary; no event body is deserialized.
pub async fn placed_digests(event_store: &Arc<dyn EventStore>) -> Vec<PlacedDigest> {
    let mut out = Vec::new();
    for row in event_store.list_sessions().await.unwrap_or_default() {
        let ids = event_store
            .session_event_ids(&row.id)
            .await
            .unwrap_or_default();
        out.push(PlacedDigest {
            host: row.host.unwrap_or_default(),
            project: row.project_id.unwrap_or_default(),
            count: ids.len(),
            digest: digest_event_ids(&ids),
            session_id: row.id,
        });
    }
    out
}

/// The per-session view of `placed_digests` (what `diff_digests` takes).
pub fn session_digests(placed: &[PlacedDigest]) -> Vec<SessionDigest> {
    placed
        .iter()
        .map(|d| SessionDigest {
            session_id: d.session_id.clone(),
            count: d.count,
            digest: d.digest.clone(),
        })
        .collect()
}

/// This node's per-session digests (same shape `/api/digests` serves).
async fn local_digests(event_store: &Arc<dyn EventStore>) -> Vec<SessionDigest> {
    session_digests(&placed_digests(event_store).await)
}

/// A peer's digests with their placement (host, project) when the peer
/// serves it; an older peer's rows place as `unknown`.
fn parse_remote_digests(body: &Value) -> Vec<PlacedDigest> {
    body.get("sessions")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some(PlacedDigest {
                        host: d["host"].as_str().unwrap_or("").to_string(),
                        project: d["project"].as_str().unwrap_or("").to_string(),
                        session_id: d["session_id"].as_str()?.to_string(),
                        count: d["count"].as_u64().unwrap_or(0) as usize,
                        digest: d["digest"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What one pass against a peer found and did (C-05).
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CatchUpReport {
    /// The peer answered `/api/digests`.
    pub reachable: bool,
    /// Sessions the peer has that we lacked, or that diverged, and for
    /// which events we did not hold were re-injected here.
    pub healed: usize,
    /// Events re-injected: only ids this node did not already hold.
    pub pulled_events: usize,
    pub missing_here: usize,
    pub missing_there: usize,
    pub diverged: usize,
    /// The sessions healed, so a caller can verify what moved.
    pub healed_sessions: Vec<String>,
}

/// One reconciliation pass against `peer`, reported. For a diverged
/// session only the ids we lack are re-injected: the union, never a
/// replay of what we hold, so a pass that finds nothing new heals nothing
/// and a converge loop can see that a round changed nothing.
pub async fn catch_up_report(
    event_store: &Arc<dyn EventStore>,
    bus: &Arc<dyn Bus>,
    peer: &str,
    client: &reqwest::Client,
) -> CatchUpReport {
    let peer = peer.trim_end_matches('/');
    let local = local_digests(event_store).await;
    let remote = match client.get(format!("{peer}/api/digests")).send().await {
        Ok(r) if r.status().is_success() => match r.json::<Value>().await {
            Ok(body) => parse_remote_digests(&body),
            Err(_) => return CatchUpReport::default(),
        },
        _ => return CatchUpReport::default(),
    };

    // The origin's placement rides on the envelope, so the pulled session
    // lands under the same project here as there: a union carries the
    // derived placement with the set, or two equal sets roll up unequal.
    let placement: std::collections::HashMap<&str, &str> = remote
        .iter()
        .map(|d| (d.session_id.as_str(), d.project.as_str()))
        .collect();
    let diff = diff_digests(&local, &session_digests(&remote));
    let mut report = CatchUpReport {
        reachable: true,
        missing_here: diff.missing_here.len(),
        missing_there: diff.missing_there.len(),
        diverged: diff.diverged.len(),
        ..CatchUpReport::default()
    };
    for sid in diff.missing_here.iter().chain(diff.diverged.iter()) {
        let raw: Vec<Value> = match client
            .get(format!("{peer}/api/sessions/{sid}/events"))
            .send()
            .await
        {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(_) => continue,
        };
        let have: std::collections::HashSet<String> = event_store
            .session_event_ids(sid)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
        let events: Vec<CloudEvent> = raw
            .iter()
            .filter_map(|v| serde_json::from_value::<CloudEvent>(v.clone()).ok())
            .filter(|ce| !have.contains(&ce.id))
            .collect();
        if events.is_empty() {
            continue;
        }
        // Re-inject into the local bus: the same path the watcher egress
        // uses, and the persist consumer dedups by PK behind it.
        let n = events.len();
        let batch = IngestBatch {
            session_id: sid.clone(),
            project_id: placement
                .get(sid.as_str())
                .copied()
                .unwrap_or("")
                .to_string(),
            events,
        };
        if bus.publish(&format!("events.{sid}"), &batch).await.is_ok() {
            report.healed += 1;
            report.pulled_events += n;
            report.healed_sessions.push(sid.clone());
        }
    }
    report
}

/// One reconciliation pass against `peer`. Returns the number of sessions healed
/// (missing-here or diverged), each re-injected into the local bus.
pub async fn catch_up_once(
    event_store: &Arc<dyn EventStore>,
    bus: &Arc<dyn Bus>,
    peer: &str,
    client: &reqwest::Client,
) -> usize {
    catch_up_report(event_store, bus, peer, client).await.healed
}

/// Spawn a background loop reconciling this node against `peer` forever.
pub fn spawn_catch_up(event_store: Arc<dyn EventStore>, bus: Arc<dyn Bus>, peer: String) {
    eprintln!(
        "  \x1b[36mcatch-up: reconciling against peer {peer} every {}s\x1b[0m",
        CATCH_UP_INTERVAL.as_secs()
    );
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            tokio::time::sleep(CATCH_UP_INTERVAL).await;
            let healed = catch_up_once(&event_store, &bus, &peer, &client).await;
            if healed > 0 {
                eprintln!("  \x1b[36mcatch-up: healed {healed} session(s) from {peer}\x1b[0m");
            }
        }
    });
}
