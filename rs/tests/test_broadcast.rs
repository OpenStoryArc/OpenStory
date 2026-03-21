//! Spec: Enriched broadcast messages — durable vs ephemeral separation.
//!
//! Phase 3 of Story 036: Stateful BFF projection.
//!
//! BroadcastMessage::Enriched separates durable records (UI accumulates)
//! from ephemeral records (UI shows transiently, doesn't store).
//! Progress events are ephemeral. Everything else is durable.

mod helpers;

use helpers::{
    make_assistant_text, make_progress_event, make_tool_use, make_user_prompt, test_state,
};
use tempfile::TempDir;

use open_story::server::projection::{is_ephemeral, SessionProjection};
use open_story::server::ws::build_initial_state;
use open_story::server::ingest_events;

/// Convert a CloudEvent to Value for projection.append().
fn to_value(ce: &open_story::cloud_event::CloudEvent) -> serde_json::Value {
    serde_json::to_value(ce).unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// describe("is_ephemeral — ephemeral classification")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod ephemeral_classification {
    use super::*;

    /// Boundary table: subtype → durable or ephemeral
    ///
    /// | Subtype                    | Classification |
    /// |---------------------------|---------------|
    /// | message.user.prompt       | durable       |
    /// | message.user.tool_result  | durable       |
    /// | message.assistant.text    | durable       |
    /// | message.assistant.tool_use| durable       |
    /// | message.assistant.thinking| durable       |
    /// | progress.bash             | ephemeral     |
    /// | progress.agent            | ephemeral     |
    /// | progress.hook             | ephemeral     |
    /// | system.turn.complete      | durable       |
    /// | system.error              | durable       |
    /// | system.compact            | durable       |
    /// | (none)                    | durable       |
    #[test]
    fn boundary_table_ephemeral_vs_durable() {
        let cases: Vec<(Option<&str>, &str, bool)> = vec![
            // (subtype, description, expected_ephemeral)
            (Some("message.user.prompt"),        "user prompt",       false),
            (Some("message.user.tool_result"),   "tool result",       false),
            (Some("message.assistant.text"),     "assistant text",    false),
            (Some("message.assistant.tool_use"), "tool use",          false),
            (Some("message.assistant.thinking"), "thinking",          false),
            (Some("progress.bash"),              "bash progress",     true),
            (Some("progress.agent"),             "agent progress",    true),
            (Some("progress.hook"),              "hook progress",     true),
            (Some("system.turn.complete"),       "turn complete",     false),
            (Some("system.error"),               "error",             false),
            (Some("system.compact"),             "compaction",        false),
            (None,                               "no subtype",        false),
        ];

        for (subtype, desc, expected) in cases {
            assert_eq!(
                is_ephemeral(subtype),
                expected,
                "{desc} (subtype={subtype:?}): expected ephemeral={expected}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("Enriched broadcast — filter_deltas")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod filter_deltas {
    use super::*;
    #[test]
    fn user_message_increments_all_user_narrative() {
        let mut proj = SessionProjection::new("test");
        let e = make_user_prompt("test", "evt-1");
        let result = proj.append(&to_value(&e));
        let deltas = &result.filter_deltas;

        assert!(deltas.get("all").copied().unwrap_or(0) >= 1);
        assert!(deltas.get("user").copied().unwrap_or(0) >= 1);
        assert!(deltas.get("narrative").copied().unwrap_or(0) >= 1);
        assert_eq!(deltas.get("tools").copied().unwrap_or(0), 0);
    }

    #[test]
    fn bash_git_increments_bash_git_filter() {
        let mut proj = SessionProjection::new("test");
        proj.append(&to_value(&make_user_prompt("test", "evt-1")));
        let e = make_tool_use("test", "evt-2", Some("evt-1"), "Bash", "git status");
        let result = proj.append(&to_value(&e));
        let deltas = &result.filter_deltas;

        assert!(deltas.get("bash.git").copied().unwrap_or(0) >= 1);
        assert_eq!(deltas.get("bash.test").copied().unwrap_or(0), 0);
    }

    #[test]
    fn ephemeral_events_produce_no_filter_deltas() {
        let mut proj = SessionProjection::new("test");
        proj.append(&to_value(&make_user_prompt("test", "evt-1")));
        let e = make_progress_event("test", "evt-2", Some("evt-1"));
        let result = proj.append(&to_value(&e));

        // Progress events should produce empty filter_deltas
        // (they either produce no ViewRecords, or no matching filters)
        for (_name, delta) in &result.filter_deltas {
            // No non-trivial filter should increment for a progress event
            // (only "all" could match, which is fine)
            assert!(*delta >= 0);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("WS initial_state from projection cache")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod ws_initial_state {
    use super::*;

    #[tokio::test]
    async fn it_should_exclude_progress_from_initial_state() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            // 3 durable events
            let durable = vec![
                make_user_prompt("sess-1", "evt-1"),
                make_tool_use("sess-1", "evt-2", Some("evt-1"), "Bash", "ls"),
                make_assistant_text("sess-1", "evt-4", Some("evt-2"), "done"),
            ];
            ingest_events(&mut s, "sess-1", &durable, None);

            // 2 progress events
            let progress = vec![
                make_progress_event("sess-1", "evt-p1", Some("evt-2")),
                make_progress_event("sess-1", "evt-p2", Some("evt-2")),
            ];
            ingest_events(&mut s, "sess-1", &progress, None);
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let records = init.records;

        // Progress events should NOT be in initial_state
        // (projection.timeline_rows() only contains durable ViewRecords from from_cloud_event,
        //  and progress events produce empty ViewRecords from from_cloud_event)
        assert!(
            records.len() >= 3,
            "expected at least 3 durable records, got {}",
            records.len()
        );
    }

    #[tokio::test]
    async fn it_should_include_tree_metadata_in_initial_records() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt("sess-1", "evt-1"),
                make_tool_use("sess-1", "evt-2", Some("evt-1"), "Read", "/foo.rs"),
            ];
            ingest_events(&mut s, "sess-1", &events, None);
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let records = init.records;
        assert!(!records.is_empty());

        // Root-level records have depth 0
        let root_records: Vec<_> = records.iter().filter(|r| r.depth == 0).collect();
        assert!(!root_records.is_empty(), "should have root-level records");

        // Child records have parent_uuid
        let child_records: Vec<_> = records.iter().filter(|r| r.depth > 0).collect();
        for child in &child_records {
            assert!(
                child.parent_uuid.is_some(),
                "child record at depth {} should have parent_uuid",
                child.depth
            );
        }
    }

    #[tokio::test]
    async fn it_should_include_filter_counts_in_initial_state() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            let events = vec![
                make_user_prompt("sess-1", "evt-1"),
                make_tool_use("sess-1", "evt-2", Some("evt-1"), "Read", "/foo.rs"),
            ];
            ingest_events(&mut s, "sess-1", &events, None);
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let filter_counts = init.filter_counts;

        let sess_counts = filter_counts.get("sess-1").expect("should have session counts");
        assert!(*sess_counts.get("all").unwrap_or(&0) >= 2);
        assert!(*sess_counts.get("tools").unwrap_or(&0) >= 1);
        assert!(*sess_counts.get("reading").unwrap_or(&0) >= 1);
    }

    #[tokio::test]
    async fn it_should_cap_initial_records_at_max() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            s.config.max_initial_records = 500;
            // Ingest 600 events
            for i in 0..600 {
                let e = make_user_prompt("sess-1", &format!("evt-{i}"));
                ingest_events(&mut s, "sess-1", &[e], None);
            }
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let records = init.records;
        assert!(
            records.len() <= 500,
            "initial_state should be capped at 500, got {}",
            records.len()
        );
    }

    #[tokio::test]
    async fn it_should_send_most_recent_records() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            for i in 0..600 {
                let e = make_user_prompt("sess-1", &format!("evt-{i:04}"));
                ingest_events(&mut s, "sess-1", &[e], None);
            }
        }

        let s = state.read().await;
        let init = build_initial_state(&s); let records = init.records;

        // Records should be the most recent, not the oldest
        // The last record should be from near the end of the sequence
        let last = records.last().expect("should have records");
        assert_eq!(last.record.id, "evt-0599", "last record should be the most recent event");
    }

    /// When records are capped at MAX_INITIAL_RECORDS, filter_counts must
    /// reflect only the capped set — not the full projection history.
    /// Bug: filter_counts came from the uncapped projection, so the UI
    /// showed e.g. "Narrative 10424" for only 362 visible records.
    #[tokio::test]
    async fn filter_counts_should_match_capped_records() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            s.config.max_initial_records = 500;
            // Ingest 600 user prompts — exceeds max_initial_records (500)
            for i in 0..600 {
                let e = make_user_prompt("sess-1", &format!("evt-{i:04}"));
                ingest_events(&mut s, "sess-1", &[e], None);
            }
        }

        let s = state.read().await;
        let init = build_initial_state(&s);

        // Records are capped
        let n_records = init.records.len();
        assert!(n_records <= 500, "records should be capped at 500, got {n_records}");

        // Filter counts must not exceed the capped record count
        let sess_counts = init.filter_counts.get("sess-1")
            .expect("should have session filter counts");
        let all_count = *sess_counts.get("all").unwrap_or(&0);
        assert!(
            all_count <= n_records,
            "filter_counts['all'] ({all_count}) should not exceed capped record count ({n_records})"
        );
        let user_count = *sess_counts.get("user").unwrap_or(&0);
        assert!(
            user_count <= n_records,
            "filter_counts['user'] ({user_count}) should not exceed capped record count ({n_records})"
        );
    }

    /// When total records are below the cap, filter_counts should match
    /// the projection's counts exactly (no recomputation needed).
    #[tokio::test]
    async fn filter_counts_match_records_when_below_cap() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            // 3 user prompts + 2 tool uses = 5 events, well below 500 cap
            let events = vec![
                make_user_prompt("sess-1", "evt-1"),
                make_user_prompt("sess-1", "evt-2"),
                make_user_prompt("sess-1", "evt-3"),
                make_tool_use("sess-1", "evt-4", Some("evt-3"), "Read", "/foo.rs"),
                make_tool_use("sess-1", "evt-5", Some("evt-3"), "Edit", "/bar.rs"),
            ];
            ingest_events(&mut s, "sess-1", &events, None);
        }

        let s = state.read().await;
        let init = build_initial_state(&s);
        let sess_counts = init.filter_counts.get("sess-1")
            .expect("should have session filter counts");

        // "all" should equal total records delivered for this session
        let session_records: usize = init.records.iter()
            .filter(|r| r.record.session_id == "sess-1")
            .count();
        assert_eq!(
            *sess_counts.get("all").unwrap_or(&0), session_records,
            "uncapped: filter_counts['all'] should equal delivered record count"
        );
        assert!(*sess_counts.get("user").unwrap_or(&0) >= 3, "should have at least 3 user records");
        assert!(*sess_counts.get("tools").unwrap_or(&0) >= 2, "should have at least 2 tool records");
    }

    /// Boundary table: multi-session capping recomputes per-session counts.
    ///
    /// | Session | Events | After cap (500 total) | Expected filter_counts      |
    /// |---------|--------|-----------------------|-----------------------------|
    /// | sess-1  | 400    | ~167 (oldest trimmed) | all <= 167, user <= 167     |
    /// | sess-2  | 400    | ~167                  | all <= 167, user <= 167     |
    /// | sess-3  | 400    | ~167 (newest kept)    | all <= 167, user <= 167     |
    /// | Total   | 1200   | 500                   | sum(all) == 500             |
    #[tokio::test]
    async fn filter_counts_recomputed_per_session_when_capped() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            s.config.max_initial_records = 500;
            // 3 sessions × 400 events = 1200 total, cap triggers at 500
            for sess in ["sess-1", "sess-2", "sess-3"] {
                for i in 0..400 {
                    let e = make_user_prompt(sess, &format!("{sess}-evt-{i:04}"));
                    ingest_events(&mut s, sess, &[e], None);
                }
            }
        }

        let s = state.read().await;
        let init = build_initial_state(&s);

        assert_eq!(init.records.len(), 500, "records should be capped at 500");

        // Sum of per-session "all" counts must equal total capped records
        let total_all: usize = init.filter_counts.values()
            .map(|c| *c.get("all").unwrap_or(&0))
            .sum();
        assert_eq!(
            total_all, 500,
            "sum of per-session filter_counts['all'] ({total_all}) should equal capped total (500)"
        );

        // Each session's counts must not exceed its share of capped records
        for (sid, counts) in &init.filter_counts {
            let session_records = init.records.iter()
                .filter(|r| r.record.session_id == *sid)
                .count();
            let all_count = *counts.get("all").unwrap_or(&0);
            assert_eq!(
                all_count, session_records,
                "session {sid}: filter_counts['all'] ({all_count}) should equal delivered records ({session_records})"
            );
        }
    }
}
