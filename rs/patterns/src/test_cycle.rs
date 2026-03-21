//! TestCycleDetector — detects red-green TDD cycles.
//!
//! State machine: idle → saw_test → saw_fail → editing → (saw_test again)
//! Emits test.cycle when cycle completes (pass or fail) or is interrupted.

use open_story_views::unified::RecordBody;
use open_story_views::tool_input::ToolInput;

use crate::{Detector, FeedContext, PatternEvent};

/// Detects red-green test cycles: test → fail → edit → retest → pass/fail.
#[derive(Default)]
pub struct TestCycleDetector {
    state: State,
    test_id: String,
    test_timestamp: String,
    test_command: String,
    fail_id: String,
    edit_count: usize,
    iterations: usize,
    all_ids: Vec<String>,
    session_id: String,
}

#[derive(Debug, Default, PartialEq)]
enum State {
    #[default]
    Idle,
    SawTest,
    SawFail,
    Editing,
}

impl TestCycleDetector {
    pub fn new() -> Self {
        TestCycleDetector {
            state: State::Idle,
            test_id: String::new(),
            test_timestamp: String::new(),
            test_command: String::new(),
            fail_id: String::new(),
            edit_count: 0,
            iterations: 0,
            all_ids: Vec::new(),
            session_id: String::new(),
        }
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.test_id.clear();
        self.test_timestamp.clear();
        self.test_command.clear();
        self.fail_id.clear();
        self.edit_count = 0;
        self.iterations = 0;
        self.all_ids.clear();
    }

    fn is_test_command(record: &open_story_views::view_record::ViewRecord) -> Option<String> {
        if let RecordBody::ToolCall(tc) = &record.body {
            if let Some(ToolInput::Bash(b)) = tc.typed_input.as_ref() {
                let cmd = b.command.to_lowercase();
                let keywords = [
                    "cargo test", "npm test", "npx playwright", "pytest",
                    "vitest", "jest", "npm run test", "uv run pytest",
                ];
                if keywords.iter().any(|kw| cmd.contains(kw)) {
                    return Some(b.command.clone());
                }
            }
        }
        None
    }

    fn is_test_failure(record: &open_story_views::view_record::ViewRecord) -> bool {
        if let RecordBody::ToolResult(tr) = &record.body {
            if let Some(output) = &tr.output {
                let lower = output.to_lowercase();
                return ["failed", "failure", "panicked", "exit code 1", "exit code 2", "assertion"]
                    .iter()
                    .any(|kw| lower.contains(kw));
            }
        }
        false
    }

    fn is_test_pass(record: &open_story_views::view_record::ViewRecord) -> bool {
        if let RecordBody::ToolResult(tr) = &record.body {
            if let Some(output) = &tr.output {
                let lower = output.to_lowercase();
                let has_pass = ["passed", "ok", "exit code 0", "test result: ok", "0 failed"]
                    .iter()
                    .any(|kw| lower.contains(kw));
                let has_fail = ["failed", "failure", "panicked", "exit code 1", "exit code 2"]
                    .iter()
                    .any(|kw| lower.contains(kw));
                return has_pass && !has_fail;
            }
        }
        false
    }

    fn is_edit(record: &open_story_views::view_record::ViewRecord) -> bool {
        match &record.body {
            RecordBody::ToolCall(tc) => tc.name == "Edit" || tc.name == "Write",
            _ => false,
        }
    }

    fn is_user_message(record: &open_story_views::view_record::ViewRecord) -> bool {
        matches!(record.body, RecordBody::UserMessage(_))
    }

    fn emit(&self, passed: bool) -> PatternEvent {
        let summary = format!(
            "{} after {} iteration(s), {} edit(s)",
            if passed { "PASS" } else { "FAIL" },
            self.iterations,
            self.edit_count,
        );
        PatternEvent {
            pattern_type: "test.cycle".into(),
            session_id: self.session_id.clone(),
            event_ids: self.all_ids.clone(),
            started_at: self.test_timestamp.clone(),
            ended_at: self.all_ids.last().cloned().unwrap_or_default(),
            summary,
            metadata: serde_json::json!({
                "passed": passed,
                "iterations": self.iterations,
                "edits": self.edit_count,
                "test_command": &self.test_command[..self.test_command.len().min(60)],
            }),
        }
    }
}

impl Detector for TestCycleDetector {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        self.session_id = record.session_id.clone();
        let mut results = Vec::new();

        match self.state {
            State::Idle => {
                if let Some(cmd) = Self::is_test_command(record) {
                    self.state = State::SawTest;
                    self.test_id = record.id.clone();
                    self.test_timestamp = record.timestamp.clone();
                    self.test_command = cmd;
                    self.all_ids = vec![record.id.clone()];
                    self.iterations = 1;
                }
            }
            State::SawTest => {
                self.all_ids.push(record.id.clone());
                if Self::is_test_failure(record) {
                    self.state = State::SawFail;
                    self.fail_id = record.id.clone();
                } else if Self::is_test_pass(record) {
                    results.push(self.emit(true));
                    self.reset();
                } else if Self::is_user_message(record) {
                    self.reset();
                }
            }
            State::SawFail => {
                self.all_ids.push(record.id.clone());
                if Self::is_edit(record) {
                    self.state = State::Editing;
                    self.edit_count += 1;
                } else if Self::is_user_message(record) {
                    results.push(self.emit(false));
                    self.reset();
                }
            }
            State::Editing => {
                self.all_ids.push(record.id.clone());
                if Self::is_edit(record) {
                    self.edit_count += 1;
                } else if let Some(cmd) = Self::is_test_command(record) {
                    self.state = State::SawTest;
                    self.test_id = record.id.clone();
                    self.test_command = cmd;
                    self.iterations += 1;
                } else if Self::is_user_message(record) {
                    results.push(self.emit(false));
                    self.reset();
                }
            }
        }

        results
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        if self.state != State::Idle && !self.test_id.is_empty() {
            vec![self.emit(false)]
        } else {
            vec![]
        }
    }

    fn name(&self) -> &str {
        "test.cycle"
    }
}
