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

// ═══════════════════════════════════════════════════════════════════
// X-04: six committed goldens, regenerated by
//   cargo test -p open-story-patterns --test story_goldens -- --ignored regen
// ═══════════════════════════════════════════════════════════════════

use open_story_patterns::golden::Expected;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("story")
}

fn human(gap: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    exchange(ExchangeKind::Human, gap, turns)
}

fn injected(gap: u64, class: PromptClass, turns: Vec<TurnSpec>) -> ExchangeSpec {
    ExchangeSpec {
        kind: ExchangeKind::Injected,
        prompt_class: class,
        gap_before_secs: gap,
        turns,
    }
}

fn spec(name: &str, threshold: u64, exchanges: Vec<ExchangeSpec>) -> GoldenSpec {
    GoldenSpec {
        session_id: format!("golden-{name}"),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: threshold,
        exchanges,
    }
}

/// The committed golden set. Names are the fixture directory names.
/// `long_session_shape` copies only shape parameters from the dogfood
/// session b0a56730 (16 user-role messages, 4 injected, largest gap
/// 14 minutes, verbs mostly checked / explained / wrote); no content.
fn golden_specs() -> Vec<(&'static str, GoldenSpec)> {
    vec![
        (
            "single_arc_plain",
            spec(
                "single_arc_plain",
                1800,
                vec![
                    human(
                        0,
                        vec![turn(
                            "read",
                            &["src/lib.rs", "src/main.rs"],
                            &[("Read", 2)],
                            true,
                        )],
                    ),
                    human(
                        45,
                        vec![turn(
                            "edited",
                            &["src/lib.rs"],
                            &[("Edit", 1), ("Bash", 1)],
                            true,
                        )],
                    ),
                    human(
                        120,
                        vec![turn("ran tests", &["tests/lib.rs"], &[("Bash", 3)], true)],
                    ),
                    human(
                        30,
                        vec![turn("committed", &["src/lib.rs"], &[("Bash", 2)], true)],
                    ),
                ],
            ),
        ),
        (
            "two_arcs_gap",
            spec(
                "two_arcs_gap",
                1800,
                vec![
                    human(
                        0,
                        vec![turn("read", &["docs/plan.md"], &[("Read", 1)], true)],
                    ),
                    human(
                        60,
                        vec![turn("wrote", &["docs/plan.md"], &[("Write", 1)], true)],
                    ),
                    human(
                        2400,
                        vec![turn("checked", &["src/app.rs"], &[("Bash", 2)], true)],
                    ),
                    human(
                        15,
                        vec![turn("edited", &["src/app.rs"], &[("Edit", 2)], true)],
                    ),
                ],
            ),
        ),
        (
            "thin_turn_only",
            spec(
                "thin_turn_only",
                1800,
                vec![human(0, vec![turn("explained", &[], &[], false)])],
            ),
        ),
        (
            "ambiguous_closure_opening",
            spec(
                "ambiguous_closure_opening",
                1800,
                vec![
                    human(0, vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)]),
                    human(
                        20,
                        vec![turn("committed", &["src/a.rs"], &[("Bash", 1)], true)],
                    ),
                    human(
                        90,
                        vec![turn(
                            "explored",
                            &["docs/next.md"],
                            &[("Glob", 2), ("Read", 1)],
                            true,
                        )],
                    ),
                    human(
                        40,
                        vec![turn("wrote", &["docs/next.md"], &[("Write", 1)], true)],
                    ),
                ],
            ),
        ),
        (
            "injected_skill_messages",
            spec(
                "injected_skill_messages",
                1800,
                vec![
                    human(
                        0,
                        vec![turn("read", &["src/lib.rs"], &[("Skill", 1)], true)],
                    ),
                    injected(
                        1,
                        PromptClass::Long,
                        vec![turn("checked", &["src/lib.rs"], &[("Bash", 4)], true)],
                    ),
                    injected(
                        0,
                        PromptClass::Short,
                        vec![turn("explained", &[], &[], false)],
                    ),
                    human(
                        300,
                        vec![turn("wrote", &["docs/design.md"], &[("Skill", 1)], true)],
                    ),
                    injected(
                        2,
                        PromptClass::Long,
                        vec![turn("wrote", &["docs/design.md"], &[("Write", 2)], true)],
                    ),
                    human(
                        600,
                        vec![turn("checked", &["docs/design.md"], &[("Bash", 1)], true)],
                    ),
                ],
            ),
        ),
        (
            "long_session_shape",
            spec(
                "long_session_shape",
                1800,
                vec![
                    human(0, vec![turn("explained", &[], &[], false)]),
                    injected(
                        4,
                        PromptClass::Long,
                        vec![turn("checked", &["notes/a.md"], &[("Bash", 5)], true)],
                    ),
                    human(
                        138,
                        vec![turn(
                            "checked",
                            &["notes/a.md", "scripts/x.py"],
                            &[("Bash", 4)],
                            true,
                        )],
                    ),
                    human(
                        150,
                        vec![turn("checked", &["notes/b.md"], &[("Bash", 8)], true)],
                    ),
                    human(318, vec![turn("explained", &[], &[], true)]),
                    human(
                        48,
                        vec![turn("checked", &["docs/c.md"], &[("Bash", 6)], true)],
                    ),
                    human(858, vec![turn("explained", &[], &[], false)]),
                    injected(
                        0,
                        PromptClass::Long,
                        vec![turn("explained", &[], &[], false)],
                    ),
                    injected(
                        0,
                        PromptClass::Long,
                        vec![turn(
                            "wrote",
                            &["out/page.html", "notes/d.md"],
                            &[("Write", 4), ("Bash", 2)],
                            true,
                        )],
                    ),
                    human(330, vec![turn("explained", &[], &[], false)]),
                    injected(
                        0,
                        PromptClass::Long,
                        vec![turn(
                            "wrote",
                            &["notes/e.md"],
                            &[("Bash", 8), ("Write", 1)],
                            true,
                        )],
                    ),
                    human(
                        510,
                        vec![turn(
                            "wrote",
                            &["notes/f.md", "notes/g.md"],
                            &[("Write", 4), ("Bash", 2)],
                            true,
                        )],
                    ),
                    human(
                        762,
                        vec![turn("checked", &["notes/f.md"], &[("Bash", 1)], true)],
                    ),
                    human(174, vec![turn("explained", &[], &[], true)]),
                    human(198, vec![turn("explained", &[], &[], true)]),
                    human(
                        108,
                        vec![turn("checked", &["scripts/y.py"], &[("Bash", 4)], true)],
                    ),
                ],
            ),
        ),
    ]
}

