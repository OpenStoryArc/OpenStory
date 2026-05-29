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
use axum::extract::State;
use serde::Serialize;
use serde_json::Value;

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

/// What `/api/admin/topology` returns.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Topology {
    pub shape: TopologyShape,
    #[serde(rename = "self")]
    pub self_: NodeInfo,
}

/// Pure derivation: shape and node info from inputs. No I/O, no env reads
/// — those happen at the handler boundary. Testable in isolation.
pub fn derive_topology(
    host: &str,
    role: Role,
    hub_domain: Option<&str>,
    peer_hub_domains: &[String],
    peer_domains: &[String],
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
    }
}

/// `GET /api/admin/topology` — read this node's federation view.
///
/// Reads the same env vars the CLI boot path consumes, then delegates to
/// the pure [`derive_topology`]. Side-effects (env reads) at the edge;
/// the discrimination logic stays testable.
pub async fn get_topology(State(state): State<SharedState>) -> Json<Value> {
    log_event("api", "GET /api/admin/topology");
    let s = state.read().await;
    let role = s.config.role;
    drop(s);

    let hub_domain = std::env::var("OPEN_STORY_HUB_DOMAIN")
        .ok()
        .filter(|v| !v.is_empty());
    let peer_hub_domains: Vec<String> = std::env::var("OPEN_STORY_PEER_HUB_DOMAINS")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let peer_domains: Vec<String> = std::env::var("OPEN_STORY_PEER_DOMAINS")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let topology = derive_topology(
        open_story_core::host::host(),
        role,
        hub_domain.as_deref(),
        &peer_hub_domains,
        &peer_domains,
    );
    Json(serde_json::to_value(&topology).expect("Topology serializes"))
}

#[cfg(test)]
mod tests {
    //! Pure derivation tests — no env, no I/O. The handler delegates
    //! straight to `derive_topology`, so this is the contract.
    use super::*;

    #[test]
    fn solo_with_no_flags() {
        let t = derive_topology("a1", Role::Full, None, &[], &[]);
        assert_eq!(t.shape, TopologyShape::Solo);
        assert_eq!(t.self_.role, NodeRole::Solo);
        assert_eq!(t.self_.domain, None);
        assert!(t.self_.hub_domain.is_none());
    }

    #[test]
    fn hub_with_no_peer_hubs_is_t2() {
        let t = derive_topology("hub", Role::Consumer, Some("hub"), &[], &[]);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Hub);
        assert_eq!(t.self_.domain.as_deref(), Some("hub"));
        assert_eq!(t.self_.hub_domain.as_deref(), Some("hub"));
    }

    #[test]
    fn hub_with_peer_hubs_is_t3() {
        let peers = vec!["hub-b".to_string()];
        let t = derive_topology("hub-a", Role::Consumer, Some("hub-a"), &peers, &[]);
        assert_eq!(t.shape, TopologyShape::T3);
        assert_eq!(t.self_.role, NodeRole::Hub);
        assert_eq!(t.self_.peer_hub_domains, peers);
    }

    #[test]
    fn leaf_attached_to_hub_is_t2_from_its_vantage() {
        // Even if the hub itself is part of a T3 mesh, the LEAF sees
        // exactly one hub. Cross-hub fanout is the hub's concern.
        let t = derive_topology("node-0", Role::Full, Some("hub-a"), &[], &[]);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Leaf);
        assert_eq!(t.self_.domain.as_deref(), Some("node-0"));
        assert_eq!(t.self_.hub_domain.as_deref(), Some("hub-a"));
        assert!(t.self_.peer_hub_domains.is_empty());
    }

    #[test]
    fn peers_only_is_t1_mesh() {
        let peers = vec!["laptop".to_string(), "phone".to_string()];
        let t = derive_topology("a1", Role::Full, None, &[], &peers);
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
        let t = derive_topology("node-0", Role::Full, Some("hub"), &[], &peers);
        assert_eq!(t.shape, TopologyShape::T2);
        assert_eq!(t.self_.role, NodeRole::Leaf);
    }

    #[test]
    fn topology_serializes_to_expected_shape() {
        let t = derive_topology("node-0", Role::Full, Some("hub"), &[], &[]);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["shape"], "t2");
        assert_eq!(v["self"]["host"], "node-0");
        assert_eq!(v["self"]["role"], "leaf");
        assert_eq!(v["self"]["hub_domain"], "hub");
    }
}
