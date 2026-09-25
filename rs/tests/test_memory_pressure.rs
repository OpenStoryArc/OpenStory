//! B-07: say it before the kernel does (docs/research/openstory-as-node/
//! 2026-09-25-boot-pass-memory.md). Health gains
//! `process.memory_limit_bytes` (cgroup v2 `memory.max`, else null) and
//! the verdict raises `memory_pressure` — warn at 75 %, critical at 90 %
//! of the limit — the same rule the probe script and the dashboard apply.
//!
//! Run with: cargo test -p open-story --test test_memory_pressure

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story_server::node_health::{self, verdict};
use serde_json::json;

fn ids(v: &serde_json::Value) -> Vec<String> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect()
}

fn body(rss: u64, limit: Option<u64>) -> serde_json::Value {
    json!({
        "boot": {"phase": "serving", "replay": {"done": 1, "total": 1, "elapsed_ms": 1}},
        "bus": {"connected": true},
        "process": {"pid": 1, "rss_bytes": rss, "uptime_secs": 5, "memory_limit_bytes": limit},
    })
}

mod when_the_verdict_sees_memory_pressure {
    use super::*;

    #[test]
    fn it_is_critical_at_ninety_percent_of_the_limit() {
        let v = verdict(&body(4_600_000_000, Some(5_000_000_000)));
        assert_eq!(v["level"], "critical", "{v}");
        assert_eq!(ids(&v), ["memory_pressure"]);
        let text = v["findings"][0]["text"].as_str().unwrap();
        assert!(text.contains("92%") && text.contains("5.0 GB"), "{text}");
    }

    #[test]
    fn it_warns_at_seventy_five_percent() {
        let v = verdict(&body(3_800_000_000, Some(5_000_000_000)));
        assert_eq!(v["level"], "warn", "{v}");
        assert_eq!(ids(&v), ["memory_pressure"]);
    }

    #[test]
    fn it_says_nothing_below_the_threshold_or_without_a_limit() {
        assert_eq!(
            verdict(&body(3_000_000_000, Some(5_000_000_000)))["level"],
            "ok"
        );
        assert_eq!(verdict(&body(4_900_000_000, None))["level"], "ok");
    }
}

/// B-10 found the field null inside the production image: `rss_bytes`
/// shelled out to `ps`, which debian-slim does not ship, so the verdict
/// could not see pressure on the very nodes the rule was written for. On
/// Linux the number is in `/proc/self/status`.
mod when_rss_is_read_from_proc {
    use super::*;

    #[test]
    fn it_parses_vmrss_kilobytes_into_bytes() {
        let status = "Name:\topen-story\nVmPeak:\t 900000 kB\nVmRSS:\t  123456 kB\nThreads:\t9\n";
        assert_eq!(
            node_health::rss_from_proc_status(status),
            Some(123_456 * 1024)
        );
        assert_eq!(node_health::rss_from_proc_status("Name:\tx\n"), None);
        assert_eq!(node_health::rss_from_proc_status(""), None);
    }

    #[test]
    fn it_is_a_number_on_this_host_without_ps() {
        // Whatever the host, the process can measure itself.
        assert!(node_health::process_rss_bytes().unwrap_or(0) > 0);
    }
}

mod when_the_limit_is_read_from_the_cgroup {
    use super::*;

    #[test]
    fn it_parses_a_number_and_treats_max_as_no_limit() {
        assert_eq!(
            node_health::memory_limit_from_cgroup("5368709120\n"),
            Some(5_368_709_120)
        );
        assert_eq!(node_health::memory_limit_from_cgroup("max\n"), None);
        assert_eq!(node_health::memory_limit_from_cgroup(""), None);
    }

    #[tokio::test]
    async fn it_is_on_the_health_body_null_or_a_number() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state, req).await).await;
        let limit = &body["process"]["memory_limit_bytes"];
        assert!(
            limit.is_null() || limit.is_u64(),
            "present, null outside a cgroup: {body}"
        );
        assert!(
            body["process"]
                .as_object()
                .unwrap()
                .contains_key("memory_limit_bytes"),
            "{body}"
        );
    }
}
