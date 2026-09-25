//! Pure pieces of `/api/health` (REQUIREMENTS H-06): the leaf link and the
//! per-watcher detail. The handler fetches; these decide.

use crate::watcher_diagnostics::WatcherSnapshot;
use serde_json::{json, Value};

/// `host:port` of a NATS URL with scheme and userinfo removed, so a hub
/// address can appear on a health page without its token.
pub fn redact_hub(url: &str) -> Option<String> {
    let s = url.trim();
    if s.is_empty() {
        return None;
    }
    let s = s.split("://").last().unwrap_or(s);
    let s = s.rsplit('@').next().unwrap_or(s);
    let s = s.split('/').next().unwrap_or(s);
    (!s.is_empty()).then(|| s.to_string())
}

/// The NATS monitoring endpoint for the server behind `nats_url`: same
/// host, port 8222 (what the managed leaf config and `nats.conf` set).
pub fn monitor_url(nats_url: &str) -> String {
    let host = redact_hub(nats_url)
        .and_then(|hp| hp.rsplit_once(':').map(|(h, _)| h.to_string()).or(Some(hp)))
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let host = if host == "localhost" {
        "127.0.0.1".to_string()
    } else {
        host
    };
    format!("http://{host}:8222")
}

/// Whether `/leafz` from the monitoring port shows at least one leaf link.
pub fn leaf_connected(leafz: &Value) -> bool {
    leafz.get("leafnodes").and_then(|n| n.as_u64()).unwrap_or(0) > 0
}

/// The leaf block: configured from config, connected from `/leafz` (None
/// when the monitor could not be reached), hub redacted.
pub fn leaf_report(nats_leaf_url: &str, leafz: Option<&Value>) -> Value {
    let hub = redact_hub(nats_leaf_url);
    let configured = hub.is_some();
    json!({
        "configured": configured,
        "connected": configured && leafz.is_some_and(leaf_connected),
        "hub": hub,
        "monitor": if leafz.is_some() { "ok" } else { "unreachable" },
    })
}

