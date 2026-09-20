//! StoryDetector behaviour (memory hands, requirements group A).
//!
//! Every test feeds CloudEvents through the real PatternPipeline and
//! asserts on the story.exchange / story.arc patterns that come out.

use open_story_patterns::golden::{
    generate, ExchangeKind, ExchangeSpec, GoldenSpec, PromptClass, TurnSpec,
};
use open_story_patterns::story::StoryDetector;
use open_story_patterns::{PatternEvent, PatternPipeline, TurnDetector};

#[allow(dead_code)]
fn turn(verb: &str, objects: &[&str], tools: &[(&str, u32)], rich: bool) -> TurnSpec {
    TurnSpec {
        verb: verb.to_string(),
        objects: objects.iter().map(|s| s.to_string()).collect(),
        tools: tools.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        rich,
    }
}

#[allow(dead_code)]
fn exchange(kind: ExchangeKind, gap_before_secs: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    ExchangeSpec {
        kind,
        prompt_class: PromptClass::Short,
        gap_before_secs,
        turns,
    }
}

#[allow(dead_code)]
fn spec(name: &str, threshold: u64, exchanges: Vec<ExchangeSpec>) -> GoldenSpec {
    GoldenSpec {
        session_id: format!("story-{name}"),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: threshold,
        exchanges,
    }
}

/// Run a spec through the full pipeline (eval-apply → sentence → story)
/// and return only the story patterns, in emission order.
#[allow(dead_code)]
fn run(spec: &GoldenSpec, pipeline: &mut PatternPipeline) -> Vec<PatternEvent> {
    let mut out = Vec::new();
    for ev in generate(spec) {
        let (patterns, _turns) = pipeline.feed_event(&ev);
        out.extend(patterns);
    }
    let (patterns, _turns) = pipeline.flush();
    out.extend(patterns);
    out.into_iter()
        .filter(|p| p.pattern_type.starts_with("story."))
        .collect()
}

mod when_pipeline_is_default {
    use super::*;

    #[test]
    fn it_includes_story_detector() {
        let names: Vec<String> = PatternPipeline::new()
            .turn_detector_names()
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(names, vec!["sentence".to_string(), "story".to_string()]);
    }

