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

fn parse_remote_digests(body: &Value) -> Vec<SessionDigest> {
    body.get("sessions")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some(SessionDigest {
                        session_id: d["session_id"].as_str()?.to_string(),
                        count: d["count"].as_u64().unwrap_or(0) as usize,
                        digest: d["digest"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One reconciliation pass against `peer`. Returns the number of sessions healed
/// (missing-here or diverged), each re-injected into the local bus.
pub async fn catch_up_once(
    event_store: &Arc<dyn EventStore>,
    bus: &Arc<dyn Bus>,
    peer: &str,
    client: &reqwest::Client,
) -> usize {
    let local = local_digests(event_store).await;
    let remote = match client.get(format!("{peer}/api/digests")).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(body) => parse_remote_digests(&body),
            Err(_) => return 0,
        },
        Err(_) => return 0,
    };

    let diff = diff_digests(&local, &remote);
    let mut healed = 0;
    for sid in diff.missing_here.iter().chain(diff.diverged.iter()) {
        let raw: Vec<Value> = match client
            .get(format!("{peer}/api/sessions/{sid}/events"))
            .send()
            .await
        {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(_) => continue,
        };
        let events: Vec<CloudEvent> = raw
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        if events.is_empty() {
            continue;
        }
        // Re-inject into the local bus; the persist consumer dedups by PK, so
        // pulling an event we already have (e.g. for a diverged session) is safe.
        let batch = IngestBatch {
            session_id: sid.clone(),
            project_id: String::new(),
            events,
        };
        if bus.publish(&format!("events.{sid}"), &batch).await.is_ok() {
            healed += 1;
        }
    }
    healed
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
