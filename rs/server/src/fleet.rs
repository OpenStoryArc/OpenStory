//! Convergence digests + verify diff — the shared primitive behind network
//! health (`/api/fleet`) and the `verify` state-management action.
//!
//! A per-session digest is `(count, stable hash of the sorted event-id set)`.
//! Two nodes agree on a session iff their digests match. The hash is FNV-1a,
//! NOT std's `DefaultHasher` (SipHash) — `DefaultHasher` is not guaranteed
//! stable across machines or Rust versions, which would make cross-node
//! comparison meaningless. FNV-1a is deterministic everywhere, which is the
//! whole point: a digest computed on a leaf must equal the digest computed on
//! the hub for the same event-id set.
//!
//! `diff_digests` is `verify`: given this node's digests and a peer's, classify
//! each session as converged, missing-here (peer has, we lack → catch-up pulls
//! these), missing-there, or diverged (same session, different id set). It is
//! also the convergence signal for fleet health. See
//! `docs/research/node-and-network-health.md` and
//! `docs/research/state-management-interface.md`.

use std::collections::HashMap;

use serde::Serialize;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionDigest {
    pub session_id: String,
    pub count: usize,
    pub digest: String,
}

/// Order-independent digest of an event-id set. Ids are sorted first, so two
/// nodes that received the same events in different arrival orders still match.
/// A record separator between ids prevents `["ab","c"]` and `["a","bc"]` from
/// colliding.
pub fn digest_event_ids(ids: &[String]) -> String {
    let mut sorted: Vec<&str> = ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut h: u64 = FNV_OFFSET;
    for id in sorted {
        for &b in id.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        // record separator (0x1e) so concatenation boundaries matter
        h ^= 0x1e;
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}

/// The result of comparing this node's digests against a peer's — i.e. `verify`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct FleetDiff {
    /// True when both nodes hold exactly the same sessions with the same digests.
    pub converged: bool,
    /// Sessions the peer has that this node lacks — `catch-up` would pull these.
    pub missing_here: Vec<String>,
    /// Sessions this node has that the peer lacks.
    pub missing_there: Vec<String>,
    /// Sessions both have but whose event-id sets differ.
    pub diverged: Vec<String>,
}

/// Compare two digest sets. Pure and symmetric in structure: `missing_here` is
/// what `local` must pull to converge; `missing_there` is what the peer must.
pub fn diff_digests(local: &[SessionDigest], remote: &[SessionDigest]) -> FleetDiff {
    let lmap: HashMap<&str, &SessionDigest> =
        local.iter().map(|d| (d.session_id.as_str(), d)).collect();
    let rmap: HashMap<&str, &SessionDigest> =
        remote.iter().map(|d| (d.session_id.as_str(), d)).collect();

    let mut diff = FleetDiff::default();
    for r in remote {
        if !lmap.contains_key(r.session_id.as_str()) {
            diff.missing_here.push(r.session_id.clone());
        }
    }
    for l in local {
        match rmap.get(l.session_id.as_str()) {
            None => diff.missing_there.push(l.session_id.clone()),
            Some(r) if r.digest != l.digest => diff.diverged.push(l.session_id.clone()),
            _ => {}
        }
    }
    diff.missing_here.sort();
    diff.missing_there.sort();
    diff.diverged.sort();
    diff.converged =
        diff.missing_here.is_empty() && diff.missing_there.is_empty() && diff.diverged.is_empty();
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn digest_is_order_independent() {
        assert_eq!(
            digest_event_ids(&ids(&["a", "b", "c"])),
            digest_event_ids(&ids(&["c", "a", "b"])),
            "same id set in any order must produce the same digest"
        );
    }

    #[test]
    fn digest_differs_on_different_sets() {
        assert_ne!(
            digest_event_ids(&ids(&["a", "b"])),
            digest_event_ids(&ids(&["a", "b", "c"])),
            "a missing/extra id must change the digest"
        );
    }

    #[test]
    fn digest_separator_prevents_concatenation_collision() {
        assert_ne!(
            digest_event_ids(&ids(&["ab", "c"])),
            digest_event_ids(&ids(&["a", "bc"])),
        );
    }

    #[test]
    fn empty_set_has_stable_digest() {
        assert_eq!(digest_event_ids(&[]), digest_event_ids(&[]));
    }

    #[test]
    fn diff_reports_converged_when_equal() {
        let a = vec![SessionDigest {
            session_id: "s1".into(),
            count: 2,
            digest: digest_event_ids(&ids(&["e1", "e2"])),
        }];
        let b = a.clone();
        let d = diff_digests(&a, &b);
        assert!(d.converged);
        assert!(d.missing_here.is_empty() && d.missing_there.is_empty() && d.diverged.is_empty());
    }

    #[test]
    fn diff_classifies_missing_and_diverged() {
        let local = vec![
            SessionDigest {
                session_id: "shared-same".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["x"])),
            },
            SessionDigest {
                session_id: "shared-diff".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["local-only-evt"])),
            },
            SessionDigest {
                session_id: "only-local".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["y"])),
            },
        ];
        let remote = vec![
            SessionDigest {
                session_id: "shared-same".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["x"])),
            },
            SessionDigest {
                session_id: "shared-diff".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["remote-only-evt"])),
            },
            SessionDigest {
                session_id: "only-remote".into(),
                count: 1,
                digest: digest_event_ids(&ids(&["z"])),
            },
        ];
        let d = diff_digests(&local, &remote);
        assert!(!d.converged);
        assert_eq!(d.missing_here, vec!["only-remote".to_string()]);
        assert_eq!(d.missing_there, vec!["only-local".to_string()]);
        assert_eq!(d.diverged, vec!["shared-diff".to_string()]);
    }
}
