//! BDD specs for streaming pattern detectors.
//!
//! Ported from scripts/streaming_patterns.py (28 Python tests).
//! Each detector is tested independently with boundary tables.

use serde_json::json;

use open_story_views::unified::*;
use open_story_views::view_record::ViewRecord;
use open_story_views::tool_input;

use open_story::patterns::*;

// ═══════════════════════════════════════════════════════════════════
// Test helpers — ViewRecord factories
// ═══════════════════════════════════════════════════════════════════

fn make_ctx<'a>(record: &'a ViewRecord, depth: u16, parent_uuid: Option<&'a str>) -> FeedContext<'a> {
    FeedContext {
        record,
        depth,
        parent_uuid,
    }
}

fn bash_call(id: &str, command: &str) -> ViewRecord {
    let input = json!({"command": command});
    let typed = tool_input::parse_tool_input("Bash", input.clone());
    ViewRecord {
        id: id.into(),
        seq: 1,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolCall(Box::new(ToolCall {
            call_id: format!("toolu_{id}"),
            name: "Bash".into(),
            input: input.clone(),
            raw_input: input,
            typed_input: Some(typed),
            status: None,
        })),
    }
}

fn tool_call(id: &str, name: &str, call_id: &str) -> ViewRecord {
    let input = json!({"file_path": "/test.rs"});
    let typed = tool_input::parse_tool_input(name, input.clone());
    ViewRecord {
        id: id.into(),
        seq: 1,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolCall(Box::new(ToolCall {
            call_id: call_id.into(),
            name: name.into(),
            input: input.clone(),
            raw_input: input,
            typed_input: Some(typed),
            status: None,
        })),
    }
}

fn agent_call(id: &str, call_id: &str, prompt: &str) -> ViewRecord {
    let input = json!({"prompt": prompt, "description": "test agent"});
    let typed = tool_input::parse_tool_input("Agent", input.clone());
    ViewRecord {
        id: id.into(),
        seq: 1,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolCall(Box::new(ToolCall {
            call_id: call_id.into(),
            name: "Agent".into(),
            input: input.clone(),
            raw_input: input,
            typed_input: Some(typed),
            status: None,
        })),
    }
}

fn tool_result(id: &str, call_id: &str, output: &str) -> ViewRecord {
    ViewRecord {
        id: id.into(),
        seq: 2,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:01Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolResult(ToolResult {
            call_id: call_id.into(),
            output: Some(output.into()),
            is_error: false,
            tool_outcome: None,
        }),
    }
}

fn error_result(id: &str, call_id: &str, output: &str) -> ViewRecord {
    ViewRecord {
        id: id.into(),
        seq: 2,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:01Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolResult(ToolResult {
            call_id: call_id.into(),
            output: Some(output.into()),
            is_error: true,
            tool_outcome: None,
        }),
    }
}

fn user_message(id: &str) -> ViewRecord {
    ViewRecord {
        id: id.into(),
        seq: 3,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:02Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::UserMessage(UserMessage {
            content: MessageContent::Text("test".into()),
            images: vec![],
        }),
    }
}

fn assistant_text(id: &str) -> ViewRecord {
    ViewRecord {
        id: id.into(),
        seq: 4,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:01Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::AssistantMessage(Box::new(AssistantMessage {
            model: "claude-4".into(),
            content: vec![ContentBlock::Text {
                text: "Let me fix that.".into(),
            }],
            stop_reason: None,
            end_turn: None,
            phase: None,
        })),
    }
}