/// Pure: the three committed artifacts for one spec, as bytes.
fn render(spec: &GoldenSpec) -> (String, String, String) {
    let events = generate(spec);
    let expected: Expected = expect(spec, &events);
    let spec_json = format!("{}\n", serde_json::to_string_pretty(spec).unwrap());
    let events_jsonl = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let expected_json = format!("{}\n", serde_json::to_string_pretty(&expected).unwrap());
    (spec_json, events_jsonl, expected_json)
}

mod when_regen_runs {
    use super::*;

    #[test]
    fn committed_fixtures_are_byte_identical() {
        let dir = fixtures_dir();
        let hint =
            "regenerate: cargo test -p open-story-patterns --test story_goldens -- --ignored regen";
        for (name, spec) in golden_specs() {
            let (spec_json, events_jsonl, expected_json) = render(&spec);
            let d = dir.join(name);
            for (file, fresh) in [
                ("spec.json", spec_json),
                ("events.jsonl", events_jsonl),
                ("expected.json", expected_json),
            ] {
                let committed = std::fs::read_to_string(d.join(file))
                    .unwrap_or_else(|e| panic!("{name}/{file} missing ({e}); {hint}"));
                assert_eq!(committed, fresh, "{name}/{file} drifted; {hint}");
            }
        }
    }

    #[test]
    fn the_six_goldens_exist() {
        let names: Vec<&str> = golden_specs().iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), 6);
        for name in names {
            assert!(
                fixtures_dir().join(name).join("expected.json").is_file(),
                "{name} is committed"
            );
        }
    }

    /// Writes the fixtures. Ignored so it never runs in CI by accident.
    #[test]
    #[ignore]
    fn regen() {
        for (name, spec) in golden_specs() {
            let (spec_json, events_jsonl, expected_json) = render(&spec);
            let d = fixtures_dir().join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("spec.json"), spec_json).unwrap();
            std::fs::write(d.join("events.jsonl"), events_jsonl).unwrap();
            std::fs::write(d.join("expected.json"), expected_json).unwrap();
            eprintln!("wrote {}", d.display());
        }
    }
}
