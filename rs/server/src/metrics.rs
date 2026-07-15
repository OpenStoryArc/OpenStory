//! Prometheus metrics endpoint and instrumentation helpers.
//!
//! When `metrics_enabled` is true, exposes `/metrics` endpoint for Prometheus scraping.
//! Counters, gauges, and histograms track the event pipeline health.

use axum::http::StatusCode;

/// Metric names — centralized to avoid typo bugs and enable test assertions.
pub mod names {
    pub const EVENTS_INGESTED: &str = "events_ingested_total";
    pub const EVENTS_DEDUPED: &str = "events_deduped_total";
    pub const PATTERNS_DETECTED: &str = "patterns_detected_total";
    pub const WS_MESSAGES_SENT: &str = "ws_messages_sent_total";
    pub const WATCHER_RAW_EVENTS: &str = "watcher_raw_events_total";
    pub const WATCHER_IGNORED_EVENTS: &str = "watcher_ignored_events_total";
    pub const WATCHER_FILES_PROCESSED: &str = "watcher_files_processed_total";
    pub const WATCHER_ZERO_NEW_BYTE_READS: &str = "watcher_zero_new_byte_reads_total";
    pub const WATCHER_CLOUD_EVENTS_EMITTED: &str = "watcher_cloud_events_emitted_total";
    pub const WATCHER_PUBLISH_FAILURES: &str = "watcher_publish_failures_total";
    pub const WATCHER_LAST_EVENT_TIMESTAMP: &str = "watcher_last_event_timestamp_seconds";
    pub const WATCHER_LAST_SUCCESS_TIMESTAMP: &str = "watcher_last_success_timestamp_seconds";

    pub const SESSIONS_ACTIVE: &str = "sessions_active";
    pub const SESSIONS_TOTAL: &str = "sessions_total";
    pub const WS_CLIENTS_CONNECTED: &str = "ws_clients_connected";

    // HOOKS_RECEIVED, HOOK_DURATION, INGEST_DURATION removed 2026-04-15
    // — registered but never recorded into. The /hooks endpoint that
    // would have populated HOOKS_RECEIVED was retired; the duration
    // histograms had no `histogram!()` callsite anywhere in the
    // codebase. Audit: docs/research/architecture-audit/HOOKS_RETIREMENT_AUDIT.md
}

/// Initialize the Prometheus recorder. Call once at startup.
///
/// Returns `None` if the recorder was already installed (e.g., in tests).
pub fn init_recorder() -> Option<metrics_exporter_prometheus::PrometheusHandle> {
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    builder.install_recorder().ok()
}

/// Record an event ingestion (counter increment by subtype).
pub fn record_events_ingested(subtype: &str, count: u64) {
    metrics::counter!(names::EVENTS_INGESTED, "subtype" => subtype.to_string()).increment(count);
}

/// Record deduplicated events.
pub fn record_events_deduped(count: u64) {
    metrics::counter!(names::EVENTS_DEDUPED).increment(count);
}

/// Record patterns detected.
pub fn record_patterns_detected(count: u64) {
    metrics::counter!(names::PATTERNS_DETECTED).increment(count);
}

/// Record a WebSocket message sent.
pub fn record_ws_message_sent() {
    metrics::counter!(names::WS_MESSAGES_SENT).increment(1);
}

pub fn record_watcher_raw_event(actor: &str, kind: &str) {
    metrics::counter!(
        names::WATCHER_RAW_EVENTS,
        "actor" => actor.to_string(),
        "kind" => kind.to_string()
    )
    .increment(1);
    set_watcher_last_event_now(actor);
}

