//! Admin — operator-facing topology + (eventually) share/store policy.
//!
//! v0 surface: `GET /api/admin/topology` returns this node's view of the
//! federation it boots into — derived purely from its own configuration
//! (role + the `OPEN_STORY_HUB_DOMAIN` / `OPEN_STORY_PEER_HUB_DOMAINS` /
//! `OPEN_STORY_PEER_DOMAINS` env vars).
//!
//! This is the sovereignty edge of the Admin view: *this device's* picture
//! of the topology, not a global one. Other devices' liveness will be
//! layered in via JetStream stream-info introspection in a follow-up.
//!
//! Design follows the federation-topology-viz doc shapes:
//!
//! - **Solo** — no federation flags set; the single-node case.
//! - **T1** mesh — `OPEN_STORY_PEER_DOMAINS=...` set; no hub.
//! - **T2** star — `OPEN_STORY_HUB_DOMAIN=hub` set, no peer hubs.
//! - **T3** multi-hub — hub-role node with `OPEN_STORY_PEER_HUB_DOMAINS=...`,
//!   or a leaf attached to a hub that has peer hubs (the leaf sees T2 from
//!   its own perspective; cross-hub fanout is the hubs' concern).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use open_story_store::event_store::{SharePolicyMode, SharePolicyRow};

use crate::config::Role;
use crate::logging::log_event;
use crate::state::SharedState;

/// One of the four topology shapes the federation can be in, from this
/// node's vantage. Encoded kebab-case for the JSON wire (`solo`, `t1`,
/// `t2`, `t3`) so the UI can branch with a single string compare.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TopologyShape {
    Solo,
    T1,
    T2,
    T3,
}

/// What this node is doing in the topology.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    /// Standalone — no federation.
    Solo,
    /// Hub aggregate (T2 single-hub, T3 multi-hub) — runs `events-agg`.
    Hub,
    /// Leaf — publishes its own events, mirrors the fleet.
    Leaf,
}

/// This node's identity + configuration view.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NodeInfo {
    pub host: String,
    pub role: NodeRole,
    /// JetStream domain pinned for this node's NATS when federation is on
    /// (== `host` for leaves, == `hub_domain` for hubs; `None` for solo).
    pub domain: Option<String>,
    /// The hub this leaf is attached to, or this hub's own domain.
    pub hub_domain: Option<String>,
    /// In T3, the peer hubs this hub sources from.
    pub peer_hub_domains: Vec<String>,
    /// In T1, the peer devices this node sources from.
    pub peer_domains: Vec<String>,
}

/// A single host known to be (or to have been) part of the fleet, from
/// this device's vantage. Used to render the "whole topology" view —
/// not just self + configured peers, but every node we have evidence of.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NodeSummary {
    pub host: String,
    pub is_self: bool,
    /// How many sessions originating on this host have landed in local
    /// storage. Zero is meaningful: it means the host is *configured* as
    /// a peer (env var) but we haven't seen events from it yet.
    pub session_count: u64,
    /// Why we know about this host. `"self"` = this device. `"sessions"`
    /// = at least one SessionRow has this host. `"peer-config"` = this
    /// host appears in `OPEN_STORY_PEER_DOMAINS` (T1 mesh). `"hub-config"`
    /// = this host is the configured `OPEN_STORY_HUB_DOMAIN` (T2/T3).
    pub source: NodeEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NodeEvidence {
    SelfNode,
    Sessions,
    PeerConfig,
    HubConfig,
}

/// What `/api/admin/topology` returns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Topology {
    pub shape: TopologyShape,
    #[serde(rename = "self")]
    pub self_: NodeInfo,
    /// Every host this device has evidence of — self plus any host
    /// appearing in `SessionRow.host` plus any configured peer. Sorted
    /// self-first then alphabetically. Used to render the fleet view.
    pub nodes: Vec<NodeSummary>,
}

/// Federation env-var snapshot — the subset of process env that
/// `derive_topology` and `compute_topology` consume. Snapshotting at the
/// boundary keeps the derivation pure and testable; an actor refreshes
/// the snapshot only when it has reason to (boot, SIGHUP, etc.).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvInputs {
    pub hub_domain: Option<String>,
    pub peer_hub_domains: Vec<String>,
    pub peer_domains: Vec<String>,
}

