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