pub fn record_watcher_ignored_event(actor: &str, reason: &str) {
    metrics::counter!(
        names::WATCHER_IGNORED_EVENTS,
        "actor" => actor.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

pub fn record_watcher_file_processed(actor: &str, emitted: u64, zero_new_bytes: bool) {
    metrics::counter!(names::WATCHER_FILES_PROCESSED, "actor" => actor.to_string()).increment(1);
    if zero_new_bytes {
        metrics::counter!(names::WATCHER_ZERO_NEW_BYTE_READS, "actor" => actor.to_string())
            .increment(1);
    }
    if emitted > 0 {
        metrics::counter!(names::WATCHER_CLOUD_EVENTS_EMITTED, "actor" => actor.to_string())
            .increment(emitted);
    }
}

pub fn record_watcher_publish(actor: &str, success: bool) {
    if success {
        set_watcher_last_success_now(actor);
    } else {
        metrics::counter!(names::WATCHER_PUBLISH_FAILURES, "actor" => actor.to_string())
            .increment(1);
    }
}

/// Update active session count gauge.
pub fn set_sessions_active(count: u64) {
    metrics::gauge!(names::SESSIONS_ACTIVE).set(count as f64);
}

/// Update total session count gauge.
pub fn set_sessions_total(count: u64) {
    metrics::gauge!(names::SESSIONS_TOTAL).set(count as f64);
}

/// Update connected WebSocket client count.
pub fn set_ws_clients(count: u64) {
    metrics::gauge!(names::WS_CLIENTS_CONNECTED).set(count as f64);
}

fn set_watcher_last_event_now(actor: &str) {
    metrics::gauge!(names::WATCHER_LAST_EVENT_TIMESTAMP, "actor" => actor.to_string())
        .set(chrono::Utc::now().timestamp() as f64);
}

fn set_watcher_last_success_now(actor: &str) {
    metrics::gauge!(names::WATCHER_LAST_SUCCESS_TIMESTAMP, "actor" => actor.to_string())
        .set(chrono::Utc::now().timestamp() as f64);
}

/// Render the bounded read-through projection cache gauges as Prometheus
/// exposition lines. Pure function — no I/O, no global recorder — so tests
/// can assert on exact output without standing up a Prometheus handle.
///
/// One line per gauge/counter, `name value\n`. `evictions` is a monotonic
/// counter (hence `_total`); the rest are point-in-time gauges.
pub fn render_cache_metrics(
    proj_bytes: u64,
    proj_sessions: usize,
    evictions: u64,
    payload_bytes: u64,
) -> String {
    format!(
        "openstory_projection_cache_bytes {proj_bytes}\n\
         openstory_projection_resident_sessions {proj_sessions}\n\
         openstory_projection_evictions_total {evictions}\n\
         openstory_payload_cache_bytes {payload_bytes}\n"
    )
}

/// Build a Router with a single GET /metrics route, capturing the Prometheus
/// handle plus a `SharedState` clone so the response can append live
/// projection/payload cache gauges (see `render_cache_metrics`) after the
/// standard `metrics` crate output.
pub fn metrics_router(
    handle: metrics_exporter_prometheus::PrometheusHandle,
    state: crate::state::SharedState,
) -> axum::Router {
    axum::Router::new()
        .route(
            "/metrics",
            axum::routing::get(
                move |axum::extract::State(state): axum::extract::State<
                    crate::state::SharedState,
                >| {
                    let h = handle.clone();
                    async move {
                        let s = state.read().await;
                        let cache_metrics = render_cache_metrics(
                            s.store.projections.resident_bytes(),
                            s.store.projections.resident_sessions(),
                            s.store.projections.evictions(),
                            s.store.full_payloads.resident_bytes(),
                        );
                        let body = format!("{}{}", h.render(), cache_metrics);
                        (StatusCode::OK, body)
                    }
                },
            ),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_names_are_prometheus_valid() {
        let all_names = [
            names::EVENTS_INGESTED,
            names::EVENTS_DEDUPED,
            names::PATTERNS_DETECTED,
            names::WS_MESSAGES_SENT,
            names::WATCHER_RAW_EVENTS,
            names::WATCHER_IGNORED_EVENTS,
            names::WATCHER_FILES_PROCESSED,
            names::WATCHER_ZERO_NEW_BYTE_READS,
            names::WATCHER_CLOUD_EVENTS_EMITTED,
            names::WATCHER_PUBLISH_FAILURES,
            names::WATCHER_LAST_EVENT_TIMESTAMP,
            names::WATCHER_LAST_SUCCESS_TIMESTAMP,
            names::SESSIONS_ACTIVE,
            names::SESSIONS_TOTAL,
            names::WS_CLIENTS_CONNECTED,
        ];
        for name in all_names {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "metric name {name} contains invalid characters"
            );
            assert!(!name.is_empty(), "metric name should not be empty");
        }
    }

    #[test]
    fn record_functions_do_not_panic_without_recorder() {
        // When no recorder is installed, metrics calls are no-ops.
        // This verifies they don't panic.
        record_events_ingested("message.user.prompt", 5);
        record_events_deduped(2);
        record_patterns_detected(3);
        record_ws_message_sent();
        record_watcher_raw_event("codex", "Modify(Data)");
        record_watcher_ignored_event("codex", "non_jsonl");
        record_watcher_file_processed("codex", 2, false);
        record_watcher_publish("codex", true);
        record_watcher_publish("codex", false);
        set_sessions_active(10);
        set_sessions_total(42);
        set_ws_clients(3);
    }

    #[test]
    fn metrics_report_cache_gauges() {
        let m = render_cache_metrics(
            /*proj_bytes*/ 123, /*proj_sessions*/ 4, /*evictions*/ 2,
            /*payload_bytes*/ 55,
        );
        assert!(m.contains("openstory_projection_cache_bytes 123"));
        assert!(m.contains("openstory_projection_resident_sessions 4"));
        assert!(m.contains("openstory_projection_evictions_total 2"));
        assert!(m.contains("openstory_payload_cache_bytes 55"));
    }
}
