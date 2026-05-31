//! Share-policy lookup for bus-level Invariant ① enforcement.
//!
//! Phase 4.10. The bus consults this trait before publishing each
//! `IngestBatch`. If the batch's session is marked `private`, the
//! publish is skipped — the event never reaches the NATS subject, never
//! reaches peer subscribers, never lands on the hub aggregate.
//!
//! This completes the Layer 4 contract end-to-end: the API enforces the
//! UI's "private" toggle on every read (Phase 4.2), and the bus
//! enforces it on every write. A private session is truly local — even
//! the federation transport can't see it.
//!
//! Wiring lives in the server: `rs/src/server/mod.rs` constructs a
//! `EventStoreSharePolicy` adapter around the `Arc<dyn EventStore>` and
//! injects it into the bus via `NatsBus::with_policy_lookup`. The bus
//! crate itself doesn't depend on the store crate — it accepts any
//! impl of this trait.

use async_trait::async_trait;
use std::sync::Arc;

/// Bus-side policy lookup. Pure read-only — only `is_private` is needed
/// on the publish hot path.
#[async_trait]
pub trait SharePolicyLookup: Send + Sync + 'static {
    /// Returns `true` iff the session is marked `Private` (and should
    /// therefore be skipped at publish time). Implementations should
    /// fail-CLOSED on store errors — return `true` (skip) rather than
    /// `false` (publish), matching the sovereignty path's fail-closed
    /// default established in Phase 4.4.
    async fn is_private(&self, session_id: &str) -> bool;
}

/// Default impl — every session is treated as shared. Used by NatsBus
/// out of the box; the server upgrades this to the real store-backed
/// lookup at boot. Means `cargo test` against an in-memory NatsBus
/// doesn't have to wire up a policy store.
pub struct NoopSharePolicyLookup;

#[async_trait]
impl SharePolicyLookup for NoopSharePolicyLookup {
    async fn is_private(&self, _session_id: &str) -> bool {
        false
    }
}

impl NoopSharePolicyLookup {
    pub fn arc() -> Arc<dyn SharePolicyLookup> {
        Arc::new(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;

    /// Test-only stub: an explicit set of "private" session ids; everything
    /// else is shared. Used by Phase 4.10 unit tests to verify the
    /// bus-level skip path without a real store.
    pub(crate) struct StubLookup {
        pub privates: HashSet<String>,
        pub calls: Mutex<Vec<String>>,
    }

    impl StubLookup {
        fn new(privates: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                privates: privates.iter().map(|s| s.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl SharePolicyLookup for StubLookup {
        async fn is_private(&self, session_id: &str) -> bool {
            self.calls.lock().unwrap().push(session_id.to_string());
            self.privates.contains(session_id)
        }
    }

    #[tokio::test]
    async fn noop_lookup_is_never_private() {
        let l = NoopSharePolicyLookup;
        assert!(!l.is_private("anything").await);
    }

    #[tokio::test]
    async fn stub_lookup_routes_by_session_id() {
        let l = StubLookup::new(&["sess-private"]);
        assert!(l.is_private("sess-private").await);
        assert!(!l.is_private("sess-public").await);
        let calls = l.calls.lock().unwrap();
        assert_eq!(calls.as_slice(), &["sess-private", "sess-public"]);
    }
}
