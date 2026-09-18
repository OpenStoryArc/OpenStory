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

// ═══════════════════════════════════════════════════════════════════
// X-03: expect — the golden's expected output, by construction
// ═══════════════════════════════════════════════════════════════════

use open_story_patterns::golden::{expect, ArcClose};
use open_story_patterns::story::handle;

fn spec_two_arcs() -> GoldenSpec {
    GoldenSpec {
        session_id: "golden-two-arcs".to_string(),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: 300,
        exchanges: vec![
            exchange(
                ExchangeKind::Human,
                0,
                vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)],
            ),
            exchange(
                ExchangeKind::Human,
                10,
                vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)],
            ),
            exchange(
                ExchangeKind::Human,
                600,
                vec![turn("checked", &["src/b.rs"], &[("Bash", 1)], true)],
            ),
        ],
    }
}

fn spec_ambiguous_seam() -> GoldenSpec {
    GoldenSpec {
        session_id: "golden-ambiguous".to_string(),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: 1800,
        exchanges: vec![
            exchange(
                ExchangeKind::Human,
                0,
                vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)],
            ),
            exchange(
                ExchangeKind::Human,
                5,
                vec![turn("committed", &["src/a.rs"], &[("Bash", 1)], true)],
            ),
            exchange(
                ExchangeKind::Human,
                20,
                vec![turn("explored", &["docs/new.md"], &[("Glob", 2)], true)],
            ),
        ],
    }
}

mod when_expected_is_built {
    use super::*;

    #[test]
    fn injected_exchanges_are_folded_into_their_human_exchange() {
        let spec = spec_with_injected();
        let events = generate(&spec);
        let expected = expect(&spec, &events);

        assert_eq!(
            expected.exchanges.len(),
            2,
            "3 spec exchanges, 1 injected → 2 exchanges"
        );
        let prompts = prompt_indices(&events);
        let first = &expected.exchanges[0];
        assert!(
            first.event_range.0 <= prompts[1] && prompts[1] < first.event_range.1,
            "the injected prompt lies inside the first exchange's event range"
        );
        assert_eq!(expected.exchanges[1].event_range.0, prompts[2]);
        assert_eq!(first.event_range.1, prompts[2], "ranges tile the stream");
        assert_eq!(first.tools.get("Skill"), Some(&1));
        assert_eq!(
            first.tools.get("Bash"),
            Some(&2),
            "injected turn's tools fold in"
        );
        assert_eq!(first.entities.get("src/lib.rs"), Some(&1));
        assert_eq!(first.sentence_count, 2);
        assert_eq!(expected.arcs.len(), 1);
        assert_eq!(
            expected.arcs[0].exchanges,
            vec![first.handle.clone(), expected.exchanges[1].handle.clone()]
        );
    }

    #[test]
    fn entities_and_tools_match_the_spec() {
        let spec = spec_two_arcs();
        let expected = expect(&spec, &generate(&spec));
        let arc0 = &expected.arcs[0];
        assert_eq!(arc0.tools.get("Read"), Some(&1));
        assert_eq!(arc0.tools.get("Edit"), Some(&1));
        assert_eq!(
            arc0.entities.get("src/a.rs"),
            Some(&2),
            "same object across two exchanges counts twice"
        );
        assert_eq!(expected.arcs[1].entities.get("src/b.rs"), Some(&1));
    }
}

mod when_a_gap_exceeds_the_threshold {
    use super::*;

    #[test]
    fn expected_opens_a_new_arc() {
        let spec = spec_two_arcs();
        let expected = expect(&spec, &generate(&spec));
        assert_eq!(expected.arcs.len(), 2);
        assert_eq!(expected.arcs[0].exchange_range, (0, 2));
        assert_eq!(expected.arcs[1].exchange_range, (2, 3));
        assert_eq!(expected.arcs[0].closed_by, ArcClose::Gap);
        assert_eq!(expected.arcs[1].closed_by, ArcClose::EndOfStream);
        assert_ne!(expected.arcs[0].handle, expected.arcs[1].handle);
        assert!(expected.arcs.iter().all(|a| a.ambiguous_seams.is_empty()));
    }
}

mod when_closure_verb_precedes_opening_verb_within_gap {
    use super::*;

    #[test]
    fn expected_marks_the_seam_ambiguous_without_splitting() {
        let spec = spec_ambiguous_seam();
        let expected = expect(&spec, &generate(&spec));
        assert_eq!(
            expected.arcs.len(),
            1,
            "no split: the gap is under the threshold"
        );
        assert_eq!(
            expected.arcs[0].ambiguous_seams,
            vec![2],
            "seam before exchange 2 (committed → explored, new entity)"
        );
    }
}

mod when_handles_are_computed {
    use super::*;

    #[test]
    fn they_are_sixteen_hex_and_order_independent() {
        let a = handle(&["e2".to_string(), "e1".to_string()]);
        let b = handle(&["e1".to_string(), "e2".to_string()]);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, handle(&["e1".to_string()]));
    }

    #[test]
    fn expected_handles_are_content_addressed_from_event_ids() {
        let spec = spec_two_arcs();
        let events = generate(&spec);
        let expected = expect(&spec, &events);
        let ex0 = &expected.exchanges[0];
        let ids: Vec<String> = events[ex0.event_range.0..ex0.event_range.1]
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(ex0.handle, handle(&ids));
        assert_eq!(ex0.event_ids, ids);
    }
}
