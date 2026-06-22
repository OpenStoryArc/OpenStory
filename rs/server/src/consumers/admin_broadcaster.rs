//! admin_broadcaster — the actor that owns updates to `AppState.admin_topology_tx`.
//!
//! ## Architecture (SICP stream-mux)
//!
//! ```text
//!   pulse_rx: mpsc::Receiver<()>            ← "session set might have changed"
//!         │                                   (driven by bus.subscribe("changes.>")
//!         │                                    or, in tests, an explicit Sender)
//!         ▼
//!   ┌─────────────────┐
//!   │  actor loop:    │
//!   │  on any pulse:  │
//!   │   1. tally       │  ← I/O (read store.list_sessions)
//!   │   2. compute    │  ← pure compute_topology
//!   │   3. diff vs    │  ← Eq check
//!   │      latest      │
//!   │   4. send_replace│  ← if changed
//!   └────────┬─────────┘
//!            ▼
//!   watch::Sender<Topology>     ← REST handler `.borrow()`s,
//!                                  Step 7's UI sink `.changed().await`s
//! ```
//!
//! Inputs are heterogeneous streams unified by `tokio::select!`. v0.2
//! Step 5 ships the session-pulse input; Step 6 adds the JetStream
//! advisory input under the same loop.
//!
//! The diff-and-send pattern lifts the cache invalidation invariant out
//! of the consumers: only changed topology snapshots are broadcast, so
//! WS subscribers don't get spammed with no-op updates.

use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use open_story_bus::Bus;
use open_story_store::event_store::EventStore;

use crate::admin::{
    compute_topology_with_owners, fetch_live_sources, EnvInputs, Topology,
};
use crate::config::Role;

/// Spawn the broadcaster actor. Owns `watch_tx` (write side); reads
/// sessions via the `event_store` trait — *only* needs that capability,
/// not the wider `StoreState`. Optionally holds an `Arc<dyn Bus>` so it
/// can enrich each frame with live JetStream source state when one is
/// available. The actor runs until `pulse_rx` is closed.
pub fn spawn(
    watch_tx: watch::Sender<Topology>,
    event_store: Arc<dyn EventStore>,
    bus: Option<Arc<dyn Bus>>,
    env: EnvInputs,
    host: String,
    role: Role,
    mut pulse_rx: mpsc::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while pulse_rx.recv().await.is_some() {
            let (session_hosts, session_owners) =
                tally_session_hosts_and_owners(&*event_store).await;
            let mut next = compute_topology_with_owners(
                &host,
                role,
                &env,
                &session_hosts,
                &session_owners,
            );
            // Enrich with live JetStream sources when the bus exposes a
            // context. Solo / NoopBus: no change. Federation: surfaces
            // every leaf registered on events-agg with active/lag state.
            if let Some(b) = bus.as_ref() {
                if let Some(js) = b.jetstream() {
                    next.live_sources = fetch_live_sources(js).await;
                }
            }
            // Eq dedup — only broadcast frames that actually differ from
            // the current cached value.
            let changed = { *watch_tx.borrow() != next };
            if changed {
                let _previous = watch_tx.send_replace(next);
            }
        }
    })
}

/// Pulse forwarder: WS broadcast events → pulse channel. The payload is
/// irrelevant — only the "something happened" signal matters — so this
/// must survive both failure modes of a fast producer / slow consumer:
///
/// - `RecvError::Lagged`: the broadcast ring lapped us. The missed
///   messages each *would have been* a pulse; one coalesced pulse is an
///   equivalent signal. Keep receiving.
/// - Full pulse channel: a pulse is already queued, so the actor will
///   recompute anyway — drop this one (`try_send`) instead of blocking,
///   because blocking here is exactly what causes the lag above.
///
/// Exits only when either channel is closed.
pub fn spawn_pulse_forwarder(
    mut broadcast_rx: tokio::sync::broadcast::Receiver<crate::broadcast::BroadcastMessage>,
    pulse_tx: mpsc::Sender<()>,
) -> JoinHandle<()> {
    use tokio::sync::broadcast::error::RecvError;
    use tokio::sync::mpsc::error::TrySendError;
    tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(_) => match pulse_tx.try_send(()) {
                    Ok(()) | Err(TrySendError::Full(())) => {}
                    Err(TrySendError::Closed(())) => break,
                },
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    })
}

