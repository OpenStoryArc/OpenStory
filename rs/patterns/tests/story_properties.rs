//! Property tests for the story fold (memory hands, A-11 and A-13).
//!
//! Random GoldenSpecs go through the real pipeline. The laws: the count
//! monoid is associative with an empty identity; the fold is a pure
//! function of the event stream; every event lands in exactly one
//! exchange and every exchange in exactly one arc; and closing an arc by
//! watermark (a flush while the session is idle) yields the same
//! membership as closing it by the next exchange's arrival.

use open_story_patterns::golden::{
    generate, ExchangeKind, ExchangeSpec, GoldenSpec, PromptClass, TurnSpec,
};
use open_story_patterns::story::{merge_counts, StoryDetector};
use open_story_patterns::{PatternEvent, PatternPipeline};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn counts_strategy() -> impl Strategy<Value = BTreeMap<String, u32>> {
    proptest::collection::btree_map("[a-e]", 0u32..5, 0..4)
}

fn turn_strategy() -> impl Strategy<Value = TurnSpec> {
    (
        prop::sample::select(vec![
            "read",
            "edited",
            "wrote",
            "checked",
            "committed",
            "explored",
            "explained",
        ]),
        proptest::collection::vec(
            prop::sample::select(vec!["src/a.rs", "src/b.rs", "docs/c.md"]),
            0..3,
        ),
        proptest::collection::vec(
            (
                prop::sample::select(vec!["Read", "Edit", "Bash", "Glob", "Skill"]),
                1u32..3,
            ),
            0..3,
        ),
        any::<bool>(),
    )
        .prop_map(|(verb, objects, tools, rich)| TurnSpec {
            verb: verb.to_string(),
            objects: objects.into_iter().map(String::from).collect(),
            tools: tools.into_iter().map(|(n, c)| (n.to_string(), c)).collect(),
            rich,
        })
}

fn exchange_strategy(first: bool) -> impl Strategy<Value = ExchangeSpec> {
    let kind = if first {
        Just(ExchangeKind::Human).boxed()
    } else {
        prop_oneof![3 => Just(ExchangeKind::Human), 1 => Just(ExchangeKind::Injected)].boxed()
    };
    (
        kind,
        0u64..4000,
        proptest::collection::vec(turn_strategy(), 1..3),
    )
        .prop_map(|(kind, gap, turns)| ExchangeSpec {
            kind,
            prompt_class: PromptClass::Short,
            gap_before_secs: gap,
            turns,
        })
}

fn spec_strategy() -> impl Strategy<Value = GoldenSpec> {
    (
        exchange_strategy(true),
        proptest::collection::vec(exchange_strategy(false), 0..7),
    )
        .prop_map(|(first, rest)| {
            let mut exchanges = vec![first];
            exchanges.extend(rest);
            GoldenSpec {
                session_id: "prop-session".to_string(),
                started_at: "2026-01-01T09:00:00Z".to_string(),
                gap_threshold_secs: 1800,
                exchanges,
            }
        })
}

fn fold(spec: &GoldenSpec) -> Vec<PatternEvent> {
    let mut pipeline = PatternPipeline::with_turn_detectors(vec![Box::new(StoryDetector::new(
        spec.gap_threshold_secs,
    ))]);
    let mut out = Vec::new();
    for ev in generate(spec) {
        out.extend(pipeline.feed_event(&ev).0);
    }
    out.extend(pipeline.flush().0);
    out.into_iter()
        .filter(|p| p.pattern_type.starts_with("story."))
        .collect()
}

fn story_only(v: Vec<PatternEvent>) -> Vec<PatternEvent> {
    v.into_iter()
        .filter(|p| p.pattern_type.starts_with("story."))
        .collect()
}

