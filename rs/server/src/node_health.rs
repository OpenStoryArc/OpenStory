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
    env!("OPEN_STORY_GIT_SHA")
}

/// When this binary was built, RFC 3339 UTC.
pub fn built_at() -> &'static str {
    env!("OPEN_STORY_BUILT_AT")
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
