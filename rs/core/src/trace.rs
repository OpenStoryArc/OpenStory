//! Per-event stage spans (O-03).
//!
//! Every event that flows through the node can carry one `event` span per
//! stage: translate (the reader), publish (the watcher), persist (the
//! store). Each span names `stage`, `event_id`, `session_id`, `actor`, and,
//! from publish on, `subject`, so a trace reader joins the stages on the
//! event id. Sampling is deterministic on the event id: the same event is
//! either traced through every stage or through none. One percent by
//! default; everything under trace-level logging.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::cloud_event::CloudEvent;

/// The configured rate, stored as bits so it can be set once at boot and
/// read from every stage without a lock.
static RATE_BITS: AtomicU64 = AtomicU64::new(0x3F847AE147AE147B); // 0.01

/// The configured sample rate. Clamped to [0, 1].
pub fn set_sample_rate(rate: f64) {
    RATE_BITS.store(rate.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

pub fn sample_rate() -> f64 {
    f64::from_bits(RATE_BITS.load(Ordering::Relaxed))
}

/// The rate in force: everything under trace-level logging, else the
/// configured rate clamped to [0, 1].
pub fn effective_rate(configured: f64, trace_logging: bool) -> f64 {
    if trace_logging {
        1.0
    } else {
        configured.clamp(0.0, 1.0)
    }
}

/// Is this event id in the sample at `rate`? FNV-1a over the id, so the
/// answer is the same on every stage and every node.
pub fn sampled(event_id: &str, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in event_id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Ten thousand buckets: 0.01 is exactly one hundred of them.
    let bucket = (h % 10_000) as f64;
    bucket < rate * 10_000.0
}

fn trace_logging() -> bool {
    tracing::level_filters::LevelFilter::current() >= tracing::Level::TRACE
}

/// The stage span for `ce`, when it is in the sample.
pub fn stage_span(
    stage: &'static str,
    ce: &CloudEvent,
    subject: Option<&str>,
    actor: &str,
) -> Option<tracing::Span> {
    let rate = effective_rate(sample_rate(), trace_logging());
    if !sampled(&ce.id, rate) {
        return None;
    }
    Some(tracing::info_span!(
        "event",
        stage,
        event_id = %ce.id,
        session_id = %ce.data.session_id,
        subject = subject.unwrap_or(""),
        actor,
    ))
}

/// Mark one stage for one event: enter its span and leave it.
pub fn mark(stage: &'static str, ce: &CloudEvent, subject: Option<&str>, actor: &str) {
    if let Some(span) = stage_span(stage, ce, subject, actor) {
        let _entered = span.enter();
    }
}

/// Mark one stage for every event in a batch.
pub fn mark_batch(stage: &'static str, events: &[CloudEvent], subject: Option<&str>, actor: &str) {
    for ce in events {
        mark(stage, ce, subject, actor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rate_is_one_percent() {
        assert!((sample_rate() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn set_rate_clamps() {
        set_sample_rate(3.0);
        assert_eq!(sample_rate(), 1.0);
        set_sample_rate(0.01);
    }
}