impl EnvInputs {
    /// Read the federation env vars at the I/O boundary. Used once per
    /// boot (and on policy reconciliation); not called in the request hot
    /// path — the cached Topology already reflects this snapshot.
    pub fn from_env() -> Self {
        let split = |v: String| -> Vec<String> {
            v.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };
        Self {
            hub_domain: std::env::var("OPEN_STORY_HUB_DOMAIN")
                .ok()
                .filter(|v| !v.is_empty()),
            peer_hub_domains: std::env::var("OPEN_STORY_PEER_HUB_DOMAINS")
                .ok()
                .filter(|v| !v.is_empty())
                .map(split)
                .unwrap_or_default(),
            peer_domains: std::env::var("OPEN_STORY_PEER_DOMAINS")
                .ok()
                .filter(|v| !v.is_empty())
                .map(split)
                .unwrap_or_default(),
        }
    }
}

/// Pure top-level helper — same inputs the handler used to gather inline,
/// now an explicit boundary. Wraps [`derive_topology`] so the broadcaster
/// can call one function with one snapshot at a time.
pub fn compute_topology(
    host: &str,
    role: Role,
    env: &EnvInputs,
    session_hosts: &[(String, u64)],
) -> Topology {
    derive_topology(
        host,
        role,
        env.hub_domain.as_deref(),
        &env.peer_hub_domains,
        &env.peer_domains,
        session_hosts,
    )
}

/// Pure derivation: shape, node info, and fleet roster from inputs. No I/O,
/// no env reads — those happen at the handler boundary. Testable in
/// isolation. `session_hosts` is a (host, count) tally from the local
/// session store; the function merges that with the configured peers so
/// the returned `nodes` includes both seen-in-data and known-by-config
/// fleet members.
pub fn derive_topology(
    host: &str,
    role: Role,
    hub_domain: Option<&str>,
    peer_hub_domains: &[String],
    peer_domains: &[String],
    session_hosts: &[(String, u64)],
) -> Topology {
    // Discriminate the federation mode.
    // - hub_domain set + Consumer role  → this node IS a hub
    // - hub_domain set + other role     → this node is a LEAF attached to a hub
    // - peer_domains set (no hub)       → T1 mesh leaf
    // - neither                         → solo
    let (shape, node_role, domain) = match (hub_domain, !peer_domains.is_empty()) {
        (Some(dom), _) if matches!(role, Role::Consumer) => {
            // Hub. T3 iff peer_hub_domains is non-empty.
            let shape = if peer_hub_domains.is_empty() {
                TopologyShape::T2
            } else {
                TopologyShape::T3
            };
            (shape, NodeRole::Hub, Some(dom.to_string()))
        }
        (Some(_), _) => {
            // Leaf attached to a hub. From the leaf's vantage it's T2 even
            // if the hub itself is part of a T3 mesh — the leaf sees one
            // hub. Cross-hub fanout shows up in the hub's own /api/admin
            // response, not the leaf's.
            (TopologyShape::T2, NodeRole::Leaf, Some(host.to_string()))
        }
        (None, true) => {
            // T1 solo multi-device mesh — peer-only, no hub.
            (TopologyShape::T1, NodeRole::Leaf, Some(host.to_string()))
        }
        (None, false) => (TopologyShape::Solo, NodeRole::Solo, None),
    };

    // Build the fleet roster: union of (self, session-host counts,
    // configured peers). Configured peers with 0 sessions still appear
    // — that's the "expected fleet" the operator told us about.
    let mut nodes: std::collections::BTreeMap<String, NodeSummary> =
        std::collections::BTreeMap::new();

    // Self always appears, even with zero sessions.
    nodes.insert(
        host.to_string(),
        NodeSummary {
            host: host.to_string(),
            is_self: true,
            session_count: 0,
            source: NodeEvidence::SelfNode,
        },
    );

    // Sessions: every distinct origin host with its count.
    for (h, count) in session_hosts {
        let entry = nodes.entry(h.clone()).or_insert_with(|| NodeSummary {
            host: h.clone(),
            is_self: h == host,
            session_count: 0,
            source: NodeEvidence::Sessions,
        });
        entry.session_count = entry.session_count.saturating_add(*count);
        // Promote source: self > sessions > peer-config > hub-config.
        if !entry.is_self {
            entry.source = NodeEvidence::Sessions;
        }
    }

    // Configured T1 peers (presence-only; session_count stays 0 if unseen).
    for p in peer_domains {
        nodes.entry(p.clone()).or_insert_with(|| NodeSummary {
            host: p.clone(),
            is_self: p == host,
            session_count: 0,
            source: NodeEvidence::PeerConfig,
        });
    }

    // Configured hub (T2/T3) — record presence; not the same kind of
    // peer (it's an aggregator), but the operator should see it.
    if let Some(h) = hub_domain {
        nodes.entry(h.to_string()).or_insert_with(|| NodeSummary {
            host: h.to_string(),
            is_self: h == host,
            session_count: 0,
            source: NodeEvidence::HubConfig,
        });
    }

    // Peer hubs (T3) — same treatment.
    for h in peer_hub_domains {
        nodes.entry(h.clone()).or_insert_with(|| NodeSummary {
            host: h.clone(),
            is_self: h == host,
            session_count: 0,
            source: NodeEvidence::HubConfig,
        });
    }

    // Self first, then alphabetical for stability across reloads.
    let mut nodes: Vec<NodeSummary> = nodes.into_values().collect();
    nodes.sort_by(|a, b| match (a.is_self, b.is_self) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.host.cmp(&b.host),
    });

    Topology {
        shape,
        self_: NodeInfo {
            host: host.to_string(),
            role: node_role,
            domain,
            hub_domain: hub_domain.map(String::from),
            peer_hub_domains: peer_hub_domains.to_vec(),
            peer_domains: peer_domains.to_vec(),
        },
        nodes,
    }
}