fn reasoning(id: &str) -> ViewRecord {
    ViewRecord {
        id: id.into(),
        seq: 4,
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:01Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::Reasoning(Reasoning {
            summary: vec![],
            content: Some("Analyzing the error...".into()),
            encrypted: false,
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════
// TestCycleDetector
// ═══════════════════════════════════════════════════════════════════

mod test_cycle {
    use super::*;

    #[test]
    fn it_should_detect_full_red_green_cycle() {
        // test -> fail -> edit -> test -> pass
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("a", "cargo test -p open-story");
        let r = d.feed(&make_ctx(&r1, 0, None));
        assert!(r.is_empty(), "test command starts cycle silently");

        let r2 = tool_result("b", "toolu_a", "FAILED 3 tests, exit code 1");
        let r = d.feed(&make_ctx(&r2, 0, None));
        assert!(r.is_empty(), "failure transitions silently");

        let r3 = tool_call("c", "Edit", "toolu_c");
        let r = d.feed(&make_ctx(&r3, 0, None));
        assert!(r.is_empty(), "edit transitions silently");

        let r4 = bash_call("d", "cargo test -p open-story");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert!(r.is_empty(), "re-test transitions silently");

        let r5 = tool_result("e", "toolu_d", "test result: ok. 54 passed");
        let r = d.feed(&make_ctx(&r5, 0, None));
        assert_eq!(r.len(), 1, "pass emits test.cycle");
        assert_eq!(r[0].pattern_type, "test.cycle");
        assert_eq!(r[0].metadata["passed"], true);
        assert_eq!(r[0].metadata["iterations"], 2);
        assert_eq!(r[0].metadata["edits"], 1);
    }

    #[test]
    fn it_should_detect_immediate_pass_as_trivial_cycle() {
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("x", "npm test");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = tool_result("y", "toolu_x", "test result: ok. 54 passed");
        let r = d.feed(&make_ctx(&r2, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["iterations"], 1);
        assert_eq!(r[0].metadata["passed"], true);
    }

    #[test]
    fn it_should_flush_incomplete_cycle_as_fail() {
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("a", "cargo test");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = tool_result("b", "toolu_a", "FAILED 2 tests");
        d.feed(&make_ctx(&r2, 0, None));

        let r = d.flush();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["passed"], false);
    }

    #[test]
    fn it_should_reset_on_user_message_during_saw_test() {
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("a", "cargo test");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = user_message("b");
        let r = d.feed(&make_ctx(&r2, 0, None));
        assert!(r.is_empty());

        // Should be reset — next test starts fresh
        let r3 = bash_call("c", "cargo test");
        d.feed(&make_ctx(&r3, 0, None));
        // Flush should show 1 iteration, not 2
        let f = d.flush();
        assert_eq!(f[0].metadata["iterations"], 1);
    }

    #[test]
    fn it_should_emit_fail_on_user_interrupt_during_editing() {
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("a", "cargo test");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = tool_result("b", "toolu_a", "FAILED");
        d.feed(&make_ctx(&r2, 0, None));
        let r3 = tool_call("c", "Edit", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        let r4 = user_message("d");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["passed"], false);
    }

    #[test]
    fn it_should_not_trigger_on_non_test_bash_commands() {
        let mut d = TestCycleDetector::new();

        let r1 = bash_call("a", "ls -la");
        let r = d.feed(&make_ctx(&r1, 0, None));
        assert!(r.is_empty());

        let f = d.flush();
        assert!(f.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════
// GitFlowDetector
// ═══════════════════════════════════════════════════════════════════

mod git_flow {
    use super::*;

    #[test]
    fn it_should_detect_git_command_sequence() {
        let mut d = GitFlowDetector::new();

        let r1 = bash_call("a", "git status");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = bash_call("b", "git add -A");
        d.feed(&make_ctx(&r2, 0, None));

        let r3 = bash_call("c", "git commit -m 'fix'");
        d.feed(&make_ctx(&r3, 0, None));

        // Non-git event triggers emit
        let r4 = tool_call("d", "Read", "toolu_d");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].pattern_type, "git.workflow");
        assert_eq!(r[0].metadata["length"], 3);
        assert_eq!(r[0].metadata["has_status"], true);
        assert_eq!(r[0].metadata["has_add"], true);
        assert_eq!(r[0].metadata["has_commit"], true);
    }

    #[test]
    fn it_should_not_emit_for_single_git_command() {
        let mut d = GitFlowDetector::new();

        let r1 = bash_call("a", "git status");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = tool_call("b", "Read", "toolu_b");
        let r = d.feed(&make_ctx(&r2, 0, None));
        assert!(r.is_empty(), "single git command should not emit workflow");
    }

    #[test]
    fn it_should_flush_pending_sequence() {
        let mut d = GitFlowDetector::new();

        let r1 = bash_call("a", "git add .");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = bash_call("b", "git push origin main");
        d.feed(&make_ctx(&r2, 0, None));

        let r = d.flush();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["has_push"], true);
    }

    #[test]
    fn it_should_not_trigger_on_non_git_bash() {
        let mut d = GitFlowDetector::new();

        let r1 = bash_call("a", "cargo test");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = bash_call("b", "npm install");
        d.feed(&make_ctx(&r2, 0, None));

        let r = d.flush();
        assert!(r.is_empty());
    }

    #[test]
    fn it_should_include_verbs_in_summary() {
        let mut d = GitFlowDetector::new();

        let r1 = bash_call("a", "git status");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = bash_call("b", "git commit -m 'done'");
        d.feed(&make_ctx(&r2, 0, None));

        let r = d.flush();
        assert_eq!(r[0].summary, "status -> commit");
    }
}

// ═══════════════════════════════════════════════════════════════════
// ErrorRecoveryDetector
// ═══════════════════════════════════════════════════════════════════

mod error_recovery {
    use super::*;

    #[test]
    fn it_should_detect_error_then_reasoning_then_retry_success() {
        let mut d = ErrorRecoveryDetector::new();

        let r1 = error_result("a", "toolu_x", "error: cannot find module 'foo'");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = reasoning("b");
        d.feed(&make_ctx(&r2, 0, None));

        let r3 = tool_call("c", "Edit", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        let r4 = tool_result("d", "toolu_c", "file updated successfully");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].pattern_type, "error.recovery");
        assert_eq!(r[0].metadata["recovered"], true);
        assert_eq!(r[0].metadata["retries"], 1);
    }

    #[test]
    fn it_should_detect_error_with_assistant_text_then_retry() {
        let mut d = ErrorRecoveryDetector::new();

        let r1 = error_result("a", "toolu_x", "error: type mismatch");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = assistant_text("b");
        d.feed(&make_ctx(&r2, 0, None));

        let r3 = tool_call("c", "Edit", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        let r4 = tool_result("d", "toolu_c", "OK");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["recovered"], true);
    }

    #[test]
    fn it_should_handle_double_error_then_success() {
        let mut d = ErrorRecoveryDetector::new();

        // First error
        let r1 = error_result("a", "toolu_x", "error: foo");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = reasoning("b");
        d.feed(&make_ctx(&r2, 0, None));
        let r3 = tool_call("c", "Edit", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        // Still fails
        let r4 = error_result("d", "toolu_c", "error: bar");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert!(r.is_empty(), "double error should not emit yet");

        // Now reason and retry again
        let r5 = reasoning("e");
        d.feed(&make_ctx(&r5, 0, None));
        let r6 = tool_call("f", "Edit", "toolu_f");
        d.feed(&make_ctx(&r6, 0, None));
        let r7 = tool_result("g", "toolu_f", "OK");
        let r = d.feed(&make_ctx(&r7, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["recovered"], true);
        assert_eq!(r[0].metadata["retries"], 2);
    }

    #[test]
    fn it_should_reset_on_user_message_without_emit_if_no_retries() {
        let mut d = ErrorRecoveryDetector::new();

        let r1 = error_result("a", "toolu_x", "error: bad");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = user_message("b");
        let r = d.feed(&make_ctx(&r2, 0, None));
        assert!(r.is_empty(), "no emit if no retries attempted");
    }

    #[test]
    fn it_should_flush_incomplete_recovery_as_failed() {
        let mut d = ErrorRecoveryDetector::new();

        let r1 = error_result("a", "toolu_x", "error: bad");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = reasoning("b");
        d.feed(&make_ctx(&r2, 0, None));
        let r3 = tool_call("c", "Edit", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        let r = d.flush();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["recovered"], false);
    }
}

// ═══════════════════════════════════════════════════════════════════
// AgentDelegationDetector
// ═══════════════════════════════════════════════════════════════════

mod agent_delegation {
    use super::*;

    #[test]
    fn it_should_detect_agent_call_with_subtree_and_result() {
        let mut d = AgentDelegationDetector::new();

        // Agent call at depth 0
        let r1 = agent_call("a", "call_agent", "Search for the bug");
        d.feed(&make_ctx(&r1, 0, None));

        // Subtree events at depth > 0
        let r2 = tool_call("b", "Read", "toolu_b");
        d.feed(&make_ctx(&r2, 1, Some("a")));
        let r3 = tool_call("c", "Grep", "toolu_c");
        d.feed(&make_ctx(&r3, 1, Some("a")));

        // Agent result at depth 0
        let r4 = tool_result("d", "call_agent", "Agent completed");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].pattern_type, "agent.delegation");
        assert_eq!(r[0].metadata["total_events"], 3); // agent + 2 subtree
    }

    #[test]
    fn it_should_track_tools_used_in_subtree() {
        let mut d = AgentDelegationDetector::new();

        let r1 = agent_call("a", "call_a", "Fix it");
        d.feed(&make_ctx(&r1, 0, None));

        let r2 = tool_call("b", "Read", "toolu_b");
        d.feed(&make_ctx(&r2, 1, Some("a")));
        let r3 = tool_call("c", "Read", "toolu_c");
        d.feed(&make_ctx(&r3, 1, Some("a")));
        let r4 = tool_call("d", "Edit", "toolu_d");
        d.feed(&make_ctx(&r4, 1, Some("a")));

        let r5 = tool_result("e", "call_a", "done");
        let r = d.feed(&make_ctx(&r5, 0, None));
        let tools = r[0].metadata["tools_used"].as_object().unwrap();
        assert_eq!(tools["Read"], 2);
        assert_eq!(tools["Edit"], 1);
    }

    #[test]
    fn it_should_not_emit_for_incomplete_agent() {
        let mut d = AgentDelegationDetector::new();

        let r1 = agent_call("a", "call_a", "Do stuff");
        d.feed(&make_ctx(&r1, 0, None));

        // No result — flush should be empty
        let r = d.flush();
        assert!(r.is_empty());
    }

    #[test]
    fn it_should_ignore_events_at_same_depth_as_agent() {
        let mut d = AgentDelegationDetector::new();

        let r1 = agent_call("a", "call_a", "Do stuff");
        d.feed(&make_ctx(&r1, 2, None));

        // Same depth (2) — should NOT count as subtree
        let r2 = tool_call("b", "Read", "toolu_b");
        d.feed(&make_ctx(&r2, 2, Some("a")));

        let r3 = tool_result("c", "call_a", "done");
        let r = d.feed(&make_ctx(&r3, 2, None));
        assert_eq!(r[0].metadata["total_events"], 1); // only the agent call itself
    }
}

// ═══════════════════════════════════════════════════════════════════
// TurnPhaseDetector
// ═══════════════════════════════════════════════════════════════════

mod turn_phase {
    use super::*;

    #[test]
    fn it_should_classify_conversation_turn_with_no_tools() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = assistant_text("b");
        d.feed(&make_ctx(&r2, 0, None));

        // New user message triggers emit of previous turn
        let r3 = user_message("c");
        let r = d.feed(&make_ctx(&r3, 0, None));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].metadata["phase"], "conversation");
    }

    #[test]
    fn it_should_classify_exploration_turn() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = tool_call("b", "Read", "toolu_b");
        d.feed(&make_ctx(&r2, 0, None));
        let r3 = tool_call("c", "Grep", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));
        let r4 = tool_call("d", "Glob", "toolu_d");
        d.feed(&make_ctx(&r4, 0, None));

        let r5 = user_message("e");
        let r = d.feed(&make_ctx(&r5, 0, None));
        assert_eq!(r[0].metadata["phase"], "exploration");
    }

    #[test]
    fn it_should_classify_implementation_turn() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = tool_call("b", "Edit", "toolu_b");
        d.feed(&make_ctx(&r2, 0, None));
        let r3 = tool_call("c", "Write", "toolu_c");
        d.feed(&make_ctx(&r3, 0, None));

        let r4 = user_message("d");
        let r = d.feed(&make_ctx(&r4, 0, None));
        assert_eq!(r[0].metadata["phase"], "implementation");
    }

    #[test]
    fn it_should_classify_delegation_turn() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = agent_call("b", "call_b", "go");
        d.feed(&make_ctx(&r2, 0, None));

        let r3 = user_message("c");
        let r = d.feed(&make_ctx(&r3, 0, None));
        assert_eq!(r[0].metadata["phase"], "delegation");
    }

    #[test]
    fn it_should_flush_final_turn() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = assistant_text("b");
        d.feed(&make_ctx(&r2, 0, None));

        let r = d.flush();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].pattern_type, "turn.phase");
    }

    #[test]
    fn it_should_increment_turn_number() {
        let mut d = TurnPhaseDetector::new();

        let r1 = user_message("a");
        d.feed(&make_ctx(&r1, 0, None));
        let r2 = assistant_text("b");
        d.feed(&make_ctx(&r2, 0, None));

        let r3 = user_message("c");
        let r = d.feed(&make_ctx(&r3, 0, None));
        assert_eq!(r[0].metadata["turn"], 0);

        let r4 = assistant_text("d");
        d.feed(&make_ctx(&r4, 0, None));

        let r5 = user_message("e");
        let r = d.feed(&make_ctx(&r5, 0, None));
        assert_eq!(r[0].metadata["turn"], 1);
    }
}

// ═══════════════════════════════════════════════════════════════════
// PatternPipeline
// ═══════════════════════════════════════════════════════════════════

mod pipeline {
    use super::*;

    #[test]
    fn it_should_fan_out_to_all_detectors() {
        let mut pipeline = PatternPipeline::new();

        // Feed a user message + assistant + user message
        // This should trigger TurnPhaseDetector at minimum
        let r1 = user_message("a");
        pipeline.feed(&make_ctx(&r1, 0, None));
        let r2 = assistant_text("b");
        pipeline.feed(&make_ctx(&r2, 0, None));

        let r3 = user_message("c");
        let r = pipeline.feed(&make_ctx(&r3, 0, None));
        // TurnPhaseDetector should have emitted
        assert!(r.iter().any(|p| p.pattern_type == "turn.phase"));
    }

    #[test]
    fn it_should_flush_all_detectors() {
        let mut pipeline = PatternPipeline::new();

        let r1 = user_message("a");
        pipeline.feed(&make_ctx(&r1, 0, None));
        let r2 = assistant_text("b");
        pipeline.feed(&make_ctx(&r2, 0, None));

        let (patterns, _turns) = pipeline.flush();
        // At least TurnPhaseDetector should flush
        assert!(!patterns.is_empty());
    }
}