    #[test]
    fn a_story_detector_can_be_constructed_alone() {
        let det = StoryDetector::new(1800);
        assert_eq!(det.name(), "story");
        let mut pipeline = PatternPipeline::with_turn_detectors(vec![Box::new(det)]);
        let s = spec("alone", 1800, vec![]);
        assert!(
            run(&s, &mut pipeline).is_empty(),
            "no events, no story patterns"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// A-03 … A-10: the fold
// ═══════════════════════════════════════════════════════════════════

use open_story_patterns::golden::{expect, Expected};
use open_story_patterns::story::handle;
use std::path::PathBuf;

fn h(gap: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    exchange(ExchangeKind::Human, gap, turns)
}

fn inj(gap: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    exchange(ExchangeKind::Injected, gap, turns)
}

fn fold(spec: &GoldenSpec) -> Vec<PatternEvent> {
    let mut pipeline = PatternPipeline::with_turn_detectors(vec![Box::new(StoryDetector::new(
        spec.gap_threshold_secs,
    ))]);
    run(spec, &mut pipeline)
}

fn exchanges(patterns: &[PatternEvent]) -> Vec<&PatternEvent> {
    patterns
        .iter()
        .filter(|p| p.pattern_type == "story.exchange")
        .collect()
}

fn arcs(patterns: &[PatternEvent]) -> Vec<&PatternEvent> {
    patterns
        .iter()
        .filter(|p| p.pattern_type == "story.arc")
        .collect()
}

fn counts(v: &serde_json::Value) -> std::collections::BTreeMap<String, u64> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

mod when_a_human_prompt_arrives {
    use super::*;

    #[test]
    fn it_closes_the_exchange_and_opens_the_next() {
        let s = spec(
            "two_humans",
            1800,
            vec![
                h(0, vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)]),
                h(
                    30,
                    vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)],
                ),
            ],
        );
        let out = fold(&s);
        let ex = exchanges(&out);
        assert_eq!(ex.len(), 2);
        assert!(ex[0].metadata["user_prompt"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 0"));
        assert!(ex[1].metadata["user_prompt"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 1"));
        let all: Vec<String> = generate(&s).iter().map(|e| e.id.clone()).collect();
        let joined: Vec<String> = ex.iter().flat_map(|p| p.event_ids.clone()).collect();
        assert_eq!(joined, all, "exchanges tile the stream in order");
    }
}

mod when_an_injected_user_role_message_arrives {
    use super::*;

    #[test]
    fn it_does_not_open_an_exchange() {
        let s = spec(
            "injected",
            1800,
            vec![
                h(0, vec![turn("read", &["src/a.rs"], &[("Skill", 1)], true)]),
                inj(
                    1,
                    vec![turn("checked", &["src/a.rs"], &[("Bash", 2)], true)],
                ),
                h(
                    60,
                    vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)],
                ),
            ],
        );
        let out = fold(&s);
        let ex = exchanges(&out);
        assert_eq!(
            ex.len(),
            2,
            "the injected prompt folds into the open exchange"
        );
        assert!(ex[0].metadata["user_prompt"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 0"));
        assert_eq!(ex[0].metadata["injected_count"], 1);
        assert_eq!(counts(&ex[0].metadata["tools"]).get("Bash"), Some(&2));
    }
}

mod when_a_turn_has_no_applies {
    use super::*;

    #[test]
    fn it_folds_as_a_thin_turn() {
        let s = spec(
            "thin",
            1800,
            vec![h(
                0,
                vec![
                    turn("read", &["src/a.rs"], &[("Read", 1)], true),
                    turn("explained", &[], &[], false),
                ],
            )],
        );
        let out = fold(&s);
        let ex = exchanges(&out);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].metadata["turns"], 2);
        assert_eq!(ex[0].metadata["rich_turns"], 1);
        assert_eq!(
            counts(&ex[0].metadata["entities"]).len(),
            1,
            "the thin turn adds no entities"
        );
        assert_eq!(
            ex[0].event_ids.len(),
            generate(&s).len(),
            "the thin turn's events still belong to the exchange"
        );
    }
}

mod when_a_turn_applies_tools {
    use super::*;

    #[test]
    fn it_adds_entity_inputs_and_counts_tools_by_name_and_role() {
        let s = spec(
            "tools",
            1800,
            vec![h(
                0,
                vec![turn(
                    "edited",
                    &["src/a.rs", "src/b.rs"],
                    &[("Read", 2), ("Edit", 1), ("Bash", 1)],
                    true,
                )],
            )],
        );
        let out = fold(&s);
        let ex = exchanges(&out);
        let entities = counts(&ex[0].metadata["entities"]);
        assert_eq!(
            entities.get("src/a.rs"),
            Some(&2),
            "Read a, then Edit a (objects cycle)"
        );
        assert_eq!(entities.get("src/b.rs"), Some(&1));
        assert!(
            !entities.keys().any(|k| k.starts_with("ls ")),
            "bash commands are not entities"
        );
        let tools = counts(&ex[0].metadata["tools"]);
        assert_eq!(tools.get("Read"), Some(&2));
        assert_eq!(tools.get("Edit"), Some(&1));
        assert_eq!(tools.get("Bash"), Some(&1));
        let by_role = counts(&ex[0].metadata["tools_by_role"]);
        assert_eq!(by_role.get("Preparatory"), Some(&2));
        assert_eq!(by_role.get("Creative"), Some(&1));
        assert_eq!(by_role.get("Verificatory"), Some(&1));
    }
}

mod when_thirty_minutes_pass_between_exchanges {
    use super::*;