/// `GET /api/admin/topology` — serve the cached topology snapshot.
///
/// Reads the current frame from the `admin_topology_tx` watch channel —
/// the broadcaster owns updates; this handler just serves the latest
/// snapshot. No JetStream queries, no env reads, no session tallies on
/// the request hot path. SICP stream-with-memory pattern: `borrow()` is
/// the "force this stream's current value" operation.
pub async fn get_topology(State(state): State<SharedState>) -> Json<Value> {
    log_event("api", "GET /api/admin/topology");
    let s = state.read().await;
    let topology = s.admin_topology_tx.borrow().clone();
    drop(s);
    Json(serde_json::to_value(&topology).expect("Topology serializes"))
}

// ── Share policy endpoints ───────────────────────────────────────────────
//
// v0 contract:
//   GET  /api/admin/share-policy             → list policies + default
//   PUT  /api/admin/share-policy/{id}        body { "mode": "shared"|"private" }
//
// Sessions without a row are SHARED by default (sovereignty-by-default at
// the operator's choice — turning something private is an explicit act).
//
// What this layer DOES NOT do (deferred to a Phase 4 follow-up):
//
//   - Mutate `events-agg`'s source `filter_subject` on PUT (bus-level
//     enforcement). The DB write is the *authority*; once the subject
//     scheme distinguishes shared vs private (a Phase 4 prerequisite),
//     the bus can reconcile from the policy table on every
//     `ensure_streams` cycle.
//
// ── Named hard edge — invariant ③ (revocation = stop-flow, not purge) ─
//
// Marking a session private STOPS new visibility immediately (the API
// filter from invariant ① gates `/api/digests` and `/api/sessions/{id}/
// events`). It does NOT delete events on this device — `cleanup_old_
// sessions` with `keep_host=Some(self)` preserves them (invariant ②) and
// flipping back to `shared` restores access unchanged. Cf.
// `test_revocation_is_stop_flow_not_purge_invariant_three`.
//
// What it CANNOT do today: purge or invalidate copies already mirrored
// to peer devices. That's the personhood Q1/Q9 problem (research doc
// jetstream-sources-federation.md §"Three consistency invariants"). It
// is *named*, not assumed-free — the operator should know that un-share
// stops *new flow* and that distributed propagated copies persist on
// peers until those operators consent to a purge. Operational solutions
// (cooperative purge protocol, retention coordination, encrypted-at-rest
// with key revocation) are explicit Phase 5+ work.

