//! Spec: SessionProjection — incremental cache with filter counts.
//!
//! Phase 2 of Story 036: Stateful BFF projection.
//!
//! SessionProjection replaces Vec<Value> in AppState. It incrementally
//! maintains tree structure, pattern detector states, and filter counts.
//! All derived state is updated on append, never recomputed from scratch.

mod helpers;

use helpers::{
    body_json, make_assistant_text, make_event,
    make_tool_result, make_tool_use, make_user_prompt, send_request, test_state,
};
use axum::body::Body;
use axum::http::Request;
use serde_json::{json, Value};
use tempfile::TempDir;

use open_story::server::projection::{filter_matches, SessionProjection, FILTER_NAMES};

/// Convert a CloudEvent to Value for projection.append().
fn to_value(ce: &open_story::cloud_event::CloudEvent) -> Value {
    serde_json::to_value(ce).unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// describe("SessionProjection::append")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod projection_append {
    use super::*;

    #[test]
    fn it_should_return_append_result_with_node() {
        let mut proj = SessionProjection::new("test-session");
        let event = make_user_prompt("test-session", "evt-1");
        let result = proj.append(&to_value(&event));
        assert!(!result.is_empty());
        assert_eq!(result.records[0].id, "evt-1");
    }

    #[test]
    fn it_should_increment_event_count() {
        let mut proj = SessionProjection::new("test-session");
        assert_eq!(proj.event_count(), 0);

        let e1 = make_user_prompt("test-session", "evt-1");
        proj.append(&to_value(&e1));
        assert_eq!(proj.event_count(), 1);

        let e2 = make_tool_use("test-session", "evt-2", Some("evt-1"), "Bash", "cargo test");
        proj.append(&to_value(&e2));
        assert_eq!(proj.event_count(), 2);
    }

    #[test]
    fn it_should_track_tree_depth() {
        let mut proj = SessionProjection::new("test-session");

        let e1 = make_user_prompt("test-session", "evt-1");
        proj.append(&to_value(&e1)); // depth 0 (no parent)

        let e2 = make_tool_use("test-session", "evt-2", Some("evt-1"), "Bash", "ls");
        proj.append(&to_value(&e2)); // depth 1

        let e3 = make_tool_result("test-session", "evt-3", Some("evt-2"), "toolu_evt-2", "ok");
        proj.append(&to_value(&e3)); // depth 2

        assert_eq!(proj.node_depth("evt-1"), 0);
        assert_eq!(proj.node_depth("evt-2"), 1);
        assert_eq!(proj.node_depth("evt-3"), 2);
    }

    #[test]
    fn it_should_deduplicate_by_event_id() {
        let mut proj = SessionProjection::new("test-session");

        let e1 = make_user_prompt("test-session", "evt-1");
        let result1 = proj.append(&to_value(&e1));
        assert!(!result1.is_empty());
        assert_eq!(proj.event_count(), 1);

        // Same event ID again
        let result2 = proj.append(&to_value(&e1));
        assert!(result2.is_empty());
        assert_eq!(proj.event_count(), 1); // unchanged
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("SessionProjection filter counts")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod filter_counts {
    use super::*;

    #[test]
    fn it_should_return_filter_deltas_on_append() {
        let mut proj = SessionProjection::new("test-session");
        let e = make_user_prompt("test-session", "evt-1");
        let result = proj.append(&to_value(&e));
        let deltas = &result.filter_deltas;

        assert!(deltas.get("all").copied().unwrap_or(0) >= 1);
        assert!(deltas.get("user").copied().unwrap_or(0) >= 1);
        assert!(deltas.get("narrative").copied().unwrap_or(0) >= 1);
        assert_eq!(deltas.get("tools").copied().unwrap_or(0), 0);
    }

    #[test]
    fn it_should_increment_tools_for_tool_use() {
        let mut proj = SessionProjection::new("test-session");

        let e1 = make_user_prompt("test-session", "evt-1");
        proj.append(&to_value(&e1));

        let e2 = make_tool_use("test-session", "evt-2", Some("evt-1"), "Bash", "cargo test");
        let result = proj.append(&to_value(&e2));
        let deltas = &result.filter_deltas;

        assert!(deltas.get("tools").copied().unwrap_or(0) >= 1);
        assert!(deltas.get("bash.test").copied().unwrap_or(0) >= 1);
    }

    /// Boundary table: filter count consistency after N appends.
    ///
    /// For every filter, the incrementally maintained count must
    /// match a full recount over all rows.
    #[test]
    fn incremental_counts_match_full_recount() {
        let mut proj = SessionProjection::new("test-session");

        // Realistic event sequence
        let events = vec![
            to_value(&make_user_prompt("test-session", "evt-1")),
            to_value(&make_tool_use("test-session", "evt-2", Some("evt-1"), "Bash", "cargo test")),
            to_value(&make_tool_result("test-session", "evt-3", Some("evt-2"), "toolu_evt-2", "test result: ok. 5 passed")),
            to_value(&make_assistant_text("test-session", "evt-4", Some("evt-3"), "All tests pass")),
            to_value(&make_tool_use("test-session", "evt-5", Some("evt-4"), "Edit", "/src/lib.rs")),
            to_value(&make_tool_result("test-session", "evt-6", Some("evt-5"), "toolu_evt-5", "file updated")),
            to_value(&make_tool_use("test-session", "evt-7", Some("evt-6"), "Bash", "git add .")),
            to_value(&make_tool_result("test-session", "evt-8", Some("evt-7"), "toolu_evt-7", "[master abc] fix")),
        ];

        for event in &events {
            proj.append(event);
        }

        // For each filter: cached count == full recount
        let rows = proj.timeline_rows();
        for name in FILTER_NAMES {
            let cached = proj.filter_counts().get(*name).copied().unwrap_or(0);
            let actual = rows.iter().filter(|r| filter_matches(name, r)).count();
            assert_eq!(
                cached, actual,
                "filter '{name}': incremental={cached}, recount={actual}"
            );
        }
    }

    #[test]
    fn query_meta_returns_cached_counts() {
        let mut proj = SessionProjection::new("test-session");

        proj.append(&to_value(&make_user_prompt("test-session", "evt-1")));
        proj.append(&to_value(&make_tool_use(
            "test-session", "evt-2", Some("evt-1"), "Read", "/foo.rs",
        )));

        let meta = proj.query_meta();
        assert_eq!(meta.event_count, 2);
        assert!(*meta.filter_counts.get("all").unwrap_or(&0) >= 2);
        assert!(*meta.filter_counts.get("tools").unwrap_or(&0) >= 1);
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("SessionProjection labels")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod projection_labels {
    use super::*;

    #[test]
    fn it_should_extract_label_from_first_user_prompt() {
        let mut proj = SessionProjection::new("test-session");
        let e = make_user_prompt("test-session", "evt-1");
        let result = proj.append(&to_value(&e));
        assert!(result.label_changed, "label should change on first prompt");
        assert_eq!(proj.label(), Some("test prompt"));
    }

    #[test]
    fn it_should_not_overwrite_label_on_second_prompt() {
        let mut proj = SessionProjection::new("test-session");

        // First prompt sets label
        let e1 = make_user_prompt("test-session", "evt-1");
        proj.append(&to_value(&e1));
        assert_eq!(proj.label(), Some("test prompt"));

        // Second prompt does NOT overwrite
        let e2 = make_user_prompt("test-session", "evt-2");
        let result = proj.append(&to_value(&e2));
        assert!(!result.label_changed, "label should not change on second prompt");
        assert_eq!(proj.label(), Some("test prompt"));
    }

    #[test]
    fn it_should_extract_branch_from_event_data() {
        let mut proj = SessionProjection::new("test-session");

        // Event with git_branch in agent_payload
        let mut event = to_value(&make_user_prompt("test-session", "evt-1"));
        event["data"]["agent_payload"]["git_branch"] = json!("feature/login-fix");
        let result = proj.append(&event);

        assert!(result.label_changed);
        assert_eq!(proj.branch(), Some("feature/login-fix"));
    }

    #[test]
    fn it_should_truncate_label_to_50_chars() {
        let mut proj = SessionProjection::new("test-session");

        let long_prompt = "a".repeat(100);
        let mut event = to_value(&make_user_prompt("test-session", "evt-1"));
        // Override the prompt text with a long string in agent_payload
        event["data"]["agent_payload"]["text"] = json!(long_prompt);
        event["data"]["raw"]["message"]["content"] = json!([{"type": "text", "text": long_prompt}]);
        proj.append(&event);

        let label = proj.label().expect("should have label");
        assert_eq!(label.len(), 50, "label should be truncated to 50 chars");
    }

    #[test]
    fn it_should_return_none_before_any_prompt() {
        let proj = SessionProjection::new("test-session");
        assert_eq!(proj.label(), None);
        assert_eq!(proj.branch(), None);
    }

    #[test]
    fn it_should_not_set_label_from_tool_events() {
        let mut proj = SessionProjection::new("test-session");
        let e = make_tool_use("test-session", "evt-1", None, "Bash", "cargo test");
        let result = proj.append(&to_value(&e));
        assert!(!result.label_changed);
        assert_eq!(proj.label(), None);
    }
}

// ═══════════════════════════════════════════════════════════════════
// describe("SessionProjection in AppState integration")
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod appstate_integration {
    use super::*;
    use open_story::server::ingest_events;

    #[tokio::test]
    async fn ingest_events_updates_projection() {
        let data_dir = TempDir::new().unwrap();
        let state = test_state(&data_dir);
        {
            let mut s = state.write().await;
            let events = vec![
                make_event("io.arc.event", "sess-1"),
                make_event("io.arc.event", "sess-1"),
                make_event("io.arc.event", "sess-1"),
            ];
            ingest_events(&mut s, "sess-1", &events, None);
        }

        let s = state.read().await;
        let proj = s.store.projections.get("sess-1").unwrap();
        assert_eq!(proj.event_count(), 3);
    }

    #[tokio::test]
    async fn initial_state_includes_session_labels() {
        use open_story::server::ws::build_initial_state;

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
        let init = build_initial_state(&s);
        let labels = &init.session_labels;
        assert!(labels.contains_key("sess-1"), "should have label for sess-1");
        let label = labels.get("sess-1").unwrap();
        assert_eq!(label.label.as_deref(), Some("test prompt"));
    }

    #[tokio::test]
    async fn meta_endpoint_uses_cached_counts() {
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

        let req = Request::get("/api/sessions/sess-1/meta")
            .body(Body::empty())
            .unwrap();
        let resp = send_request(state, req).await;
        assert_eq!(resp.status(), 200);

        let body = body_json(resp).await;
        assert!(body["filter_counts"].is_object());
        assert!(body["filter_counts"]["all"].as_u64().unwrap() >= 1);
        assert!(body["event_count"].as_u64().unwrap() >= 1);
    }
}
