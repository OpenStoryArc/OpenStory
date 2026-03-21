//! GitFlowDetector — detects sequences of git commands.
//!
//! Accumulates consecutive git Bash commands. When a non-git event arrives,
//! emits git.workflow if 2+ git commands were seen.

use open_story_views::unified::RecordBody;
use open_story_views::tool_input::ToolInput;

use crate::{Detector, FeedContext, PatternEvent};

/// Detects sequences of 2+ git commands (status → add → commit → push).
#[derive(Default)]
pub struct GitFlowDetector {
    commands: Vec<String>,
    node_ids: Vec<String>,
    timestamps: Vec<String>,
    session_id: String,
}

impl GitFlowDetector {
    pub fn new() -> Self {
        GitFlowDetector {
            commands: Vec::new(),
            node_ids: Vec::new(),
            timestamps: Vec::new(),
            session_id: String::new(),
        }
    }

    fn is_git_command(record: &open_story_views::view_record::ViewRecord) -> Option<String> {
        if let RecordBody::ToolCall(tc) = &record.body {
            if let Some(ToolInput::Bash(b)) = tc.typed_input.as_ref() {
                let trimmed = b.command.trim();
                if trimmed.starts_with("git ") || trimmed == "git" {
                    return Some(b.command.clone());
                }
            }
        }
        None
    }

    fn is_tool_call(record: &open_story_views::view_record::ViewRecord) -> bool {
        matches!(record.body, RecordBody::ToolCall(_))
    }

    fn git_verb(command: &str) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "git" {
            parts[1].to_string()
        } else {
            "?".to_string()
        }
    }

    fn emit(&self) -> Option<PatternEvent> {
        if self.commands.len() < 2 {
            return None;
        }
        let verbs: Vec<String> = self.commands.iter().map(|c| Self::git_verb(c)).collect();
        let summary = verbs.join(" -> ");
        Some(PatternEvent {
            pattern_type: "git.workflow".into(),
            session_id: self.session_id.clone(),
            event_ids: self.node_ids.clone(),
            started_at: self.timestamps.first().cloned().unwrap_or_default(),
            ended_at: self.timestamps.last().cloned().unwrap_or_default(),
            summary,
            metadata: serde_json::json!({
                "commands": self.commands,
                "verbs": verbs,
                "has_status": verbs.contains(&"status".to_string()),
                "has_add": verbs.contains(&"add".to_string()),
                "has_commit": verbs.contains(&"commit".to_string()),
                "has_push": verbs.contains(&"push".to_string()),
                "length": self.commands.len(),
            }),
        })
    }

    fn clear(&mut self) {
        self.commands.clear();
        self.node_ids.clear();
        self.timestamps.clear();
    }
}

impl Detector for GitFlowDetector {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        self.session_id = record.session_id.clone();
        let mut results = Vec::new();

        if Self::is_tool_call(record) {
            if let Some(cmd) = Self::is_git_command(record) {
                self.commands.push(cmd);
                self.node_ids.push(record.id.clone());
                self.timestamps.push(record.timestamp.clone());
                return results;
            }
        }

        // Non-git event: flush accumulated commands
        if !self.commands.is_empty() {
            if let Some(evt) = self.emit() {
                results.push(evt);
            }
            self.clear();
        }

        results
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        if let Some(evt) = self.emit() {
            self.clear();
            vec![evt]
        } else {
            vec![]
        }
    }

    fn name(&self) -> &str {
        "git.workflow"
    }
}
