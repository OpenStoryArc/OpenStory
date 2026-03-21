//! AgentDelegationDetector — tracks Agent tool_use → subtree → tool_result.
//!
//! Uses depth from FeedContext to determine which events belong to the agent's subtree.
//! Emits agent.delegation when the matching tool_result arrives.

use std::collections::HashMap;

use open_story_views::unified::RecordBody;

use crate::{Detector, FeedContext, PatternEvent};

/// Tracks pending Agent tool calls and their subtree activity.
struct PendingAgent {
    start_timestamp: String,
    start_depth: u16,
    description: String,
    tools: HashMap<String, usize>,
    total_events: usize,
    ids: Vec<String>,
}

/// Detects agent delegation patterns: Agent tool_use → subtree work → tool_result.
#[derive(Default)]
pub struct AgentDelegationDetector {
    /// Pending agents keyed by call_id.
    pending: HashMap<String, PendingAgent>,
    session_id: String,
}

impl AgentDelegationDetector {
    pub fn new() -> Self {
        AgentDelegationDetector {
            pending: HashMap::new(),
            session_id: String::new(),
        }
    }
}

impl Detector for AgentDelegationDetector {
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        self.session_id = record.session_id.clone();
        let mut results = Vec::new();

        match &record.body {
            RecordBody::ToolCall(tc) if tc.name == "Agent" => {
                let description = tc.raw_input
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                self.pending.insert(tc.call_id.clone(), PendingAgent {
                    start_timestamp: record.timestamp.clone(),
                    start_depth: ctx.depth,
                    description,
                    tools: HashMap::new(),
                    total_events: 1,
                    ids: vec![record.id.clone()],
                });
            }
            RecordBody::ToolResult(tr) if self.pending.contains_key(&tr.call_id) => {
                let info = self.pending.remove(&tr.call_id).unwrap();
                let mut ids = info.ids;
                ids.push(record.id.clone());
                results.push(PatternEvent {
                    pattern_type: "agent.delegation".into(),
                    session_id: self.session_id.clone(),
                    event_ids: ids,
                    started_at: info.start_timestamp,
                    ended_at: record.timestamp.clone(),
                    summary: format!(
                        "Agent '{}' -> {:?}",
                        &info.description[..info.description.len().min(40)],
                        info.tools,
                    ),
                    metadata: serde_json::json!({
                        "description": info.description,
                        "tools_used": info.tools,
                        "total_events": info.total_events,
                    }),
                });
            }
            _ => {
                // Track tools used inside any pending agent's subtree
                for info in self.pending.values_mut() {
                    if ctx.depth > info.start_depth {
                        if let RecordBody::ToolCall(tc) = &record.body {
                            *info.tools.entry(tc.name.clone()).or_insert(0) += 1;
                        }
                        info.total_events += 1;
                        info.ids.push(record.id.clone());
                    }
                }
            }
        }

        results
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        // Incomplete agent delegations are not emitted
        vec![]
    }

    fn name(&self) -> &str {
        "agent.delegation"
    }
}
