//! `story_backfill` — fold every stored session through the story detector
//! and report what the fold sees (memory hands, requirement A-12).
//!
//! Read-only over events: it reads `session_events`, replays them through
//! a fresh `PatternPipeline` holding only the `StoryDetector`, and counts.
//! Writing the resulting `story.exchange` / `story.arc` patterns back is a
//! separate, explicit step (`write: true`) so the report can be run against
//! a live store with no side effects. Like `reproject`, this is one of the
//! explicit state-management operations.
//!
//! Publishing is a third, equally explicit step (`publish: true`): each
//! session's story patterns ride the bus on the same subject and in the
//! same shape the live patterns consumer uses, so a host on the MCP
//! `subscribe_arcs` stream hears backfilled arcs close exactly as it hears
//! live ones. Write and publish are independent — publishing without
//! writing announces the fold without persisting it.

use anyhow::Result;
use open_story_bus::Bus;
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
    /// Stale story rows removed before writing, so a changed fold never
    /// leaves an old handle beside a fresh one.
    pub patterns_replaced: usize,
    /// Story patterns that reached the bus. Counts only successful
    /// publishes, so it can trail `exchanges + arcs` when the bus is down.
    pub patterns_published: usize,
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
            "story backfill\n  sessions: {}\n  sessions without events: {}\n  exchanges: {}\n  arcs: {}\n  exchanges per arc: {:.2}\n  arcs closed by gap: {}\n  ambiguous arcs: {} ({:.1}% ambiguous share)\n  patterns written: {}\n  patterns replaced: {}\n  patterns published: {}\n",
            self.sessions,
            self.sessions_without_events,
            self.exchanges,
            self.arcs,
            self.exchanges_per_arc(),
            self.gap_closed_arcs,
            self.ambiguous_arcs,
            self.ambiguous_share() * 100.0,
            self.patterns_written,
            self.patterns_replaced,
            self.patterns_published,
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
/// Never publishes — see [`run_with_bus`].
pub async fn run(
    store: &dyn EventStore,
    gap_threshold_secs: u64,
    write: bool,
) -> Result<StoryReport> {
    run_with_bus(store, None, gap_threshold_secs, write).await
}

/// Fold every session; with `write`, persist the story patterns; with a
/// `bus`, publish each session's story patterns as one batch on
/// `patterns.{project}.{session_id}` — the live patterns consumer's
/// subject and payload (a JSON array of `PatternEvent`), so the MCP
/// `subscribe_arcs` pump decodes backfilled arcs like live ones.
///
/// `write` and `bus` are independent: a bus without `write` announces the
/// fold without persisting it, which is how a host can be caught up on
/// history that was already written by an earlier run.
///
/// Publishing is best-effort, like the live consumer: a failed publish is
/// logged and skipped, the fold continues, and only successful publishes
/// count toward `patterns_published`.
pub async fn run_with_bus(
    store: &dyn EventStore,
    bus: Option<&dyn Bus>,
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
            report.patterns_replaced +=
                store.delete_session_patterns(&row.id, "story.").await? as usize;
            for p in &patterns {
                store.insert_pattern(&row.id, p).await?;
                report.patterns_written += 1;
            }
        }
        if let Some(bus) = bus {
            if patterns.is_empty() {
                continue;
            }
            let project = match row.project_id.as_deref() {
                Some(p) if !p.is_empty() => p,
                _ => "default",
            };
            let subject = format!("patterns.{}.{}", project, row.id);
            match serde_json::to_vec(&patterns) {
                Ok(payload) => match bus.publish_bytes(&subject, &payload).await {
                    Ok(()) => report.patterns_published += patterns.len(),
                    Err(e) => eprintln!("story backfill publish_bytes({subject}) failed: {e}"),
                },
                Err(e) => eprintln!("story backfill serialize({subject}) failed: {e}"),
            }
        }
    }
    Ok(report)
}

// ═══════════════════════════════════════════════════════════════════
// Admin endpoint — POST /api/admin/story-backfill?write=true&publish=true&gap_secs=1800
// ═══════════════════════════════════════════════════════════════════

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BackfillQuery {
    /// Persist the story patterns (default: report only).
    #[serde(default)]
    pub write: bool,
    /// Publish each session's story patterns to the bus on the live
    /// consumer's `patterns.{project}.{session_id}` subject (default: no).
    /// Independent of `write`: `publish=true` alone announces the fold
    /// without persisting it.
    #[serde(default)]
    pub publish: bool,
    /// Arc gap threshold in seconds (default 1800).
    pub gap_secs: Option<u64>,
}

/// Fold every stored session through the story detector from inside the
/// running server, so history gets `story.exchange` / `story.arc` patterns
/// without a second process writing SQLite, and with `publish` also lets
/// a listening host hear those arcs close. Admin-gated like the other
/// state-management operations. Returns the report as JSON.
pub async fn admin_story_backfill(
    State(state): State<crate::state::SharedState>,
    Query(q): Query<BackfillQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (store, bus) = {
        let s = state.read().await;
        let bus = q.publish.then(|| s.bus.clone());
        (s.store.event_store.clone(), bus)
    };
    let gap = q
        .gap_secs
        .unwrap_or(open_story_patterns::story::DEFAULT_GAP_THRESHOLD_SECS);
    let report = run_with_bus(store.as_ref(), bus.as_deref(), gap, q.write)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("story backfill failed: {e}"),
            )
        })?;
    Ok(Json(serde_json::json!({
        "sessions": report.sessions,
        "sessions_without_events": report.sessions_without_events,
        "exchanges": report.exchanges,
        "arcs": report.arcs,
        "gap_closed_arcs": report.gap_closed_arcs,
        "ambiguous_arcs": report.ambiguous_arcs,
        "ambiguous_share": report.ambiguous_share(),
        "exchanges_per_arc": report.exchanges_per_arc(),
        "patterns_written": report.patterns_written,
        "patterns_published": report.patterns_published,
        "write": q.write,
        "publish": q.publish,
        "gap_secs": gap,
        "text": report.render(),
    })))
}