/// Per-watcher detail: actor, agent, root, last event time, its age in
/// seconds against `now`, and publish failures since boot.
pub fn watcher_detail(
    snapshots: &[WatcherSnapshot],
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Value> {
    snapshots
        .iter()
        .map(|w| {
            let age_secs = w
                .last_event_at
                .as_deref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .map(|t| (now - t.with_timezone(&chrono::Utc)).num_seconds().max(0));
            json!({
                "actor": w.actor,
                "agent": w.agent,
                "root": w.root,
                "last_event_at": w.last_event_at,
                "age_secs": age_secs,
                "publish_failures": w.counters.publish_failures,
            })
        })
        .collect()
}

// ── Verdict (M-01) ───────────────────────────────────────────────────────────

/// Stream fill that warns, and the fill that is critical.
pub const STREAM_WARN: f64 = 0.70;
pub const STREAM_CRIT: f64 = 0.90;
/// A watcher quiet this long warns, and this long is critical.
pub const WATCHER_WARN_SECS: i64 = 300;
pub const WATCHER_CRIT_SECS: i64 = 3600;

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

/// The node's verdict from its own health body: `level` (ok, warn,
/// critical) and `findings` worst first, each with a stable `id` a
/// proposal can cite as evidence. The same rules as
/// `scripts/node_health_probe.py` and the header dot, computed once here so
/// every reader agrees. Pure.
pub fn verdict(body: &Value) -> Value {
    let mut findings: Vec<Value> = Vec::new();

    if body["bus"]["connected"] == json!(false) {
        findings.push(finding(
            "critical",
            "bus_disconnected".into(),
            "the bus is not connected".into(),
        ));
    }
    let leaf = &body["leaf"];
    if leaf["configured"] == json!(true) && leaf["connected"] != json!(true) {
        let hub = leaf["hub"].as_str().unwrap_or("the hub");
        findings.push(finding(
            "critical",
            "leaf_down".into(),
            format!("the leaf link to {hub} is not connected"),
        ));
    }
    if let Some(phase) = body["boot"]["phase"].as_str() {
        if phase != "serving" {
            let r = &body["boot"]["replay"];
            let text = match (r["done"].as_u64(), r["total"].as_u64()) {
                (Some(d), Some(t)) if t > 0 => format!("replaying {d} of {t} sessions"),
                _ => format!("the node is {phase}"),
            };
            findings.push(finding("warn", "replaying".into(), text));
        }
    }
    if body["projections"]["fresh"] == json!(false) {
        findings.push(finding(
            "warn",
            "projections_stale".into(),
            format!(
                "projections {} of {} sessions; run reproject",
                body["projections"]["count"], body["projections"]["sessions"]
            ),
        ));
    }
    for s in body["streams"].as_array().into_iter().flatten() {
        let name = s["name"].as_str().unwrap_or("?");
        if let Some(pct) = s["percent"].as_f64() {
            let level = if pct >= STREAM_CRIT {
                "critical"
            } else if pct >= STREAM_WARN {
                "warn"
            } else {
                continue;
            };
            findings.push(finding(
                level,
                format!("stream_cap:{name}"),
                format!("stream {name} is at {:.0}% of its cap", pct * 100.0),
            ));
        }
    }
    let serving = body["boot"]["phase"]
        .as_str()
        .is_none_or(|p| p == "serving");
    if let Some(consumers) = body["consumers"].as_object() {
        let mut names: Vec<&String> = consumers.keys().collect();
        names.sort();
        for name in names {
            let c = &consumers[name];
            let restarts = c["restarts"].as_u64().unwrap_or(0);
            if c["state"] == json!("pending_start") {
                // B-05: held until the node serves; not dead. Still pending
                // once it serves is worth a look.
                if serving {
                    findings.push(finding(
                        "warn",
                        format!("consumer_pending:{name}"),
                        format!("consumer {name} has not started yet"),
                    ));
                }
            } else if c["alive"] == json!(false) {
                findings.push(finding(
                    "critical",
                    format!("consumer_dead:{name}"),
                    format!("consumer {name} is not alive ({restarts} restarts)"),
                ));
            } else if restarts > 0 {
                findings.push(finding(
                    "warn",
                    format!("consumer_restarted:{name}"),
                    format!("consumer {name} restarted {restarts} times"),
                ));
            }
        }
    }
    for w in body["watchers_detail"].as_array().into_iter().flatten() {
        let actor = w["actor"].as_str().unwrap_or("?");
        if let Some(age) = w["age_secs"].as_i64() {
            let level = if age > WATCHER_CRIT_SECS {
                Some("critical")
            } else if age > WATCHER_WARN_SECS {
                Some("warn")
            } else {
                None
            };
            if let Some(level) = level {
                findings.push(finding(
                    level,
                    format!("watcher_quiet:{actor}"),
                    format!("watcher {actor} last saw an event {age} s ago"),
                ));
            }
        }
        if let Some(n) = w["publish_failures"].as_u64().filter(|n| *n > 0) {
            findings.push(finding(
                "warn",
                format!("publish_failures:{actor}"),
                format!("watcher {actor} has {n} publish failures since boot"),
            ));
        }
    }
    if let Some(n) = body["presence"]["failures"].as_u64().filter(|n| *n > 0) {
        let last = body["presence"]["last_error"].as_str().unwrap_or("");
        findings.push(finding(
            "warn",
            "presence_failures".into(),
            format!("{n} presence beats failed to publish; last: {last}"),
        ));
    }

    findings.sort_by_key(|f| std::cmp::Reverse(rank(f["level"].as_str().unwrap_or("ok"))));
    let level = findings
        .iter()
        .map(|f| f["level"].as_str().unwrap_or("ok"))
        .max_by_key(|l| rank(l))
        .unwrap_or("ok");
    json!({ "level": level, "findings": findings })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod when_a_hub_url_carries_a_token {
        use super::*;
        #[test]
        fn it_keeps_only_host_and_port() {
            assert_eq!(
                redact_hub("nats://tok@debian-16gb-ash-1:7422").as_deref(),
                Some("debian-16gb-ash-1:7422")
            );
            assert_eq!(redact_hub("nats://u:p@h:1/x").as_deref(), Some("h:1"));
            assert_eq!(redact_hub(""), None);
            assert_eq!(redact_hub("   "), None);
        }
    }

    mod when_the_monitor_answers {
        use super::*;
        #[test]
        fn it_reads_leaf_links_from_leafz() {
            assert!(leaf_connected(&json!({"leafnodes": 1, "leafs": [{}]})));
            assert!(!leaf_connected(&json!({"leafnodes": 0, "leafs": []})));
            assert!(!leaf_connected(&json!({})));
            let r = leaf_report("nats://t@hub:7422", Some(&json!({"leafnodes": 1})));
            assert_eq!(r["connected"], true);
            assert_eq!(r["hub"], "hub:7422");
            let r = leaf_report("nats://t@hub:7422", None);
            assert_eq!(r["connected"], false);
            assert_eq!(r["monitor"], "unreachable");
            let r = leaf_report("", Some(&json!({"leafnodes": 3})));
            assert_eq!(r["configured"], false);
            assert_eq!(
                r["connected"], false,
                "no hub configured means no leaf, whatever the monitor says"
            );
        }
    }

    mod when_monitor_url_is_derived {
        use super::*;
        #[test]
        fn it_uses_the_nats_host_on_8222() {
            assert_eq!(
                monitor_url("nats://u:p@localhost:4222"),
                "http://127.0.0.1:8222"
            );
            assert_eq!(monitor_url("nats://10.0.0.5:4322"), "http://10.0.0.5:8222");
        }
    }
}

// ── H-07: build stamp and process facts ─────────────────────────────────

/// The short git sha this binary was built from ("unknown" outside a repo).
pub fn git_sha() -> &'static str {
    // "unknown" when no build script ran (a source tree without build.rs
    // copied in, as a container build once was); never a failed compile.
    option_env!("OPEN_STORY_GIT_SHA").unwrap_or("unknown")
}

