//! Story goldens (memory hands, requirements group X).
//!
//! A golden is derived from a `GoldenSpec`, never copied from a real
//! session. These tests pin the pure generator: same spec, same bytes;
//! injected user-role messages arrive mid-turn, human prompts open a turn.

use open_story_core::cloud_event::CloudEvent;
use open_story_patterns::golden::{
    generate, ExchangeKind, ExchangeSpec, GoldenSpec, PromptClass, TurnSpec,
};

fn turn(verb: &str, objects: &[&str], tools: &[(&str, u32)], has_sentence: bool) -> TurnSpec {
    TurnSpec {
        verb: verb.to_string(),
        objects: objects.iter().map(|s| s.to_string()).collect(),
        tools: tools.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        has_sentence,
    }
}

fn exchange(kind: ExchangeKind, gap_before_secs: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    ExchangeSpec {
        kind,
        prompt_class: PromptClass::Short,
        gap_before_secs,
        turns,
    }
}

/// Human, then an injected skill body mid-turn, then a human after a long gap.
fn spec_with_injected() -> GoldenSpec {
    GoldenSpec {
        session_id: "golden-injected".to_string(),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: 1800,
        exchanges: vec![
            exchange(
                ExchangeKind::Human,
                0,
                vec![turn("read", &["src/lib.rs"], &[("Skill", 1)], true)],
            ),
            exchange(
                ExchangeKind::Injected,
                2,
                vec![turn("checked", &[], &[("Bash", 2)], true)],
            ),
            exchange(
                ExchangeKind::Human,
                600,
                vec![turn("edited", &["src/lib.rs"], &[("Edit", 1)], true)],
            ),
        ],
    }
}

fn subtype(ev: &CloudEvent) -> &str {
    ev.subtype.as_deref().unwrap_or("")
}

fn secs(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .expect("rfc3339 time")
        .timestamp()
}

/// True when an assistant event sits between the most recent turn close
/// (or the stream start) and index `i` — the signature of a mid-turn arrival.
fn arrived_mid_turn(events: &[CloudEvent], i: usize) -> bool {
    events[..i]
        .iter()
        .rev()
        .take_while(|ev| subtype(ev) != "system.turn.complete")
        .any(|ev| subtype(ev).starts_with("message.assistant"))
}

fn prompt_indices(events: &[CloudEvent]) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter(|(_, ev)| subtype(ev) == "message.user.prompt")
        .map(|(i, _)| i)
        .collect()
}

mod when_generate_runs_twice_on_one_spec {
    use super::*;

    #[test]
    fn the_event_streams_are_identical() {
        let spec = spec_with_injected();
        let a = serde_json::to_string(&generate(&spec)).unwrap();
        let b = serde_json::to_string(&generate(&spec)).unwrap();
        assert_eq!(a, b, "generate must be a pure function of the spec");
        let events = generate(&spec);
        let mut ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), events.len(), "event ids must be unique");
    }
}

mod when_a_spec_has_an_injected_exchange {
    use super::*;

    #[test]
    fn its_user_message_arrives_before_the_turn_closes() {
        let events = generate(&spec_with_injected());
        let prompts = prompt_indices(&events);
        assert_eq!(prompts.len(), 3, "one user prompt per exchange");
        assert!(
            !arrived_mid_turn(&events, prompts[0]),
            "first human prompt opens the stream"
        );
        assert!(
            arrived_mid_turn(&events, prompts[1]),
            "injected prompt arrives while a turn is open"
        );
        assert!(
            !arrived_mid_turn(&events, prompts[2]),
            "human prompt follows a closed turn"
        );
        assert_eq!(
            subtype(events.last().unwrap()),
            "system.turn.complete",
            "stream ends closed"
        );
    }
}

mod when_generate_lays_out_time {
    use super::*;

    #[test]
    fn events_start_at_started_at_and_honor_gap_before() {
        let spec = spec_with_injected();
        let events = generate(&spec);
        assert_eq!(secs(&events[0].time), secs(&spec.started_at));
        let times: Vec<i64> = events.iter().map(|e| secs(&e.time)).collect();
        assert!(
            times.windows(2).all(|w| w[0] <= w[1]),
            "event time never goes backwards"
        );
        let third = prompt_indices(&events)[2];
        assert!(
            times[third] - times[third - 1] >= 600,
            "gap_before_secs is honored in event time: {} vs {}",
            times[third],
            times[third - 1]
        );
    }
}
