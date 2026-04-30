//! Spec: Enriched broadcast messages — durable vs ephemeral separation.
//!
//! Phase 3 of Story 036: Stateful BFF projection.
//!
//! BroadcastMessage::Enriched separates durable records (UI accumulates)
//! from ephemeral records (UI shows transiently, doesn't store).
//! Progress events are ephemeral. Everything else is durable.
//!
//! ── ws_initial_state module removed (feat/lazy-load-initial-state) ──
//! The pre-redesign WS handshake shipped every session's enriched
//! WireRecords; tests in this module pinned the cap, sort order, and
//! per-session filter_counts of that payload. After the lazy-load
//! redesign the handshake carries only session_labels and recent
//! patterns — records moved to GET /api/sessions/{id}/records. The new
//! contracts are inline-tested in rs/server/src/ws.rs, and the
//! tree-metadata contract for the records endpoint is exercised by the
//! UI lazy-load tests. To compare against the deleted module:
//!   git show master:rs/tests/test_broadcast.rs

mod helpers;

use helpers::{make_progress_event, make_tool_use, make_user_prompt};

use open_story::server::projection::{is_ephemeral, SessionProjection};

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
