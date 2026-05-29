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

use open_story_store::event_store::EventStore;

use crate::admin::{EnvInputs, Topology, compute_topology};
use crate::config::Role;

/// Spawn the broadcaster actor. Owns `watch_tx` (write side); reads
/// sessions via the `event_store` trait — *only* needs that capability,
/// not the wider `StoreState`. The actor runs until `pulse_rx` is closed
/// (typically: server shutdown).
pub fn spawn(
    watch_tx: watch::Sender<Topology>,
    event_store: Arc<dyn EventStore>,
    env: EnvInputs,
    host: String,
    role: Role,
    mut pulse_rx: mpsc::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while pulse_rx.recv().await.is_some() {
            let session_hosts = tally_session_hosts(&*event_store).await;
            let next = compute_topology(&host, role, &env, &session_hosts);
            // Eq dedup — only broadcast frames that actually differ from
            // the current cached value. Cheap (Topology is small + Eq).
            let changed = { *watch_tx.borrow() != next };
            if changed {
                let _previous = watch_tx.send_replace(next);
            }
        }
    })
}

/// I/O at the boundary: read distinct host counts from the store. NULL
/// host is skipped (pre-stamping rows we can't attribute).
async fn tally_session_hosts(event_store: &dyn EventStore) -> Vec<(String, u64)> {
    let rows = event_store.list_sessions().await.unwrap_or_default();
    let mut tally: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    for row in rows {
        if let Some(h) = row.host {
            *tally.entry(h).or_insert(0) += 1;
        }
    }
    tally.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_store::event_store::SessionRow;
    use std::time::Duration;

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
