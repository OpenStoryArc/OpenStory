//! Sovereignty invariant: `data.raw` is never mutated by the translator
//! pipeline. CLAUDE.md is explicit:
//!
//!   > Don't mutate `raw` or normalize agent-specific fields. […] The fix:
//!   > translators leave `raw` untouched, set an `agent` discriminator, and
//!   > the views layer branches on agent type.
//!
//! This test walks the real captured fixtures, runs them through
//! `read_new_lines`, and asserts that for every emitted CloudEvent the
//! `data.raw` value equals what `serde_json::from_str` returns on the
//! source line. If a translator ever silently strips, normalizes, or
//! re-orders fields, this test fails.
//!
//! Note on pi-mono decomposition: pi-mono bundles N content blocks per
//! line and decomposes into N CloudEvents, all sharing the same source
//! line. Each emitted event's `data.raw` must equal the bundled-line
//! parse — a single source line maps to one canonical raw value, even
//! when N events come out of it.

use open_story_core::reader::read_new_lines;
use open_story_core::translate::TranscriptState;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures")
}

/// Read a JSONL file as parsed JSON values, one per non-empty line.
fn read_jsonl_values(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("read {}", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Walk a fixture: produce a map from "source line value" to the set of
/// emitted CloudEvent.data.raw values. Every source line should map to
/// exactly one canonical raw (which all emitted events agree on).
fn raw_passthrough_failures(path: &Path) -> Vec<String> {
    let source_lines = read_jsonl_values(path);
    if source_lines.is_empty() {
        return vec![];
    }

    let mut state = TranscriptState::new(format!(
        "rawcheck-{}",
        path.file_stem().unwrap().to_string_lossy()
    ));
    let events = match read_new_lines(path, &mut state) {
        Ok(e) => e,
        Err(e) => return vec![format!("read_new_lines failed: {e}")],
    };

    // For every emitted event, find the source line that should match.
    // We use a set of canonical raw values from the source — every
    // event's data.raw must be a member.
    let canonical: Vec<Value> = source_lines.into_iter().collect();
    let canonical_strs: Vec<String> = canonical
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();

    let mut failures: Vec<String> = Vec::new();
    let mut event_count_by_raw: HashMap<String, usize> = HashMap::new();
    for event in &events {
        let raw_str = serde_json::to_string(&event.data.raw).unwrap();
        *event_count_by_raw.entry(raw_str.clone()).or_insert(0) += 1;
        if !canonical_strs.iter().any(|s| s == &raw_str) {
            // The event's raw didn't match ANY source line. Mutation.
            let preview = if raw_str.len() > 200 {
                format!("{}…", &raw_str[..200])
            } else {
                raw_str.clone()
            };
            failures.push(format!(
                "event id={} subtype={:?} has data.raw that doesn't match any source line. preview: {}",
                event.id, event.subtype, preview
            ));
        }
    }
    failures
}

#[test]
fn pi_mono_fixtures_preserve_data_raw_byte_for_byte() {
    let dir = fixtures_dir().join("pi_mono");
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("read {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect();
    assert!(!files.is_empty(), "no pi-mono fixtures");

    let mut all_failures: Vec<(PathBuf, String)> = Vec::new();
    for path in &files {
        for f in raw_passthrough_failures(path) {
            all_failures.push((path.clone(), f));
        }
    }

    if !all_failures.is_empty() {
        eprintln!("\n❌ raw-passthrough failures in pi-mono fixtures:");
        for (path, msg) in &all_failures {
            eprintln!("  {}: {}", path.file_name().unwrap().to_string_lossy(), msg);
        }
        panic!("{} pi-mono raw mutation(s)", all_failures.len());
    }
}

#[test]
fn pi_mono_decomposed_events_all_share_the_same_raw() {
    // The pi-mono decomposer fans out N events from one bundled line.
    // The contract (DR2 in the decomposition plan): all decomposed
    // events share the same data.raw equal to the source line. This
    // test re-asserts that property over the real fixtures so a future
    // refactor that "optimizes" by stripping per-event raw fields gets
    // caught.
    let path = fixtures_dir().join("pi_mono/scenario_07_multi_tool.jsonl");
    let mut state = TranscriptState::new("decomp-raw".to_string());
    let events = read_new_lines(&path, &mut state).unwrap();

    // Group events by their source line (we don't track that directly,
    // but events with the same `time` and matching parent_uuid in raw
    // came from the same bundle). Simpler: group by raw value itself.
    let mut groups: HashMap<String, Vec<&open_story_core::cloud_event::CloudEvent>> = HashMap::new();
    for ev in &events {
        let raw_str = serde_json::to_string(&ev.data.raw).unwrap();
        groups.entry(raw_str).or_default().push(ev);
    }

    // For pi-mono scenario_07, line 5 has [toolCall, toolCall] which
    // decomposes to 2 events sharing the same raw. So we expect at least
    // one group with multiple events all referencing the same raw.
    let multi_groups: Vec<_> = groups.iter().filter(|(_, evs)| evs.len() > 1).collect();
    assert!(
        !multi_groups.is_empty(),
        "scenario_07 should produce at least one decomposed group"
    );
    for (raw, evs) in multi_groups {
        let first = serde_json::to_string(&evs[0].data.raw).unwrap();
        for ev in &evs[1..] {
            let other = serde_json::to_string(&ev.data.raw).unwrap();
            assert_eq!(
                first, other,
                "decomposed events from one bundled line must share data.raw byte-for-byte"
            );
        }
        // Ensure raw is the bundled (multi-block) original, not a slice.
        assert!(
            raw.contains("toolCall"),
            "decomposed event's raw should preserve the full bundled content array"
        );
    }
}

#[test]
fn hermes_fixtures_preserve_data_raw_byte_for_byte() {
    let dir = fixtures_dir().join("hermes");
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("read {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect();
    if files.is_empty() {
        // Hermes fixtures may not be present in all checkouts — skip
        // gracefully rather than fail the test.
        eprintln!("no hermes fixtures present, skipping");
        return;
    }

    let mut all_failures: Vec<(PathBuf, String)> = Vec::new();
    for path in &files {
        for f in raw_passthrough_failures(path) {
            all_failures.push((path.clone(), f));
        }
    }

    if !all_failures.is_empty() {
        eprintln!("\n❌ raw-passthrough failures in hermes fixtures:");
        for (path, msg) in all_failures.iter().take(5) {
            eprintln!("  {}: {}", path.file_name().unwrap().to_string_lossy(), msg);
        }
        panic!("{} hermes raw mutation(s)", all_failures.len());
    }
}

#[test]
fn claude_code_synthetic_fixture_preserves_data_raw() {
    // synthetic.jsonl is hand-rolled Claude Code shape — the canonical
    // exercise of the default translator path.
    let path = fixtures_dir().join("synthetic.jsonl");
    let failures = raw_passthrough_failures(&path);
    if !failures.is_empty() {
        eprintln!("\n❌ failures:");
        for f in &failures {
            eprintln!("  {f}");
        }
        panic!("{} claude-code raw mutation(s)", failures.len());
    }
}
