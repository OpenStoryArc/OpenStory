//! ErrorRecoveryDetector — detects error → reasoning → retry → success chains.
//!
//! State machine: idle → saw_error → recovering → retrying
//! Uses tree context to match retry tool to the original error's parent tool.

use open_story_views::unified::RecordBody;

use crate::{Detector, FeedContext, PatternEvent};

/// Detects error recovery patterns: tool error → reasoning → retry same tool → success.
#[derive(Default)]
pub struct ErrorRecoveryDetector {
    state: State,
    error_id: String,
    error_timestamp: String,
    retries: usize,
    all_ids: Vec<String>,
    last_tool: Option<String>,
    session_id: String,
}

#[derive(Debug, Default, PartialEq)]
enum State {
    #[default]
    Idle,
    SawError,
    Recovering,
    Retrying,
}

impl ErrorRecoveryDetector {
    pub fn new() -> Self {
        ErrorRecoveryDetector {
            state: State::Idle,
            error_id: String::new(),
            error_timestamp: String::new(),
            retries: 0,
            all_ids: Vec::new(),
            last_tool: None,
            session_id: String::new(),
        }
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.error_id.clear();
        self.error_timestamp.clear();
        self.retries = 0;
        self.all_ids.clear();
        self.last_tool = None;
    }

    fn is_error_result(record: &open_story_views::view_record::ViewRecord) -> bool {
        if let RecordBody::ToolResult(tr) = &record.body {
            if tr.is_error {
                return true;
            }
            if let Some(output) = &tr.output {
                // Find a valid char boundary at or before byte 200
                let end = {
                    let max = output.len().min(200);
                    let mut i = max;
                    while i > 0 && !output.is_char_boundary(i) {
                        i -= 1;
                    }
                    i
                };
                let lower = output[..end].to_lowercase();
                return lower.contains("error");
            }
        }
        false
    }

    fn is_reasoning_or_text(record: &open_story_views::view_record::ViewRecord) -> bool {
        matches!(
            record.body,
            RecordBody::Reasoning(_) | RecordBody::AssistantMessage(_)
        )
    }

    fn is_user_message(record: &open_story_views::view_record::ViewRecord) -> bool {
        matches!(record.body, RecordBody::UserMessage(_))
    }

    fn tool_name(record: &open_story_views::view_record::ViewRecord) -> Option<&str> {
        match &record.body {
            RecordBody::ToolCall(tc) => Some(&tc.name),
            _ => None,
        }
    }

    fn is_tool_result(record: &open_story_views::view_record::ViewRecord) -> bool {
        matches!(record.body, RecordBody::ToolResult(_))
    }

    fn emit(&self, recovered: bool) -> PatternEvent {
        let tool = self.last_tool.as_deref().unwrap_or("unknown");
        PatternEvent {
            pattern_type: "error.recovery".into(),
            session_id: self.session_id.clone(),
            event_ids: self.all_ids.clone(),
            started_at: self.error_timestamp.clone(),
            ended_at: self.all_ids.last().cloned().unwrap_or_default(),
            summary: format!(
                "{} after {} retries ({})",
                if recovered { "Recovered" } else { "Failed" },
                self.retries,
                tool,
            ),
            metadata: serde_json::json!({
                "recovered": recovered,
                "retries": self.retries,
                "tool": tool,
            }),
        }
    }
}

impl Detector for ErrorRecoveryDetector {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        self.session_id = record.session_id.clone();
        let mut results = Vec::new();

        match self.state {
            State::Idle => {
                if Self::is_error_result(record) {
                    self.state = State::SawError;
                    self.error_id = record.id.clone();
                    self.error_timestamp = record.timestamp.clone();
                    self.all_ids = vec![record.id.clone()];
                    self.retries = 0;
                    // The tool that errored is identified by the call_id pairing,
                    // but we approximate by looking at what tool name matches
                    // in subsequent retry attempts
                    self.last_tool = None;
                }
            }
            State::SawError => {
                self.all_ids.push(record.id.clone());
                if Self::is_reasoning_or_text(record) {
                    self.state = State::Recovering;
                } else if Self::is_user_message(record) {
                    self.reset();
                }
            }
            State::Recovering => {
                self.all_ids.push(record.id.clone());
                if let Some(tool) = Self::tool_name(record) {
                    // First retry sets the tool name; subsequent retries must match
                    if self.last_tool.is_none() || self.last_tool.as_deref() == Some(tool) {
                        self.last_tool = Some(tool.to_string());
                        self.retries += 1;
                        self.state = State::Retrying;
                    }
                } else if Self::is_user_message(record) {
                    if self.retries > 0 {
                        results.push(self.emit(false));
                    }
                    self.reset();
                }
            }
            State::Retrying => {
                self.all_ids.push(record.id.clone());
                if Self::is_error_result(record) {
                    // Still failing — back to saw_error
                    self.state = State::SawError;
                } else if Self::is_tool_result(record) {
                    // Success!
                    results.push(self.emit(true));
                    self.reset();
                } else if Self::is_user_message(record) {
                    results.push(self.emit(false));
                    self.reset();
                }
            }
        }

        results
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        if self.state != State::Idle && self.retries > 0 {
            vec![self.emit(false)]
        } else {
            vec![]
        }
    }

    fn name(&self) -> &str {
        "error.recovery"
    }
}
