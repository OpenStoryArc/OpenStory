//! Property-based test generators for CloudEvents.
//!
//! This module is the foundation for commit 1.4 of the TDD plan at
//! `/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`:
//! proving that `BroadcastConsumer::process_batch` emits the same
//! `BroadcastMessage`s as `ingest_events` for any input batch.
//!
//! Today the generator exists and a trivial identity property holds.
//! At commit 1.4 the assertion flips to a real equivalence check:
//!
//!   proptest! {
//!     fn broadcast_outputs_equivalent(events in vec(any_cloud_event(), 1..20)) {
//!       let via_ingest = ingest_events(...).changes;
//!       let via_broadcast = broadcast.process_batch(..).
//!       prop_assert_eq!(canonicalize(via_ingest), canonicalize(via_broadcast));
//!     }
//!   }
//!
//! Growing the generator's coverage is a first-class part of Phase 0 —
//! as new CloudEvent subtypes or payload shapes appear, extend
//! `any_cloud_event` to emit them.

#![cfg(test)]

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData, PiMonoPayload};
use proptest::prelude::*;

/// All known CloudEvent subtypes (excluding those that require specialized
/// payload shapes). Grow this list as the translator emits new subtypes.
fn any_subtype() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("message.user.prompt"),
        Just("message.user.tool_result"),
        Just("message.assistant.text"),
        Just("message.assistant.tool_use"),
        Just("message.assistant.thinking"),
        Just("system.turn.complete"),
        Just("system.error"),
        Just("system.session_start"),
        Just("system.model_change"),
        Just("file.snapshot"),
    ]
    .prop_map(|s| s.to_string())
}

fn any_agent() -> impl Strategy<Value = String> {
    prop_oneof![Just("claude-code"), Just("pi-mono"), Just("hermes")].prop_map(|s| s.to_string())
}

fn any_short_text() -> impl Strategy<Value = String> {
    // Keep strings small so counterexamples are readable.
    "[a-zA-Z0-9 _-]{0,40}".prop_map(|s| s.to_string())
}

fn any_session_id() -> impl Strategy<Value = String> {
    "[a-z0-9-]{6,20}".prop_map(|s| s.to_string())
}

fn any_uuid() -> impl Strategy<Value = String> {
    // 8-4-4-4-12 lowercase hex. Not strictly v4-compliant but accepted by
    // the pipeline (UUIDs are opaque identifiers to the ingester).
    "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}".prop_map(|s| s.to_string())
}

fn any_claude_payload() -> impl Strategy<Value = ClaudeCodePayload> {
    any_short_text().prop_map(|text| {
        let mut p = ClaudeCodePayload::new();
        p.text = Some(text);
        p
    })
}

fn any_pi_payload() -> impl Strategy<Value = PiMonoPayload> {
    any_short_text().prop_map(|text| {
        let mut p = PiMonoPayload::new();
        p.text = Some(text);
        p
    })
}

fn any_agent_payload(agent: &str) -> BoxedStrategy<AgentPayload> {
    match agent {
        "pi-mono" => any_pi_payload().prop_map(AgentPayload::PiMono).boxed(),
        _ => any_claude_payload().prop_map(AgentPayload::ClaudeCode).boxed(),
    }
}

/// Generate a valid CloudEvent covering the subtypes that flow through
/// `ingest_events`. Intentionally narrow — grow as coverage gaps surface.
pub fn any_cloud_event() -> impl Strategy<Value = CloudEvent> {
    (
        any_session_id(),
        any_subtype(),
        any_agent(),
        any_uuid(),
        any::<u64>(),
    )
        .prop_flat_map(|(sid, subtype, agent, uuid, seq)| {
            any_agent_payload(&agent).prop_map(move |ap| {
                let data = EventData::with_payload(
                    serde_json::json!({}),
                    seq,
                    sid.clone(),
                    ap,
                );
                CloudEvent::new(
                    format!("arc://transcript/{sid}"),
                    "io.arc.event".to_string(),
                    data,
                    Some(subtype.clone()),
                    Some(uuid.clone()),
                    Some("2025-01-01T00:00:00Z".to_string()),
                    None,
                    None,
                    Some(agent.clone()),
                )
            })
        })
}

/// Generate a batch of CloudEvents with a consistent session_id across
/// the batch (mirrors what the watcher actually delivers).
pub fn any_batch(size: std::ops::RangeInclusive<usize>) -> impl Strategy<Value = Vec<CloudEvent>> {
    (any_session_id(), size).prop_flat_map(|(sid, n)| {
        proptest::collection::vec(any_cloud_event(), n).prop_map(move |mut events| {
            for (i, e) in events.iter_mut().enumerate() {
                e.data.session_id = sid.clone();
                e.data.seq = i as u64;
            }
            events
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        /// Stub property (commit 0c). Will be replaced at commit 1.4 with
        /// the real equivalence oracle between `ingest_events` and
        /// `BroadcastConsumer::process_batch`. For now we prove the
        /// generator produces well-formed CloudEvents.
        #[test]
        fn generator_produces_valid_cloudevents(ce in any_cloud_event()) {
            prop_assert!(!ce.source.is_empty());
            prop_assert_eq!(&ce.event_type, "io.arc.event");
            prop_assert!(ce.subtype.is_some());
            prop_assert!(!ce.id.is_empty());
            prop_assert!(!ce.data.session_id.is_empty());
            // Round-trip through serde — the ingest path assumes this works.
            let v = serde_json::to_value(&ce).expect("serialize");
            let _back: CloudEvent = serde_json::from_value(v).expect("deserialize");
        }

        #[test]
        fn batches_have_consistent_session_id(events in any_batch(1..=10)) {
            let first = events[0].data.session_id.clone();
            for e in &events {
                prop_assert_eq!(&e.data.session_id, &first);
            }
        }
    }
}
