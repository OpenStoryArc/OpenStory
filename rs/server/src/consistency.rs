//! The consistency report (consistency C-03), pure.
//!
//! The store is a grow-only set of immutable events; the merge of two nodes
//! is set union; everything else is a fold. So "consistent" is checkable:
//! two nodes agree when their roll-ups are equal, a node is caught up when
//! its peers have persisted its newest events, a node is internally sound
//! when its store and its JSONL backup agree. `report` reads snapshots
//! taken from health and presence bodies and answers in the verdict's
//! shape (`{level, findings: [{id, level, text}]}`), so the header dot, the
//! probe script, and an agent read it the way they read `/api/health`.
//!
//! Finding ids: `diverged:<host>`, `behind:<host>`, `lag:<consumer>`,
//! `unverified`, `stale_snapshot:<host>`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::fleet::{differing_projects, DigestRollup};

/// What one node last said about itself, as far as consistency cares:
/// taken from its health body (local) or its latest beat (a peer).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub host: String,
    /// The node's store folded to host, project, and root (C-01). `None`
    /// on a build that does not carry it; such a peer is not compared.
    pub rollup: Option<DigestRollup>,
    /// Per origin host, the newest event time the node has persisted
    /// (RFC 3339). Empty until C-02 fills it.
    pub watermarks: BTreeMap<String, String>,
    /// The node's last `verify` result (`agree`, `fts_unindexed`, …).
    pub verify: Option<Value>,
    /// Per consumer, batches queued behind its last receive.
    pub consumers: BTreeMap<String, u64>,
    /// A peer whose beat is older than three intervals.
    pub stale: bool,
    pub age_secs: Option<i64>,
}

impl Snapshot {
    /// From a health body (what `/api/health` and the beat carry).
    pub fn from_health(body: &Value) -> Self {
        let rollup = serde_json::from_value::<DigestRollup>(body["rollup"].clone()).ok();
        let watermarks = body["watermarks"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
        let consumers = body["consumers"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v["lag"].as_u64().unwrap_or(0)))
            .collect();
        Snapshot {
            host: body["host"].as_str().unwrap_or("").to_string(),
            rollup,
            watermarks,
            verify: body.get("verify").filter(|v| v.is_object()).cloned(),
            consumers,
            stale: false,
            age_secs: None,
        }
    }

    /// From one node of `/api/fleet/presence` (`presence::fleet_view`).
    pub fn from_presence(node: &Value) -> Self {
        let mut s = Self::from_health(&node["body"]);
        if s.host.is_empty() {
            s.host = node["host"].as_str().unwrap_or("").to_string();
        }
        s.stale = node["stale"].as_bool().unwrap_or(false);
        s.age_secs = node["age_secs"].as_i64();
        s
    }
}

/// A peer's watermark for us may trail ours by this many beats before it
/// is behind: the ordinary lag of an interval-paced fleet.
pub const BEHIND_AFTER_BEATS: i64 = 2;
/// A consumer with more than this many batches queued is lagging.
pub const LAG_BATCHES: u64 = 1;

fn rank(level: &str) -> u8 {
    match level {
        "critical" => 2,
        "warn" => 1,
        _ => 0,
    }
}

fn finding(level: &str, id: String, text: String) -> Value {
    json!({ "level": level, "id": id, "text": text })
}

fn parse_time(t: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(t)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// Every (host, project) either side knows.
fn project_union(a: &DigestRollup, b: &DigestRollup) -> usize {
    let mut keys: Vec<(&str, &str)> = Vec::new();
    for (host, h) in a.hosts.iter().chain(b.hosts.iter()) {
        for project in h.projects.keys() {
            let key = (host.as_str(), project.as_str());
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys.len()
}

/// The report: `local` against each peer, in the verdict's shape, findings
/// worst first, plus one `peers` row per peer saying whether it was
/// compared and on how many projects it differs. Pure.
pub fn report(local: &Snapshot, peers: &[Snapshot], interval_secs: u64) -> Value {
    let mut findings: Vec<Value> = Vec::new();

    if let Some(v) = &local.verify {
        if v["agree"] != json!(true) {
            let which = v["session_id"]
                .as_str()
                .map(|s| format!(" on {s}"))
                .unwrap_or_default();
            findings.push(finding(
                "critical",
                "unverified".into(),
                format!(
                    "the store and the JSONL backup disagree{which} (store {}, jsonl {}); run verify",
                    v["store_events"], v["jsonl_lines"]
                ),
            ));
        }
    }
    for (name, lag) in &local.consumers {
        if *lag > LAG_BATCHES {
            findings.push(finding(
                "warn",
                format!("lag:{name}"),
                format!("consumer {name} has {lag} batches queued behind its last receive"),
            ));
        }
    }

    let behind_after = (interval_secs as i64).max(1) * BEHIND_AFTER_BEATS;
    let mut peer_rows = Vec::with_capacity(peers.len());
    for peer in peers {
        let mut differing = 0usize;
        let mut compared = false;
        if peer.stale {
            let age = match peer.age_secs {
                Some(a) => format!("{a} s"),
                None => "an unreadable time".to_string(),
            };
            findings.push(finding(
                "warn",
                format!("stale_snapshot:{}", peer.host),
                format!(
                    "the last beat from {} is {age} old (past {} intervals of {interval_secs} s)",
                    peer.host,
                    crate::presence::STALE_AFTER_BEATS
                ),
            ));
        }
        if let (Some(mine), Some(theirs)) = (&local.rollup, &peer.rollup) {
            compared = true;
            let diff = differing_projects(mine, theirs);
            differing = diff.len();
            if differing > 0 {
                let total = project_union(mine, theirs);
                let level = if differing * 2 > total {
                    "critical"
                } else {
                    "warn"
                };
                let noun = if differing == 1 {
                    "project"
                } else {
                    "projects"
                };
                findings.push(finding(
                    level,
                    format!("diverged:{}", peer.host),
                    format!(
                        "{} differs on {differing} {noun} of {total}; converge would union and re-fold",
                        peer.host
                    ),
                ));
            }
        }
        if let (Some(ours), Some(theirs)) = (
            local.watermarks.get(&local.host),
            peer.watermarks.get(&local.host),
        ) {
            if let (Some(o), Some(t)) = (parse_time(ours), parse_time(theirs)) {
                let gap = (o - t).num_seconds();
                if gap > behind_after {
                    findings.push(finding(
                        "warn",
                        format!("behind:{}", peer.host),
                        format!(
                            "{} has persisted our events up to {theirs}, {gap} s behind ours",
                            peer.host
                        ),
                    ));
                }
            }
        }
        peer_rows.push(json!({
            "host": peer.host,
            "compared": compared,
            "differing_projects": differing,
            "stale": peer.stale,
            "age_secs": peer.age_secs,
        }));
    }

    findings.sort_by_key(|f| std::cmp::Reverse(rank(f["level"].as_str().unwrap_or("ok"))));
    let level = findings
        .iter()
        .map(|f| f["level"].as_str().unwrap_or("ok"))
        .max_by_key(|l| rank(l))
        .unwrap_or("ok");
    json!({
        "host": local.host,
        "level": level,
        "findings": findings,
        "peers": peer_rows,
    })
}
