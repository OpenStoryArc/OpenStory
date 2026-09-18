//! `story_backfill` — fold every stored session through the story detector
//! and report what the fold sees (memory hands, requirement A-12).
//!
//! Read-only over events: it reads `session_events`, replays them through
//! a fresh `PatternPipeline` holding only the `StoryDetector`, and counts.
//! Writing the resulting `story.exchange` / `story.arc` patterns back is a
//! separate, explicit step (`write: true`) so the report can be run against
//! a live store with no side effects. Like `reproject`, this is one of the
//! explicit state-management operations.

use anyhow::Result;
use open_story_core::cloud_event::CloudEvent;
use open_story_patterns::story::StoryDetector;
use open_story_patterns::{PatternEvent, PatternPipeline};
use open_story_store::event_store::EventStore;

/// What the fold saw across a store.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoryReport {
    pub sessions: usize,
    pub sessions_without_events: usize,
    pub exchanges: usize,
    pub arcs: usize,
    pub ambiguous_arcs: usize,
    pub gap_closed_arcs: usize,
    pub patterns_written: usize,
}

impl StoryReport {
    /// Share of arcs carrying at least one ambiguous seam. The number that
    /// should fall as deterministic heuristics absorb host verdicts.
    pub fn ambiguous_share(&self) -> f64 {
        if self.arcs == 0 {
            0.0
        } else {
            self.ambiguous_arcs as f64 / self.arcs as f64
        }
    }

    pub fn exchanges_per_arc(&self) -> f64 {
        if self.arcs == 0 {
            0.0
        } else {
            self.exchanges as f64 / self.arcs as f64
        }
    }

    /// One line per fact, grep-able, in the project's logging style.
    pub fn render(&self) -> String {
        format!(
            "story backfill\n  sessions: {}\n  sessions without events: {}\n  exchanges: {}\n  arcs: {}\n  exchanges per arc: {:.2}\n  arcs closed by gap: {}\n  ambiguous arcs: {} ({:.1}% ambiguous share)\n  patterns written: {}\n",
            self.sessions,
            self.sessions_without_events,
            self.exchanges,
            self.arcs,
            self.exchanges_per_arc(),
            self.gap_closed_arcs,
            self.ambiguous_arcs,
            self.ambiguous_share() * 100.0,
            self.patterns_written,
        )
    }
}

/// Pure: fold one session's events and return its story patterns.
pub fn fold_session(events: &[serde_json::Value], gap_threshold_secs: u64) -> Vec<PatternEvent> {
    let mut pipeline = PatternPipeline::with_turn_detectors(vec![Box::new(StoryDetector::new(
        gap_threshold_secs,
    ))]);
    let mut out = Vec::new();
    for value in events {
        if let Ok(ev) = serde_json::from_value::<CloudEvent>(value.clone()) {
            out.extend(pipeline.feed_event(&ev).0);
        }
    }
    out.extend(pipeline.flush().0);
    out.into_iter()
        .filter(|p| p.pattern_type.starts_with("story."))
        .collect()
}

/// Fold a report's counts from one session's story patterns.
fn tally(report: &mut StoryReport, patterns: &[PatternEvent]) {
    for p in patterns {
        match p.pattern_type.as_str() {
            "story.exchange" => report.exchanges += 1,
            "story.arc" => {
                report.arcs += 1;
                let seams = p.metadata["ambiguous_seams"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                if seams > 0 {
                    report.ambiguous_arcs += 1;
                }
                if p.metadata["closed_by"].as_str() == Some("gap") {
                    report.gap_closed_arcs += 1;
                }
            }
            _ => {}
        }
    }
}

/// Read-only report over every session in the store.
pub async fn report(store: &dyn EventStore, gap_threshold_secs: u64) -> Result<StoryReport> {
    run(store, gap_threshold_secs, false).await
}

/// Fold every session; with `write`, also persist the story patterns.
pub async fn run(
    store: &dyn EventStore,
    gap_threshold_secs: u64,
    write: bool,
) -> Result<StoryReport> {
    let mut report = StoryReport::default();
    for row in store.list_sessions().await? {
        report.sessions += 1;
        let events = store.session_events(&row.id).await?;
        if events.is_empty() {
            report.sessions_without_events += 1;
            continue;
        }
        let patterns = fold_session(&events, gap_threshold_secs);
        tally(&mut report, &patterns);
        if write {
            for p in &patterns {
                store.insert_pattern(&row.id, p).await?;
                report.patterns_written += 1;
            }
        }
    }
    Ok(report)
}
