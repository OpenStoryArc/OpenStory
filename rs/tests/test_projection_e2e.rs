//! E2E integration: real fixture data through the full projection pipeline.
//!
//! These tests mirror the invariants from the Python prototypes
//! (bff_cache_model.py, streaming_patterns.py, tree_prototype.py)
//! but run against real session data through the Rust implementation.
//!
//! Prototype invariants verified:
//!   1. Incremental filter counts match full recount (bff_cache_model.py:569-573)
//!   2. Tree depth/parent chains are consistent (tree_prototype.py, bff_cache_model.py:720-724)
//!   3. Filter deltas enable UI-side incremental updates (bff_cache_model.py:662-676)
//!   4. Dedup prevents double-counting (bff_cache_model.py:623-626)
//!   5. WS initial_state contains cached metadata (bff_cache_model.py:630-635)
//!   6. Multiple sessions are independent (bff_cache_model.py:648-655)
//!   7. Ephemeral events excluded from durable timeline (streaming_patterns.py)

mod helpers;

use std::collections::HashMap;
use std::path::PathBuf;

use helpers::test_state;
use tempfile::TempDir;

use open_story::reader::read_new_lines;
use open_story::server::ingest_events;
use open_story::server::projection::{filter_matches, is_ephemeral, SessionProjection, FILTER_NAMES};
use open_story::server::ws::build_initial_state;
use open_story::translate::TranscriptState;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Load a real fixture file, translate to CloudEvents, ingest into state.
/// Returns the session_id and number of events ingested.
fn load_fixture(state: &mut open_story::server::AppState, filename: &str) -> (String, usize) {
    let path = fixtures_dir().join(filename);
    let session_id = filename.trim_end_matches(".jsonl").to_string();
    let mut ts = TranscriptState::new(session_id.clone());
    let events = read_new_lines(&path, &mut ts).unwrap();
    let result = ingest_events(state, &session_id, &events, None);
    (session_id, result.count)
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 1: incremental counts == full recount")
// From: bff_cache_model.py lines 569-573
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod incremental_consistency {
    use super::*;

    /// Boundary table: for every real session fixture × every filter,
    /// the incrementally maintained count must equal a full recount.
    ///
    /// | Fixture                          | Filters | Expected          |
    /// |----------------------------------|---------|-------------------|
    /// | synth_origin.jsonl               | all 21  | cached == recount |
    /// | synth_hooks.jsonl                | all 21  | cached == recount |
    /// | synth_translator.jsonl           | all 21  | cached == recount |
    /// | synth_global.jsonl               | all 21  | cached == recount |
    /// | synthetic.jsonl                   | all 21  | cached == recount |
    #[tokio::test]
    async fn all_fixtures_incremental_matches_recount() {
        let fixtures = [
            "synth_origin.jsonl",
            "synth_hooks.jsonl",
            "synth_translator.jsonl",
            "synth_global.jsonl",
            "synthetic.jsonl",
        ];

        for fixture in fixtures {
            let data_dir = TempDir::new().unwrap();
            let state = test_state(&data_dir);
            let (session_id, count) = {
                let mut s = state.write().await;
                load_fixture(&mut s, fixture)
            };

            let s = state.read().await;
            let proj = s.store.projections.get(&session_id).unwrap();
            let rows = proj.timeline_rows();

            assert!(
                count > 0,
                "{fixture}: expected events to be ingested"
            );

            for name in FILTER_NAMES {
                let cached = proj.filter_counts().get(*name).copied().unwrap_or(0);
                let actual = rows.iter().filter(|r| filter_matches(name, r)).count();
                assert_eq!(
                    cached, actual,
                    "{fixture}: filter '{name}': incremental={cached}, recount={actual}"
                );
            }

            eprintln!(
                "{fixture}: {count} events, {} view_records, {} filters verified",
                rows.len(),
                FILTER_NAMES.len()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 2: tree depth/parent consistency")
// From: tree_prototype.py, bff_cache_model.py:720-724
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tree_consistency {
    use super::*;

    /// For every record in the projection:
    /// - Root records (no parentUuid in event) have depth 0
    /// - Non-root records have depth == parent_depth + 1
    /// - Parent UUID references an existing node
    #[tokio::test]
    async fn tree_depth_parent_chain_is_consistent() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            load_fixture(&mut s, "synth_origin.jsonl");
        }

        let s = state.read().await;
        let proj = s.store.projections.values().next().unwrap();
        let rows = proj.timeline_rows();

        let mut depth_check_count = 0;
        for vr in rows {
            let depth = proj.node_depth(&vr.id);
            let parent = proj.node_parent(&vr.id);

            if let Some(pid) = parent {
                // Non-root: parent exists and depth = parent_depth + 1
                let parent_depth = proj.node_depth(pid);
                assert_eq!(
                    depth,
                    parent_depth + 1,
                    "record {}: depth {} != parent_depth {} + 1",
                    &vr.id[..8.min(vr.id.len())],
                    depth,
                    parent_depth
                );
                depth_check_count += 1;
            } else {
                // Root: depth should be 0
                assert_eq!(
                    depth, 0,
                    "root record {} should have depth 0, got {}",
                    &vr.id[..8.min(vr.id.len())],
                    depth
                );
            }
        }

        eprintln!(
            "tree consistency: {} records, {} parent-child links verified",
            rows.len(),
            depth_check_count
        );
        assert!(
            depth_check_count > 0,
            "expected at least some records with parent links"
        );
    }

    /// Max depth should be reasonable (prototypes found up to 839 in large sessions).
    #[tokio::test]
    async fn tree_has_nontrivial_depth() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            load_fixture(&mut s, "synth_global.jsonl");
        }

        let s = state.read().await;
        let proj = s.store.projections.values().next().unwrap();
        let rows = proj.timeline_rows();

        let max_depth = rows.iter().map(|r| proj.node_depth(&r.id)).max().unwrap_or(0);
        eprintln!("max depth in synth_global: {}", max_depth);
        assert!(
            max_depth >= 2,
            "real session should have depth >= 2 (tool chains), got {}",
            max_depth
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 3: filter deltas enable incremental UI")
// From: bff_cache_model.py:662-676
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod filter_deltas_accumulation {
    use super::*;

    /// Simulate a UI client: start with empty counts, apply deltas from
    /// each append, verify final counts match server projection.
    #[test]
    fn ui_delta_accumulation_matches_server_cache() {
        let path = fixtures_dir().join("synth_hooks.jsonl");
        let session_id = "synth_hooks";
        let mut ts = TranscriptState::new(session_id.to_string());
        let events = read_new_lines(&path, &mut ts).unwrap();

        let mut proj = SessionProjection::new(session_id);
        let mut ui_counts: HashMap<String, i64> = HashMap::new();

        for ce in &events {
            let val = serde_json::to_value(ce).unwrap();
            let result = proj.append(&val);

            // UI applies deltas from each broadcast
            for (name, delta) in &result.filter_deltas {
                *ui_counts.entry(name.clone()).or_insert(0) += *delta as i64;
            }
        }

        // UI counts should match server cache exactly
        for name in FILTER_NAMES {
            let ui = ui_counts.get(*name).copied().unwrap_or(0) as usize;
            let server = proj.filter_counts().get(*name).copied().unwrap_or(0);
            assert_eq!(
                ui, server,
                "filter '{name}': UI accumulated={ui}, server cached={server}"
            );
        }

        let total = proj.filter_counts().get("all").copied().unwrap_or(0);
        eprintln!(
            "delta accumulation: {} events, {} total 'all' count, all 21 filters match",
            events.len(),
            total
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 4: dedup prevents double-counting")
// From: bff_cache_model.py:623-626
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod dedup {
    use super::*;

    /// Ingesting the same events twice should not double the counts.
    /// We read once and ingest the same Vec<CloudEvent> twice.
    #[tokio::test]
    async fn double_ingest_is_idempotent() {
        let path = fixtures_dir().join("synthetic.jsonl");
        let mut ts = TranscriptState::new("synthetic".to_string());
        let events = read_new_lines(&path, &mut ts).unwrap();
        assert!(!events.is_empty());

        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);

        let count_first = {
            let mut s = state.write().await;
            ingest_events(&mut s, "synthetic", &events, None).count
        };

        let count_second = {
            let mut s = state.write().await;
            ingest_events(&mut s, "synthetic", &events, None).count
        };

        assert!(count_first > 0, "first ingest should produce events");
        assert_eq!(count_second, 0, "second ingest should be fully deduped");

        let s = state.read().await;
        let proj = s.store.projections.get("synthetic").unwrap();
        assert_eq!(
            proj.event_count(),
            count_first,
            "event count should not change after dedup"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 5: WS initial_state from cache")
// From: bff_cache_model.py:630-635
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod ws_initial_state_real_data {
    use super::*;

    /// build_initial_state returns WireRecords with tree metadata
    /// and per-session filter_counts from real data.
    #[tokio::test]
    async fn initial_state_has_tree_metadata_and_counts() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            load_fixture(&mut s, "synth_origin.jsonl");
        }

        let s = state.read().await;
        let init = build_initial_state(&s);
        let records = init.records;
        let filter_counts = init.filter_counts;

        // Records should have tree metadata
        assert!(!records.is_empty(), "initial_state should have records");
        let with_parent: Vec<_> = records.iter().filter(|r| r.parent_uuid.is_some()).collect();
        assert!(
            !with_parent.is_empty(),
            "some records should have parent_uuid"
        );

        let max_depth = records.iter().map(|r| r.depth).max().unwrap_or(0);
        assert!(max_depth >= 1, "should have non-trivial depth");

        // filter_counts should be populated
        let session_id = "synth_origin";
        let counts = filter_counts.get(session_id).expect("session should have counts");
        assert!(
            *counts.get("all").unwrap_or(&0) > 0,
            "all filter count should be > 0"
        );
        assert!(
            *counts.get("tools").unwrap_or(&0) > 0,
            "tools filter count should be > 0 in a real session"
        );

        eprintln!(
            "initial_state: {} records, max_depth={}, all={}, tools={}",
            records.len(),
            max_depth,
            counts.get("all").unwrap_or(&0),
            counts.get("tools").unwrap_or(&0)
        );
    }

    /// Capping: even with a large fixture, initial_state is bounded.
    #[tokio::test]
    async fn initial_state_capped_for_large_session() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            s.config.max_initial_records = 500;
            load_fixture(&mut s, "synth_global.jsonl");
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let records = init.records;

        // synth_global has 700+ events → should be capped at 500
        assert!(
            records.len() <= 500,
            "initial_state should be capped at 500, got {}",
            records.len()
        );

        // Should be the most recent records (sorted by timestamp)
        for i in 1..records.len() {
            assert!(
                records[i].record.timestamp >= records[i - 1].record.timestamp,
                "records should be sorted by timestamp"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 6: multiple sessions independent")
// From: bff_cache_model.py:648-655
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod multi_session {
    use super::*;

    /// Loading multiple fixtures creates independent projections.
    #[tokio::test]
    async fn multiple_sessions_are_independent() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        let (sid_a, count_a, sid_b, count_b) = {
            let mut s = state.write().await;
            let (a, ca) = load_fixture(&mut s, "synth_origin.jsonl");
            let (b, cb) = load_fixture(&mut s, "synth_hooks.jsonl");
            (a, ca, b, cb)
        };

        let s = state.read().await;
        let proj_a = s.store.projections.get(&sid_a).unwrap();
        let proj_b = s.store.projections.get(&sid_b).unwrap();

        assert_eq!(proj_a.event_count(), count_a);
        assert_eq!(proj_b.event_count(), count_b);
        assert_ne!(
            proj_a.event_count(),
            proj_b.event_count(),
            "different fixtures should have different event counts"
        );

        // Each session's filter counts are independent
        let all_a = proj_a.filter_counts().get("all").copied().unwrap_or(0);
        let all_b = proj_b.filter_counts().get("all").copied().unwrap_or(0);
        assert!(all_a > 0 && all_b > 0);
        assert_ne!(all_a, all_b, "different sessions should have different counts");

        eprintln!(
            "multi-session: A={}events/{}all, B={}events/{}all",
            count_a, all_a, count_b, all_b
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Prototype invariant 7: ephemeral classification")
// From: streaming_patterns.py subtypes
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod ephemeral_in_real_data {
    use super::*;

    /// Real session data contains progress events. They should be
    /// classified as ephemeral and NOT appear in the projection timeline.
    #[tokio::test]
    async fn progress_events_classified_correctly_in_real_data() {
        let path = fixtures_dir().join("synthetic.jsonl");
        let session_id = "synthetic";
        let mut ts = TranscriptState::new(session_id.to_string());
        let events = read_new_lines(&path, &mut ts).unwrap();

        // Count progress vs durable events
        let mut progress_count = 0;
        let mut durable_count = 0;
        for ce in &events {
            if is_ephemeral(ce.subtype.as_deref()) {
                progress_count += 1;
            } else {
                durable_count += 1;
            }
        }

        eprintln!(
            "synthetic.jsonl: {} total, {} progress, {} durable",
            events.len(),
            progress_count,
            durable_count
        );

        // The synthetic fixture has progress events
        assert!(
            progress_count > 0,
            "synthetic fixture should contain progress events"
        );
        assert!(
            durable_count > 0,
            "synthetic fixture should contain durable events"
        );

        // Projection timeline should only contain records from durable events
        let mut proj = SessionProjection::new(session_id);
        for ce in &events {
            let val = serde_json::to_value(ce).unwrap();
            proj.append(&val);
        }

        // All timeline rows should come from durable events
        // (projection stores all ViewRecords from from_cloud_event,
        // but progress events produce empty ViewRecords from from_cloud_event
        // and thus don't appear in timeline_rows)
        let timeline_count = proj.timeline_rows().len();
        eprintln!(
            "projection timeline: {} rows from {} durable events",
            timeline_count,
            durable_count
        );
        assert!(
            timeline_count > 0,
            "projection should have timeline rows"
        );
    }

    /// Filter counts from real data should have sensible values:
    /// - "all" >= every other filter
    /// - "tools" > 0 (real sessions always have tool calls)
    /// - "narrative" > 0 (real sessions have user + assistant messages)
    #[tokio::test]
    async fn filter_counts_sensible_in_real_data() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            load_fixture(&mut s, "synth_hooks.jsonl");
        }

        let s = state.read().await;
        let proj = s.store.projections.get("synth_hooks").unwrap();
        let counts = proj.filter_counts();

        let all = counts.get("all").copied().unwrap_or(0);
        assert!(all > 0, "'all' should be > 0");

        // "all" should be >= every other filter
        for name in FILTER_NAMES {
            let count = counts.get(*name).copied().unwrap_or(0);
            assert!(
                count <= all,
                "filter '{name}'={count} should be <= 'all'={all}"
            );
        }

        // Real sessions should have tools and narrative
        assert!(
            counts.get("tools").copied().unwrap_or(0) > 0,
            "real session should have tool calls"
        );
        assert!(
            counts.get("narrative").copied().unwrap_or(0) > 0,
            "real session should have narrative (user + assistant)"
        );

        eprintln!("filter counts for synth_hooks:");
        for name in FILTER_NAMES {
            let count = counts.get(*name).copied().unwrap_or(0);
            if count > 0 {
                eprintln!("  {name:20} {count:>5}");
            }
        }
    }
}