/// When this binary was built, RFC 3339 UTC.
pub fn built_at() -> &'static str {
    option_env!("OPEN_STORY_BUILT_AT").unwrap_or("unknown")
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Call once at boot so uptime counts from process start; otherwise it
/// counts from the first health read.
pub fn mark_started() {
    let _ = STARTED.set(std::time::Instant::now());
}

pub fn uptime_secs() -> u64 {
    STARTED
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs()
}

/// Bytes on disk under the data dir (store, JSONL, plans, reels, logs).
pub fn store_size_bytes(data_dir: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(data_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// The allocator the running binary installed (B-06): "jemalloc" or
/// "system". The CLI sets it at start; the library default is "system".
pub fn allocator() -> &'static str {
    ALLOCATOR.get().copied().unwrap_or("system")
}

static ALLOCATOR: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Record the allocator name once; later calls keep the first.
pub fn set_allocator(name: &'static str) {
    let _ = ALLOCATOR.set(name);
}

/// Resident set size of this process, via `ps` (macOS and Linux agree on
/// `-o rss=` in kilobytes). None when `ps` is unavailable.
pub fn process_rss_bytes() -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
        .map(|kb| kb * 1024)
}

#[cfg(test)]
mod stamp_tests {
    use super::*;

    mod when_the_build_is_stamped {
        use super::*;
        #[test]
        fn it_names_a_sha_and_an_rfc3339_time() {
            let sha = git_sha();
            assert!(
                sha == "unknown" || sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{sha}"
            );
            assert!(
                chrono::DateTime::parse_from_rfc3339(built_at()).is_ok(),
                "{}",
                built_at()
            );
        }
    }

    mod when_process_facts_are_read {
        use super::*;
        #[test]
        fn it_measures_this_process_and_a_directory() {
            assert!(process_rss_bytes().unwrap_or(0) > 0);
            let tmp = tempfile::tempdir().unwrap();
            std::fs::write(tmp.path().join("a.bin"), vec![0u8; 1500]).unwrap();
            std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
            std::fs::write(tmp.path().join("sub/b.bin"), vec![0u8; 500]).unwrap();
            assert_eq!(store_size_bytes(tmp.path()), 2000);
            assert!(uptime_secs() < 3600);
        }
    }
}
