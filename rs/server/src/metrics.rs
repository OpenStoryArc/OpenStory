//! Prometheus metrics endpoint and instrumentation helpers.
//!
//! When `metrics_enabled` is true, exposes `/metrics` endpoint for Prometheus scraping.
//! Counters, gauges, and histograms track the event pipeline health.

use axum::http::StatusCode;

/// Metric names — centralized to avoid typo bugs and enable test assertions.
pub mod names {
    pub const EVENTS_INGESTED: &str = "events_ingested_total";
    pub const EVENTS_DEDUPED: &str = "events_deduped_total";
    pub const HOOKS_RECEIVED: &str = "hooks_received_total";
    pub const PATTERNS_DETECTED: &str = "patterns_detected_total";
    pub const WS_MESSAGES_SENT: &str = "ws_messages_sent_total";

    pub const SESSIONS_ACTIVE: &str = "sessions_active";
    pub const SESSIONS_TOTAL: &str = "sessions_total";
    pub const WS_CLIENTS_CONNECTED: &str = "ws_clients_connected";

    pub const INGEST_DURATION: &str = "ingest_duration_seconds";
    pub const HOOK_DURATION: &str = "hook_duration_seconds";
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

/// Record a hook received.
pub fn record_hook_received() {
    metrics::counter!(names::HOOKS_RECEIVED).increment(1);
}

/// Record patterns detected.
pub fn record_patterns_detected(count: u64) {
    metrics::counter!(names::PATTERNS_DETECTED).increment(count);
}

/// Record a WebSocket message sent.
pub fn record_ws_message_sent() {
    metrics::counter!(names::WS_MESSAGES_SENT).increment(1);
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

/// Build a Router with a single GET /metrics route, capturing the handle.
pub fn metrics_router(handle: metrics_exporter_prometheus::PrometheusHandle) -> axum::Router {
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(move || {
            let h = handle.clone();
            async move { (StatusCode::OK, h.render()) }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_names_are_prometheus_valid() {
        let all_names = [
            names::EVENTS_INGESTED,
            names::EVENTS_DEDUPED,
            names::HOOKS_RECEIVED,
            names::PATTERNS_DETECTED,
            names::WS_MESSAGES_SENT,
            names::SESSIONS_ACTIVE,
            names::SESSIONS_TOTAL,
            names::WS_CLIENTS_CONNECTED,
            names::INGEST_DURATION,
            names::HOOK_DURATION,
        ];
        for name in all_names {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "metric name {name} contains invalid characters"
            );
            assert!(
                !name.is_empty(),
                "metric name should not be empty"
            );
        }
    }

    #[test]
    fn record_functions_do_not_panic_without_recorder() {
        // When no recorder is installed, metrics calls are no-ops.
        // This verifies they don't panic.
        record_events_ingested("message.user.prompt", 5);
        record_events_deduped(2);
        record_hook_received();
        record_patterns_detected(3);
        record_ws_message_sent();
        set_sessions_active(10);
        set_sessions_total(42);
        set_ws_clients(3);
    }
}