    #[test]
    fn it_closes_the_arc() {
        let s = spec(
            "gap",
            300,
            vec![
                h(0, vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)]),
                h(
                    10,
                    vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)],
                ),
                h(
                    600,
                    vec![turn("checked", &["src/b.rs"], &[("Bash", 1)], true)],
                ),
            ],
        );
        let out = fold(&s);
        let ex = exchanges(&out);
        let ar = arcs(&out);
        assert_eq!(ar.len(), 2);
        assert_eq!(ar[0].metadata["closed_by"], "gap");
        assert_eq!(ar[1].metadata["closed_by"], "end_of_stream");
        let first_two: Vec<String> = ex[..2]
            .iter()
            .map(|p| p.metadata["handle"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ar[0].metadata["exchanges"], serde_json::json!(first_two));
    }
}

mod when_the_stream_ends {
    use super::*;

    #[test]
    fn it_flushes_exchange_and_arc() {
        let s = spec(
            "flush",
            1800,
            vec![h(
                0,
                vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)],
            )],
        );
        let mut pipeline =
            PatternPipeline::with_turn_detectors(vec![Box::new(StoryDetector::new(1800))]);
        let mut before_flush = Vec::new();
        for ev in generate(&s) {
            before_flush.extend(pipeline.feed_event(&ev).0);
        }
        assert!(
            arcs(&before_flush).is_empty(),
            "the open arc is not emitted before flush"
        );
        let (flushed, _) = pipeline.flush();
        assert_eq!(exchanges(&flushed).len(), 1);
        assert_eq!(arcs(&flushed).len(), 1);
    }
}

mod when_an_arc_is_emitted {
    use super::*;

    #[test]
    fn it_carries_handle_exchange_handles_and_boundary() {
        let s = spec(
            "shape",
            1800,
            vec![
                h(0, vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)]),
                h(5, vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)]),
            ],
        );
        let out = fold(&s);
        let ar = arcs(&out);
        let handle_s = ar[0].metadata["handle"].as_str().unwrap();
        assert_eq!(handle_s.len(), 16);
        assert!(handle_s.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(handle_s, handle("arc", &ar[0].event_ids));
        let ex_handles: Vec<String> = exchanges(&out)
            .iter()
            .map(|p| p.metadata["handle"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ar[0].metadata["exchanges"], serde_json::json!(ex_handles));
        assert_eq!(ar[0].metadata["closed_by"], "end_of_stream");
        assert_eq!(ar[0].metadata["ambiguous_seams"], serde_json::json!([]));
        assert!(ar[0].metadata["question"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 0"));
    }
}

mod when_an_exchange_closes {
    use super::*;

    #[test]
    fn its_handle_is_final_before_the_arc_closes() {
        let s = spec(
            "order",
            1800,
            vec![
                h(0, vec![turn("read", &["src/a.rs"], &[("Read", 1)], true)]),
                h(5, vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)]),
            ],
        );
        let out = fold(&s);
        let first_exchange = out
            .iter()
            .position(|p| p.pattern_type == "story.exchange")
            .unwrap();
        let first_arc = out
            .iter()
            .position(|p| p.pattern_type == "story.arc")
            .unwrap();
        assert!(first_exchange < first_arc);
        let ex = &out[first_exchange];
        assert_eq!(
            ex.metadata["handle"].as_str().unwrap(),
            handle("exchange", &ex.event_ids)
        );
    }
}

mod when_closure_verb_precedes_opening_verb_within_gap {
    use super::*;

    #[test]
    fn it_marks_boundary_ambiguous_without_splitting() {
        let s = spec(
            "seam",
            1800,
            vec![
                h(0, vec![turn("edited", &["src/a.rs"], &[("Edit", 1)], true)]),
                h(
                    20,
                    vec![turn("committed", &["src/a.rs"], &[("Bash", 1)], true)],
                ),
                h(
                    90,
                    vec![turn("explored", &["docs/next.md"], &[("Glob", 2)], true)],
                ),
            ],
        );
        let out = fold(&s);
        let ar = arcs(&out);
        assert_eq!(ar.len(), 1, "no split under the threshold");
        assert_eq!(ar[0].metadata["ambiguous_seams"], serde_json::json!([2]));
    }
}