proptest! {
    #[test]
    fn prop_merge_is_associative(a in counts_strategy(), b in counts_strategy(), c in counts_strategy()) {
        let mut ab = a.clone();
        merge_counts(&mut ab, &b);
        let mut ab_c = ab;
        merge_counts(&mut ab_c, &c);
        let mut bc = b.clone();
        merge_counts(&mut bc, &c);
        let mut a_bc = a.clone();
        merge_counts(&mut a_bc, &bc);
        prop_assert_eq!(ab_c, a_bc);
    }

    #[test]
    fn prop_identity_is_noop(a in counts_strategy()) {
        let mut left = BTreeMap::new();
        merge_counts(&mut left, &a);
        let mut right = a.clone();
        merge_counts(&mut right, &BTreeMap::new());
        prop_assert_eq!(&left, &a);
        prop_assert_eq!(&right, &a);
    }

    #[test]
    fn prop_fold_is_deterministic(spec in spec_strategy()) {
        let a = serde_json::to_string(&fold(&spec)).unwrap();
        let b = serde_json::to_string(&fold(&spec)).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn prop_turns_partition(spec in spec_strategy()) {
        let events = generate(&spec);
        let out = fold(&spec);
        let exchanges: Vec<&PatternEvent> = out.iter().filter(|p| p.pattern_type == "story.exchange").collect();
        let arcs: Vec<&PatternEvent> = out.iter().filter(|p| p.pattern_type == "story.arc").collect();

        // Every event id lands in exactly one exchange, in stream order.
        let tiled: Vec<&str> = exchanges.iter().flat_map(|p| p.event_ids.iter().map(String::as_str)).collect();
        let all: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        prop_assert_eq!(tiled, all);

        // Every exchange handle lands in exactly one arc, in order.
        let in_arcs: Vec<String> = arcs.iter().flat_map(|a| {
            a.metadata["exchanges"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect::<Vec<_>>()
        }).collect();
        let handles: Vec<String> = exchanges.iter().map(|p| p.metadata["handle"].as_str().unwrap().to_string()).collect();
        prop_assert_eq!(in_arcs, handles);

        // Exactly one exchange per Human entry: injected entries never open one.
        let humans = spec.exchanges.iter().filter(|e| e.kind == ExchangeKind::Human).count();
        prop_assert_eq!(exchanges.len(), humans);
    }
}

mod when_the_close_comes_from_a_watermark {
    use super::*;

    /// Feed a spec's events, but flush the pipeline at `flush_after` events
    /// (the consumer decided the session went idle), then keep feeding.
    fn fold_with_watermark(spec: &GoldenSpec, flush_after: usize) -> Vec<PatternEvent> {
        let mut pipeline = PatternPipeline::with_turn_detectors(vec![Box::new(
            StoryDetector::new(spec.gap_threshold_secs),
        )]);
        let mut out = Vec::new();
        for (i, ev) in generate(spec).into_iter().enumerate() {
            if i == flush_after {
                out.extend(pipeline.flush().0);
            }
            out.extend(pipeline.feed_event(&ev).0);
        }
        out.extend(pipeline.flush().0);
        story_only(out)
    }

    #[test]
    fn membership_equals_the_next_exchange_case() {
        let spec = GoldenSpec {
            session_id: "watermark".to_string(),
            started_at: "2026-01-01T09:00:00Z".to_string(),
            gap_threshold_secs: 300,
            exchanges: vec![
                ExchangeSpec {
                    kind: ExchangeKind::Human,
                    prompt_class: PromptClass::Short,
                    gap_before_secs: 0,
                    turns: vec![TurnSpec {
                        verb: "read".into(),
                        objects: vec!["src/a.rs".into()],
                        tools: vec![("Read".into(), 1)],
                        rich: true,
                    }],
                },
                ExchangeSpec {
                    kind: ExchangeKind::Human,
                    prompt_class: PromptClass::Short,
                    gap_before_secs: 10,
                    turns: vec![TurnSpec {
                        verb: "edited".into(),
                        objects: vec!["src/a.rs".into()],
                        tools: vec![("Edit".into(), 1)],
                        rich: true,
                    }],
                },
                ExchangeSpec {
                    kind: ExchangeKind::Human,
                    prompt_class: PromptClass::Short,
                    gap_before_secs: 600,
                    turns: vec![TurnSpec {
                        verb: "checked".into(),
                        objects: vec!["src/b.rs".into()],
                        tools: vec![("Bash".into(), 1)],
                        rich: true,
                    }],
                },
            ],
        };
        let events = generate(&spec);
        let third_prompt = events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.subtype.as_deref() == Some("message.user.prompt"))
            .map(|(i, _)| i)
            .nth(2)
            .unwrap();

        let by_gap = fold(&spec);
        let by_watermark = fold_with_watermark(&spec, third_prompt);

        let arcs = |v: &[PatternEvent]| -> Vec<(String, serde_json::Value, String)> {
            v.iter()
                .filter(|p| p.pattern_type == "story.arc")
                .map(|p| {
                    (
                        p.metadata["handle"].as_str().unwrap().to_string(),
                        p.metadata["exchanges"].clone(),
                        p.metadata["closed_by"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        };
        let g = arcs(&by_gap);
        let w = arcs(&by_watermark);
        assert_eq!(g.len(), 2);
        assert_eq!(w.len(), 2);
        for (a, b) in g.iter().zip(&w) {
            assert_eq!(
                a.0, b.0,
                "arc handles agree regardless of how the close was triggered"
            );
            assert_eq!(a.1, b.1, "arc membership agrees");
        }
        assert_eq!(g[0].2, "gap");
        assert_eq!(
            w[0].2, "end_of_stream",
            "the watermark close reports itself honestly"
        );
        let ex = |v: &[PatternEvent]| -> Vec<String> {
            v.iter()
                .filter(|p| p.pattern_type == "story.exchange")
                .map(|p| p.metadata["handle"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(ex(&by_gap), ex(&by_watermark));
    }
}