#[derive(Debug, Deserialize)]
pub struct SetSharePolicyBody {
    pub mode: SharePolicyMode,
}

/// `GET /api/admin/share-policy` — list every session with an explicitly-set
/// share policy. The response also includes `default_mode` so the UI can
/// render unlisted sessions correctly without re-deriving the convention.
pub async fn list_share_policy(State(state): State<SharedState>) -> Json<Value> {
    log_event("api", "GET /api/admin/share-policy");
    let s = state.read().await;
    let policies: Vec<SharePolicyRow> = s
        .store
        .event_store
        .list_share_policies()
        .await
        .unwrap_or_default();
    drop(s);
    Json(json!({
        "default_mode": SharePolicyMode::Shared.as_str(),
        "policies": policies,
    }))
}

/// `PUT /api/admin/share-policy/{session_id}` — set this session's mode.
pub async fn set_share_policy(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    Json(body): Json<SetSharePolicyBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    log_event(
        "api",
        &format!(
            "PUT /api/admin/share-policy/{session_id} → {}",
            body.mode.as_str()
        ),
    );
    let s = state.read().await;
    let res = s
        .store
        .event_store
        .set_share_policy(&session_id, body.mode, None)
        .await;
    drop(s);
    match res {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    //! Pure derivation tests — no env, no I/O. The handler delegates
    //! straight to `derive_topology`, so this is the contract.
    use super::*;

    #[test]
    fn solo_with_no_flags() {
        let t = derive_topology("a1", Role::Full, None, &[], &[], &[]);
        assert_eq!(t.shape, TopologyShape::Solo);
        assert_eq!(t.self_.role, NodeRole::Solo);
        assert_eq!(t.self_.domain, None);
        assert!(t.self_.hub_domain.is_none());
    }

    #[test]
    fn hub_with_no_peer_hubs_is_t2() {
        let t = derive_topology("hub", Role::Consumer, Some("hub"), &[], &[], &[]);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Hub);
        assert_eq!(t.self_.domain.as_deref(), Some("hub"));
        assert_eq!(t.self_.hub_domain.as_deref(), Some("hub"));
    }

    #[test]
    fn hub_with_peer_hubs_is_t3() {
        let peers = vec!["hub-b".to_string()];
        let t = derive_topology("hub-a", Role::Consumer, Some("hub-a"), &peers, &[], &[]);
        assert_eq!(t.shape, TopologyShape::T3);
        assert_eq!(t.self_.role, NodeRole::Hub);
        assert_eq!(t.self_.peer_hub_domains, peers);
    }

    #[test]
    fn leaf_attached_to_hub_is_t2_from_its_vantage() {
        // Even if the hub itself is part of a T3 mesh, the LEAF sees
        // exactly one hub. Cross-hub fanout is the hub's concern.
        let t = derive_topology("node-0", Role::Full, Some("hub-a"), &[], &[], &[]);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Leaf);
        assert_eq!(t.self_.domain.as_deref(), Some("node-0"));
        assert_eq!(t.self_.hub_domain.as_deref(), Some("hub-a"));
        assert!(t.self_.peer_hub_domains.is_empty());
    }

    #[test]
    fn peers_only_is_t1_mesh() {
        let peers = vec!["laptop".to_string(), "phone".to_string()];
        let t = derive_topology("a1", Role::Full, None, &[], &peers, &[]);
        assert_eq!(t.shape, TopologyShape::T1);
        assert_eq!(t.self_.role, NodeRole::Leaf);
        assert_eq!(t.self_.peer_domains, peers);
        assert!(t.self_.hub_domain.is_none());
    }

    #[test]
    fn hub_takes_precedence_over_peer_domains() {
        // If both are set somehow, hub-mode wins — matches the CLI's
        // dispatch order (hub_domain checked before peer_domains).
        let peers = vec!["foo".to_string()];
        let t = derive_topology("node-0", Role::Full, Some("hub"), &[], &peers, &[]);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Leaf);
    }

    #[test]
    fn topology_serializes_to_expected_shape() {
        let t = derive_topology("node-0", Role::Full, Some("hub"), &[], &[], &[]);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["shape"], "t2");
        assert_eq!(v["self"]["host"], "node-0");
        assert_eq!(v["self"]["role"], "leaf");
        assert_eq!(v["self"]["hub_domain"], "hub");
    }

    // ── Fleet roster — the `nodes` array ──────────────────────────────────

    #[test]
    fn solo_with_no_evidence_lists_just_self() {
        let t = derive_topology("a1", Role::Full, None, &[], &[], &[]);
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].host, "a1");
        assert!(t.nodes[0].is_self);
        assert_eq!(t.nodes[0].source, NodeEvidence::SelfNode);
    }

    #[test]
    fn fleet_includes_hosts_seen_in_sessions() {
        // Even in solo mode, if SessionRows expose other hosts (e.g. via
        // prior NATS sharing), they show up in the fleet view.
        let sessions = vec![
            ("a1".to_string(), 32u64),
            ("Maxs-Air".to_string(), 4),
        ];
        let t = derive_topology("a1", Role::Full, None, &[], &[], &sessions);
        let hosts: Vec<&str> = t.nodes.iter().map(|n| n.host.as_str()).collect();
        assert_eq!(hosts, vec!["a1", "Maxs-Air"], "self first, then alpha");
        let self_node = t.nodes.iter().find(|n| n.is_self).unwrap();
        assert_eq!(self_node.host, "a1");
        assert_eq!(self_node.session_count, 32);
        let peer = t.nodes.iter().find(|n| n.host == "Maxs-Air").unwrap();
        assert!(!peer.is_self);
        assert_eq!(peer.session_count, 4);
        assert_eq!(peer.source, NodeEvidence::Sessions);
    }

    #[test]
    fn fleet_includes_configured_peers_with_zero_session_count() {
        // T1 mesh: peers from env config that we haven't seen events
        // from yet still appear — the operator told us they exist.
        let peers = vec!["laptop".to_string(), "phone".to_string()];
        let t = derive_topology("a1", Role::Full, None, &[], &peers, &[]);
        let by_host: std::collections::HashMap<_, _> =
            t.nodes.iter().map(|n| (n.host.as_str(), n)).collect();
        assert_eq!(by_host["laptop"].session_count, 0);
        assert_eq!(by_host["laptop"].source, NodeEvidence::PeerConfig);
        assert_eq!(by_host["phone"].source, NodeEvidence::PeerConfig);
    }

    #[test]
    fn fleet_merges_session_and_config_sources() {
        // A host appears in BOTH peer_domains and sessions — session
        // evidence wins (we've actually seen events).
        let peers = vec!["laptop".to_string()];
        let sessions = vec![("laptop".to_string(), 7)];
        let t = derive_topology("a1", Role::Full, None, &[], &peers, &sessions);
        let laptop = t.nodes.iter().find(|n| n.host == "laptop").unwrap();
        assert_eq!(laptop.session_count, 7);
        assert_eq!(laptop.source, NodeEvidence::Sessions);
    }

    #[test]
    fn fleet_includes_hub_in_t2_view() {
        let t = derive_topology("node-0", Role::Full, Some("hub"), &[], &[], &[]);
        let hub = t.nodes.iter().find(|n| n.host == "hub").unwrap();
        assert_eq!(hub.source, NodeEvidence::HubConfig);
        assert!(!hub.is_self);
    }

    #[test]
    fn fleet_includes_peer_hubs_in_t3_view() {
        let peers = vec!["hub-b".to_string()];
        let t = derive_topology("hub-a", Role::Consumer, Some("hub-a"), &peers, &[], &[]);
        let hub_b = t.nodes.iter().find(|n| n.host == "hub-b").unwrap();
        assert_eq!(hub_b.source, NodeEvidence::HubConfig);
    }
}