mod when_a_golden_is_folded {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("story")
    }

    #[test]
    fn it_matches_expected() {
        let mut checked = 0;
        for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
            let dir = entry.unwrap().path();
            let name = dir.file_name().unwrap().to_string_lossy().to_string();
            let spec: GoldenSpec =
                serde_json::from_str(&std::fs::read_to_string(dir.join("spec.json")).unwrap())
                    .unwrap();
            let expected: Expected =
                serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap())
                    .unwrap();
            let fresh = expect(&spec, &generate(&spec));
            assert_eq!(fresh, expected, "{name}: expected.json is stale");
            let out = fold(&spec);
            let ex = exchanges(&out);
            assert_eq!(ex.len(), expected.exchanges.len(), "{name}: exchange count");
            for (i, (got, want)) in ex.iter().zip(&expected.exchanges).enumerate() {
                assert_eq!(
                    got.event_ids, want.event_ids,
                    "{name}: exchange {i} event ids"
                );
                assert_eq!(
                    got.metadata["handle"], want.handle,
                    "{name}: exchange {i} handle"
                );
                assert_eq!(
                    counts(&got.metadata["entities"]),
                    want.entities
                        .iter()
                        .map(|(k, v)| (k.clone(), *v as u64))
                        .collect(),
                    "{name}: exchange {i} entities"
                );
                assert_eq!(
                    counts(&got.metadata["tools"]),
                    want.tools
                        .iter()
                        .map(|(k, v)| (k.clone(), *v as u64))
                        .collect(),
                    "{name}: exchange {i} tools"
                );
                assert_eq!(
                    got.metadata["rich_turns"], want.rich_turns,
                    "{name}: exchange {i} rich_turns"
                );
            }
            let ar = arcs(&out);
            assert_eq!(ar.len(), expected.arcs.len(), "{name}: arc count");
            for (i, (got, want)) in ar.iter().zip(&expected.arcs).enumerate() {
                assert_eq!(
                    got.metadata["handle"], want.handle,
                    "{name}: arc {i} handle"
                );
                assert_eq!(
                    got.metadata["exchanges"],
                    serde_json::json!(want.exchanges),
                    "{name}: arc {i} exchanges"
                );
                assert_eq!(
                    got.metadata["closed_by"],
                    serde_json::to_value(want.closed_by).unwrap(),
                    "{name}: arc {i} closed_by"
                );
                assert_eq!(
                    got.metadata["ambiguous_seams"],
                    serde_json::json!(want.ambiguous_seams),
                    "{name}: arc {i} seams"
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 6);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Text the fold carries is whole: the exchange's outcome and the arc's
// resolution are the assistant's words, not a 400-byte clip of them.
// Found on the pilot: the cause of a blank PDF sat past the clip.
// ═══════════════════════════════════════════════════════════════════

mod when_an_outcome_is_long {
    use super::*;

    #[test]
    fn it_is_kept_whole_in_the_exchange_and_the_arc() {
        let objects: Vec<String> = (0..40)
            .map(|i| format!("src/module_{i:02}/file.rs"))
            .collect();
        let refs: Vec<&str> = objects.iter().map(|s| s.as_str()).collect();
        let s = spec(
            "long-outcome",
            1800,
            vec![exchange(
                ExchangeKind::Human,
                0,
                vec![turn("read", &refs, &[("Read", 40)], true)],
            )],
        );
        let full = format!("Claude read {}", objects.join(", "));
        assert!(full.len() > 400, "the fixture must exceed the old clip");

        let mut pipeline = PatternPipeline::new();
        let out = run(&s, &mut pipeline);
        let ex = out
            .iter()
            .find(|p| p.pattern_type == "story.exchange")
            .unwrap();
        assert_eq!(ex.metadata["eval_result"], full, "outcome kept whole");
        let arc = out.iter().find(|p| p.pattern_type == "story.arc").unwrap();
        assert_eq!(arc.metadata["resolution"], full, "resolution kept whole");
    }
}
