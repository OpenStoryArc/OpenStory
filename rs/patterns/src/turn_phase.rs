//! TurnPhaseDetector — classifies each turn by tool usage pattern.
//!
//! Accumulates events until a user_message boundary, then classifies the turn:
//! conversation, exploration, implementation, implementation+testing,
//! testing, execution, delegation, mixed.

use std::collections::HashMap;

use open_story_views::unified::RecordBody;

use crate::{Detector, FeedContext, PatternEvent};

/// Classifies conversation turns by their tool usage patterns.
#[derive(Default)]
pub struct TurnPhaseDetector {
    current_turn: Vec<TurnEvent>,
    turn_number: usize,
    session_id: String,
}

/// Minimal info tracked per event in the current turn.
struct TurnEvent {
    id: String,
    timestamp: String,
    tool_name: Option<String>,
}

impl TurnPhaseDetector {
    pub fn new() -> Self {
        TurnPhaseDetector {
            current_turn: Vec::new(),
            turn_number: 0,
            session_id: String::new(),
        }
    }

    fn classify(tools: &HashMap<String, usize>) -> &'static str {
        let tool_set: std::collections::HashSet<&str> =
            tools.keys().map(|s| s.as_str()).collect();

        if tool_set.is_empty() {
            return "conversation";
        }

        let explore: std::collections::HashSet<&str> =
            ["Read", "Grep", "Glob"].into_iter().collect();
        let explore_and_bash: std::collections::HashSet<&str> =
            ["Read", "Grep", "Glob", "Bash"].into_iter().collect();

        if tool_set.is_subset(&explore_and_bash) {
            let explore_count: usize = explore.iter()
                .filter_map(|t| tools.get(*t))
                .sum();
            let bash_count = tools.get("Bash").copied().unwrap_or(0);
            if explore_count > bash_count {
                return "exploration";
            }
        }

        if tool_set.contains("Edit") || tool_set.contains("Write") {
            let edit_count = tools.get("Edit").copied().unwrap_or(0)
                + tools.get("Write").copied().unwrap_or(0);
            let bash_count = tools.get("Bash").copied().unwrap_or(0);
            if bash_count > edit_count {
                return "implementation+testing";
            }
            return "implementation";
        }

        if tool_set.contains("Agent") {
            return "delegation";
        }

        if tool_set.contains("Bash") {
            let bash_count = tools.get("Bash").copied().unwrap_or(0);
            if bash_count > 5 {
                return "testing";
            }
            return "execution";
        }

        "mixed"
    }

    fn emit(&self) -> Option<PatternEvent> {
        if self.current_turn.is_empty() {
            return None;
        }

        let mut tools: HashMap<String, usize> = HashMap::new();
        for ev in &self.current_turn {
            if let Some(name) = &ev.tool_name {
                *tools.entry(name.clone()).or_insert(0) += 1;
            }
        }

        let phase = Self::classify(&tools);

        Some(PatternEvent {
            pattern_type: "turn.phase".into(),
            session_id: self.session_id.clone(),
            event_ids: self.current_turn.iter().map(|e| e.id.clone()).collect(),
            started_at: self.current_turn.first().map(|e| e.timestamp.clone()).unwrap_or_default(),
            ended_at: self.current_turn.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
            summary: format!(
                "Turn {}: {} ({} events)",
                self.turn_number,
                phase,
                self.current_turn.len(),
            ),
            metadata: serde_json::json!({
                "turn": self.turn_number,
                "phase": phase,
                "tools": tools,
                "events": self.current_turn.len(),
            }),
        })
    }
}

impl Detector for TurnPhaseDetector {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        self.session_id = record.session_id.clone();
        let mut results = Vec::new();

        let is_user = matches!(record.body, RecordBody::UserMessage(_));

        if is_user && !self.current_turn.is_empty() {
            if let Some(evt) = self.emit() {
                results.push(evt);
            }
            self.current_turn.clear();
            self.turn_number += 1;
        }

        let tool_name = match &record.body {
            RecordBody::ToolCall(tc) => Some(tc.name.clone()),
            _ => None,
        };

        self.current_turn.push(TurnEvent {
            id: record.id.clone(),
            timestamp: record.timestamp.clone(),
            tool_name,
        });

        results
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        if let Some(evt) = self.emit() {
            self.current_turn.clear();
            vec![evt]
        } else {
            vec![]
        }
    }

    fn name(&self) -> &str {
        "turn.phase"
    }
}