/// I/O at the boundary: read host tallies AND (host, person_id) ownership
/// pairs from the store. NULL host is skipped for both — pre-stamping rows
/// we can't attribute. NULL person_id is skipped for the ownership pairs
/// (it's a real "we don't know" signal — the existing fleet tally still
/// records the host).
async fn tally_session_hosts_and_owners(
    event_store: &dyn EventStore,
) -> (Vec<(String, u64)>, Vec<(String, String)>) {
    let rows = event_store.list_sessions().await.unwrap_or_default();
    let mut tally: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut owners_set: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for row in rows {
        let Some(h) = row.host else { continue };
        *tally.entry(h.clone()).or_insert(0) += 1;
        if let Some(p) = row.person_id {
            owners_set.insert((h, p));
        }
    }
    (
        tally.into_iter().collect(),
        owners_set.into_iter().collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::compute_topology;
    use open_story_store::event_store::SessionRow;
    use std::time::Duration;

    fn msg() -> crate::broadcast::BroadcastMessage {
        crate::broadcast::BroadcastMessage::ViewRecords {
            session_id: "s".into(),
            view_records: vec![],
            project_id: None,
            project_name: None,
        }
    }

    /// The forwarder must outlive a `Lagged` error. A fast producer laps
    /// the slow receiver (capacity 2, five sends before the first recv);
    /// the missed messages coalesce into the signal — the forwarder keeps
    /// forwarding afterwards instead of silently dying, which is exactly
    /// the failure that froze the admin topology frame in the field.
    #[tokio::test]
    async fn pulse_forwarder_survives_lagged_broadcast() {
        let (bcast_tx, bcast_rx) = tokio::sync::broadcast::channel(2);
        let (pulse_tx, mut pulse_rx) = mpsc::channel(8);
        // Overflow the ring BEFORE the forwarder ever polls: receiver
        // exists (subscribed above) but recv() hasn't run, so its first
        // poll observes Lagged.
        for _ in 0..5 {
            bcast_tx.send(msg()).unwrap();
        }
        let handle = spawn_pulse_forwarder(bcast_rx, pulse_tx);

        // It recovers: messages still in the ring forward as pulses.
        tokio::time::timeout(Duration::from_secs(1), pulse_rx.recv())
            .await
            .expect("forwarder should emit a pulse after Lagged")
            .expect("pulse channel open");

        // And it's still alive for fresh messages.
        bcast_tx.send(msg()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), pulse_rx.recv())
            .await
            .expect("forwarder should keep forwarding after recovery")
            .expect("pulse channel open");

        drop(bcast_tx); // closing the broadcast side ends the task
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("forwarder exits on close")
            .unwrap();
    }

    /// SICP/actor proof: pulse → compute → diff → send_replace.
    /// We seed the store with one session whose host appears nowhere in
    /// the initial topology, pulse the broadcaster, then watch the
    /// `changed()` future fire with the new host visible.
    #[tokio::test]
    async fn broadcaster_recomputes_on_pulse_and_pushes_new_frame() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store_state = open_story_store::state::StoreState::new(tmp.path()).expect("store");
        let event_store = store_state.event_store.clone();

        // Initial frame: empty fleet (no sessions yet).
        let initial = compute_topology("a1", Role::Full, &EnvInputs::default(), &[]);
        let (watch_tx, mut watch_rx) = watch::channel(initial);

        let (pulse_tx, pulse_rx) = mpsc::channel(8);
        let handle = spawn(
            watch_tx,
            event_store.clone(),
            None, // bus not under test here
            EnvInputs::default(),
            "a1".to_string(),
            Role::Full,
            pulse_rx,
        );

        // Seed a session originating from a peer host — should show up in
        // the next computed Topology once we pulse.
        event_store
            .upsert_session(&SessionRow {
                id: "sess-1".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 1,
                first_event: None,
                last_event: None,
                host: Some("peer-host".into()),
                user: None,
                origin_agent: None,
                person_id: None,
                principal_id: None,
            })
            .await
            .expect("upsert");

        // Pulse — actor wakes, recomputes, sends.
        pulse_tx.send(()).await.expect("pulse delivered");

        // Wait for the watch to fire (`changed()` resolves on next push).
        // Timeout guards against a stuck actor; cheap operation in-process.
        let updated = tokio::time::timeout(Duration::from_secs(2), watch_rx.changed()).await;
        updated.expect("watch fired in time").expect("watch open");

        let frame = watch_rx.borrow().clone();
        let hosts: Vec<&str> = frame.nodes.iter().map(|n| n.host.as_str()).collect();
        assert!(
            hosts.contains(&"peer-host"),
            "peer-host should appear after pulse — got {hosts:?}"
        );

        drop(pulse_tx); // close the input; actor exits its loop
        handle.await.expect("actor joins cleanly");
    }

    /// Dedup: an identical-frame pulse should NOT fire the watch. The
    /// broadcaster diffs first; no-op pulses don't spam downstream sinks.
    #[tokio::test]
    async fn broadcaster_skips_send_when_topology_unchanged() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store_state = open_story_store::state::StoreState::new(tmp.path()).expect("store");
        let event_store = store_state.event_store.clone();
        let initial = compute_topology("a1", Role::Full, &EnvInputs::default(), &[]);
        let (watch_tx, mut watch_rx) = watch::channel(initial);
        let (pulse_tx, pulse_rx) = mpsc::channel(8);
        let _handle = spawn(
            watch_tx,
            event_store,
            None,
            EnvInputs::default(),
            "a1".to_string(),
            Role::Full,
            pulse_rx,
        );

        // Pulse with no store changes — same Topology, no send.
        pulse_tx.send(()).await.unwrap();

        // Brief settle — give the actor a chance to no-op. If it DID send,
        // changed() would fire within this window.
        let result =
            tokio::time::timeout(Duration::from_millis(200), watch_rx.changed()).await;
        assert!(result.is_err(), "watch must NOT fire on no-op pulse");
    }
}
