//! EvalApplyDetector — surfaces the computational structure of agent sessions.
//!
//! The agent loop is SICP's metacircular evaluator:
//!   eval  = model call (AssistantMessage)
//!   apply = tool dispatch (ToolCall → ToolResult)
//!   env   = conversation history (growing list of messages)
//!
//! Two input paths:
//!   - CloudEvents (primary): feed_cloud_event() — typed payload access
//!   - ViewRecords (legacy): Detector::feed() — backward compat
//!
//! Produces:
//!   - PatternEvents (eval_apply.eval, .apply, .turn_end, etc.)
//!   - StructuralTurns — the intermediate representation for downstream detectors
//!
//! Scheme parallel: 04-eval-apply.scm (agent-step, run-agent-loop)
//! Prototype: docs/research/eval-apply-prototype/eval-apply-detector.ts

use serde::{Deserialize, Serialize};

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::ToolOutcome;
use open_story_views::unified::RecordBody;

use crate::{Detector, FeedContext, PatternEvent};

// ═══════════════════════════════════════════════════════════════════
// StructuralTurn — the intermediate representation
// ═══════════════════════════════════════════════════════════════════
//
// One eval-apply step, aggregated. The natural unit of agent behavior.
// Downstream detectors (SentenceDetector, etc.) consume these.
//
// Prototype source: types.ts:127-157

/// One step of the eval-apply coalgebra, fully resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralTurn {
    pub session_id: String,
    pub turn_number: u32,
    pub scope_depth: u32,
    pub human: Option<HumanInput>,
    pub thinking: Option<ThinkingRecord>,
    pub eval: Option<EvalOutput>,
    pub applies: Vec<ApplyRecord>,
    pub env_size: u32,
    pub env_delta: u32,
    pub stop_reason: String,
    pub is_terminal: bool,
    pub timestamp: String,
    pub duration_ms: Option<f64>,
    pub event_ids: Vec<String>,
}

/// The human message that prompted this turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanInput {
    pub content: String,
    pub timestamp: String,
}

/// The model's reasoning (thinking phase).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingRecord {
    pub summary: String,
}

/// The model's response (eval phase output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalOutput {
    pub content: String,
    pub timestamp: String,
    pub stop_reason: Option<String>,
    pub decision: String,
}

/// One tool dispatch (apply phase).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyRecord {
    pub tool_name: String,
    pub input_summary: String,
    pub output_summary: String,
    pub is_error: bool,
    pub is_agent: bool,
    pub tool_outcome: Option<ToolOutcome>,
}

// ═══════════════════════════════════════════════════════════════════
// Accumulator — the state threaded through the fold
// ═══════════════════════════════════════════════════════════════════
//
// In the Scheme prototype, the environment is a list of messages.
// Here, the Accumulator is the full state of the eval-apply observer:
// what turn we're in, what we've seen so far, what we're waiting for.
//
// The fold function `step()` takes (Accumulator, CloudEvent) and returns
// a StepResult: either Continue (new accumulator) or TurnComplete
// (new accumulator + completed turn + pattern events).

/// A pending tool dispatch awaiting its result.
#[derive(Debug, Clone)]
pub struct PendingApply {
    pub tool_name: String,
    pub input_summary: String,
    pub is_agent: bool,
}

/// The accumulator threaded through the pure fold.
/// All state the fold needs lives here — nothing in ambient `self`.
#[derive(Debug, Clone)]
pub struct Accumulator {
    pub phase: Phase,
    pub turn_number: u32,
    pub scope_depth: u32,
    pub env_size: u32,
    pub session_id: String,
    /// Turn being assembled.
    pub pending_human: Option<HumanInput>,
    pub pending_thinking: Option<ThinkingRecord>,
    pub pending_eval: Option<EvalOutput>,
    pub completed_applies: Vec<ApplyRecord>,
    pub pending_applies: Vec<PendingApply>,
    pub event_ids: Vec<String>,
    pub start_ts: Option<String>,
    pub env_size_at_start: u32,
}

/// Phase of the state machine.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    SawEval,
    SawApply,
    ResultsReady,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            turn_number: 0,
            scope_depth: 0,
            env_size: 0,
            session_id: String::new(),
            pending_human: None,
            pending_thinking: None,
            pending_eval: None,
            completed_applies: Vec::new(),
            pending_applies: Vec::new(),
            event_ids: Vec::new(),
            start_ts: None,
            env_size_at_start: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// StepResult — the output of the pure fold
// ═══════════════════════════════════════════════════════════════════

/// The result of one fold step.
/// Either we continue accumulating, or a turn is complete.
pub enum StepResult {
    /// The fold continues — no turn boundary yet.
    Continue {
        acc: Accumulator,
        patterns: Vec<PatternEvent>,
    },
    /// A coalgebra step completed — a turn crystallized.
    TurnComplete {
        acc: Accumulator,
        turn: StructuralTurn,
        patterns: Vec<PatternEvent>,
    },
}

// ═══════════════════════════════════════════════════════════════════
// step() — the pure fold function
// ═══════════════════════════════════════════════════════════════════
//
// (Accumulator, CloudEvent) → StepResult
//
// This is `agent-step` from 04-eval-apply.scm, adapted for observation.
// The Scheme version takes (env, model, tools) → Either<Outcome, NewEnv>.
// Ours takes (accumulator, event) → Either<Continue, TurnComplete>.
//
// Pure: same accumulator + same event = same result. Always.

/// One step of the eval-apply fold.
/// Pure function: no side effects, no I/O, no ambient state.
pub fn step(mut acc: Accumulator, event: &CloudEvent) -> StepResult {
    let id = event.id.as_str();
    let ts = event.time.as_str();
    let subtype = event.subtype.as_deref().unwrap_or("");
    let ap = event.data.agent_payload.as_ref();
    let mut patterns = Vec::new();

    // Track session
    if acc.session_id.is_empty() {
        acc.session_id = event.data.session_id.clone();
    }

    // Track event IDs and timestamps
    acc.event_ids.push(id.to_string());
    if acc.start_ts.is_none() {
        acc.start_ts = Some(ts.to_string());
        acc.env_size_at_start = acc.env_size;
    }

    match subtype {
        // ── Thinking: reasoning before responding ──
        "message.assistant.thinking" => {
            let summary = ap.and_then(|p| p.text()).unwrap_or("").to_string();
            acc.pending_thinking = Some(ThinkingRecord { summary });
        }

        // ── Human prompt: the input to eval ──
        s if s.starts_with("message.user.prompt") => {
            acc.env_size += 1;
            let content = ap.and_then(|p| p.text()).unwrap_or("").to_string();
            acc.pending_human = Some(HumanInput {
                content,
                timestamp: ts.to_string(),
            });
        }

        // ── Tool result: apply phase complete ──
        "message.user.tool_result" => {
            acc.env_size += 1;
            // Resolve the first pending apply with this result
            if let Some(pending) = acc.pending_applies.first().cloned() {
                acc.pending_applies.remove(0);
                let output_summary = ap.and_then(|p| p.text()).unwrap_or("").to_string();
                let tool_outcome = ap.and_then(|p| p.tool_outcome()).cloned();
                // Derive is_error from tool_outcome — the outcome already encodes success/failure
                let is_error = match &tool_outcome {
                    Some(ToolOutcome::FileWriteFailed { .. }) => true,
                    Some(ToolOutcome::FileReadFailed { .. }) => true,
                    Some(ToolOutcome::CommandExecuted { succeeded, .. }) => !succeeded,
                    _ => false,
                };
                acc.completed_applies.push(ApplyRecord {
                    tool_name: pending.tool_name,
                    input_summary: pending.input_summary,
                    output_summary,
                    is_error,
                    is_agent: pending.is_agent,
                    tool_outcome,
                });
            }
            if acc.pending_applies.is_empty() {
                acc.phase = Phase::ResultsReady;
            }
        }

        // ── Assistant message: eval phase ──
        s if s.starts_with("message.assistant") => {
            acc.env_size += 1;
            patterns.push(make_pattern(
                "eval",
                &acc.session_id,
                ts,
                &[id],
                acc.turn_number,
                acc.scope_depth,
                acc.env_size,
            ));
            acc.phase = Phase::SawEval;

            // Capture eval content
            let content = ap.and_then(|p| p.text()).unwrap_or("").to_string();
            let stop_reason = ap.and_then(|p| p.stop_reason_str()).map(|s| s.to_string());
            let decision = if subtype == "message.assistant.tool_use" {
                "tool_use".to_string()
            } else {
                "text_only".to_string()
            };
            acc.pending_eval = Some(EvalOutput {
                content,
                timestamp: ts.to_string(),
                stop_reason,
                decision,
            });

            // Track tool calls — push ALL tool_use blocks to pending_applies
            if subtype == "message.assistant.tool_use" {
                if let Some(tool_name) = ap.and_then(|p| p.tool()) {
                    let tool_input = ap
                        .and_then(|p| p.args())
                        .map(|v| summarize_tool_input(tool_name, v))
                        .unwrap_or_default();
                    let is_agent = tool_name == "Agent";
                    acc.pending_applies.push(PendingApply {
                        tool_name: tool_name.to_string(),
                        input_summary: tool_input,
                        is_agent,
                    });
                    patterns.push(make_pattern(
                        "apply",
                        &acc.session_id,
                        ts,
                        &[id],
                        acc.turn_number,
                        acc.scope_depth,
                        acc.env_size,
                    ));
                    acc.phase = Phase::SawApply;
                }
            }
        }

        // ── Turn complete: coalgebra step done ──
        "system.turn.complete" => {
            acc.turn_number += 1;
            let is_terminal = acc.phase != Phase::ResultsReady && acc.phase != Phase::SawApply;
            let stop_reason = if is_terminal { "end_turn" } else { "tool_use" };

            // Extract duration from payload
            let duration_ms = ap.and_then(|p| match p {
                open_story_core::event_data::AgentPayload::ClaudeCode(cc) => cc.duration_ms,
                _ => None,
            });

            let env_delta = acc.env_size.saturating_sub(acc.env_size_at_start);
            let event_ids = std::mem::take(&mut acc.event_ids);
            let start = acc.start_ts.take().unwrap_or_else(|| ts.to_string());

            // Emit turn_end pattern
            let stop_str = if is_terminal {
                "end_turn → TERMINATE"
            } else {
                "tool_use → CONTINUE"
            };
            patterns.push(PatternEvent {
                pattern_type: "eval_apply.turn_end".to_string(),
                session_id: acc.session_id.clone(),
                event_ids: event_ids.clone(),
                started_at: start.clone(),
                ended_at: ts.to_string(),
                summary: format!(
                    "Turn {} (depth {}): {} | env: {} messages",
                    acc.turn_number, acc.scope_depth, stop_str, acc.env_size,
                ),
                metadata: serde_json::json!({
                    "phase": "turn_end",
                    "turn": acc.turn_number,
                    "scope_depth": acc.scope_depth,
                    "env_size": acc.env_size,
                    "stop_reason": stop_str,
                }),
            });

            // Crystallize the turn
            let turn = StructuralTurn {
                session_id: acc.session_id.clone(),
                turn_number: acc.turn_number,
                scope_depth: acc.scope_depth,
                human: acc.pending_human.take(),
                thinking: acc.pending_thinking.take(),
                eval: acc.pending_eval.take(),
                applies: std::mem::take(&mut acc.completed_applies),
                env_size: acc.env_size,
                env_delta,
                stop_reason: stop_reason.to_string(),
                is_terminal,
                timestamp: start,
                duration_ms,
                event_ids,
            };

            // Reset for next turn
            acc.pending_applies.clear();
            acc.phase = Phase::Idle;

            return StepResult::TurnComplete {
                acc,
                turn,
                patterns,
            };
        }

        // ── Compaction: GC ──
        "system.compact" => {
            patterns.push(make_pattern(
                "compact",
                &acc.session_id,
                ts,
                &[id],
                acc.turn_number,
                acc.scope_depth,
                acc.env_size,
            ));
        }

        _ => {}
    }

    StepResult::Continue { acc, patterns }
}

/// Build a PatternEvent. Pure helper — no self.
fn make_pattern(
    phase: &str,
    session_id: &str,
    ts: &str,
    ids: &[&str],
    turn: u32,
    scope_depth: u32,
    env_size: u32,
) -> PatternEvent {
    let summary = match phase {
        "eval" => format!("Turn {turn}: eval (model examines environment, produces expression)"),
        "apply" => format!("Turn {turn}: apply (tool dispatch)"),
        "compact" => format!("GC: context compaction (env was {env_size} messages)"),
        "scope_open" => format!("Compound procedure: nested eval-apply at depth {scope_depth}"),
        "scope_close" => format!("Scope closed, returning to depth {scope_depth}"),
        _ => format!("eval_apply.{phase}"),
    };
    PatternEvent {
        pattern_type: format!("eval_apply.{phase}"),
        session_id: session_id.to_string(),
        event_ids: ids.iter().map(|s| s.to_string()).collect(),
        started_at: ts.to_string(),
        ended_at: ts.to_string(),
        summary,
        metadata: serde_json::json!({
            "phase": phase,
            "turn": turn,
            "scope_depth": scope_depth,
            "env_size": env_size,
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════
// EvalApplyDetector — the actor that drives the fold
// ═══════════════════════════════════════════════════════════════════
//
// This is infrastructure, not logic. It holds the accumulator,
// calls step(), and manages the output buffers. The actual
// eval-apply logic is in step() above.

/// Eval-apply structural detector.
/// The actor that drives the `step()` fold over CloudEvents.
pub struct EvalApplyDetector {
    acc: Accumulator,
    /// Completed turns ready for downstream consumers.
    completed_turns: Vec<StructuralTurn>,
    // Legacy ViewRecord support fields
    legacy_turn_ids: Vec<String>,
    legacy_turn_start: Option<String>,
    legacy_scope_depth: u32,
}

impl EvalApplyDetector {
    pub fn new() -> Self {
        Self {
            acc: Accumulator::default(),
            completed_turns: Vec::new(),
            legacy_turn_ids: Vec::new(),
            legacy_turn_start: None,
            legacy_scope_depth: 0,
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // CloudEvent path — delegates to the pure step() function
    // ═══════════════════════════════════════════════════════════════

    /// Feed a CloudEvent. The actor calls step(), handles the result.
    pub fn feed_cloud_event(&mut self, event: &CloudEvent) -> Vec<PatternEvent> {
        let result = step(self.acc.clone(), event);
        match result {
            StepResult::Continue { acc, patterns } => {
                self.acc = acc;
                patterns
            }
            StepResult::TurnComplete { acc, turn, patterns } => {
                self.acc = acc;
                self.completed_turns.push(turn);
                patterns
            }
        }
    }

    /// Take completed StructuralTurns.
    pub fn take_completed_turns(&mut self) -> Vec<StructuralTurn> {
        std::mem::take(&mut self.completed_turns)
    }

    /// Flush: yield any in-progress turn as incomplete.
    pub fn flush_turns(&mut self) -> Vec<StructuralTurn> {
        if self.acc.pending_eval.is_some() || self.acc.pending_human.is_some() {
            let env_delta = self.acc.env_size.saturating_sub(self.acc.env_size_at_start);
            let turn = StructuralTurn {
                session_id: self.acc.session_id.clone(),
                turn_number: self.acc.turn_number,
                scope_depth: self.acc.scope_depth,
                human: self.acc.pending_human.take(),
                thinking: self.acc.pending_thinking.take(),
                eval: self.acc.pending_eval.take(),
                applies: std::mem::take(&mut self.acc.completed_applies),
                env_size: self.acc.env_size,
                env_delta,
                stop_reason: "end_turn".to_string(),
                is_terminal: true,
                timestamp: self.acc.start_ts.take().unwrap_or_default(),
                duration_ms: None,
                event_ids: std::mem::take(&mut self.acc.event_ids),
            };
            vec![turn]
        } else {
            vec![]
        }
    }
}

/// Extract a short summary from tool input JSON for display.
fn summarize_tool_input(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "Read" | "Write" | "Edit" => {
            args.get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Bash" => {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.len() > 80 { format!("{}...", &cmd[..77]) } else { cmd.to_string() }
        }
        "Grep" | "Glob" => {
            args.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "Agent" => {
            args.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "WebSearch" => {
            args.get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        "WebFetch" => {
            args.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
        _ => {
            let s = args.to_string();
            if s.len() > 80 { format!("{}...", &s[..77]) } else { s }
        }
    }
}

impl Detector for EvalApplyDetector {
    fn name(&self) -> &str {
        "eval_apply"
    }

    /// Legacy ViewRecord path. Uses acc fields directly for backward compat.
    fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
        let record = ctx.record;
        let id = record.id.as_str();
        let ts = record.timestamp.as_str();
        let mut events = Vec::new();

        if self.acc.session_id.is_empty() {
            self.acc.session_id = record.session_id.clone();
        }

        // Track scope changes via depth — use local var, don't mutate shared acc
        // (ctx.depth is ViewRecord tree depth, not Agent tool scope depth)
        let depth = ctx.depth as u32;
        let prev_depth = self.legacy_scope_depth;
        if depth > prev_depth {
            self.legacy_scope_depth = depth;
            events.push(make_pattern("scope_open", &self.acc.session_id, ts, &[id],
                self.acc.turn_number, depth, self.acc.env_size));
        } else if depth < prev_depth {
            self.legacy_scope_depth = depth;
            events.push(make_pattern("scope_close", &self.acc.session_id, ts, &[id],
                self.acc.turn_number, depth, self.acc.env_size));
        }

        self.legacy_turn_ids.push(id.to_string());
        if self.legacy_turn_start.is_none() {
            self.legacy_turn_start = Some(ts.to_string());
        }

        match &record.body {
            RecordBody::AssistantMessage(_) => {
                self.acc.turn_number += 1;
                self.acc.env_size += 1;
                events.push(make_pattern("eval", &self.acc.session_id, ts, &[id],
                    self.acc.turn_number, self.acc.scope_depth, self.acc.env_size));
                self.acc.phase = Phase::SawEval;
            }

            RecordBody::ToolCall(_) => {
                events.push(make_pattern("apply", &self.acc.session_id, ts, &[id],
                    self.acc.turn_number, self.acc.scope_depth, self.acc.env_size));
                self.acc.phase = Phase::SawApply;
            }

            RecordBody::ToolResult(_) => {
                self.acc.env_size += 1;
                self.acc.phase = Phase::ResultsReady;
            }

            RecordBody::UserMessage(_) => {
                self.acc.env_size += 1;
            }

            RecordBody::TurnEnd(_) => {
                let turn_ids = std::mem::take(&mut self.legacy_turn_ids);
                let start = self.legacy_turn_start
                    .take()
                    .unwrap_or_else(|| ts.to_string());
                let stop_str = if self.acc.phase == Phase::ResultsReady || self.acc.phase == Phase::SawApply {
                    "tool_use → CONTINUE"
                } else {
                    "end_turn → TERMINATE"
                };
                events.push(PatternEvent {
                    pattern_type: "eval_apply.turn_end".to_string(),
                    session_id: self.acc.session_id.clone(),
                    event_ids: turn_ids,
                    started_at: start,
                    ended_at: ts.to_string(),
                    summary: format!(
                        "Turn {} (depth {}): {} | env: {} messages",
                        self.acc.turn_number, self.acc.scope_depth, stop_str, self.acc.env_size,
                    ),
                    metadata: serde_json::json!({
                        "phase": "turn_end",
                        "turn": self.acc.turn_number,
                        "scope_depth": self.acc.scope_depth,
                        "env_size": self.acc.env_size,
                        "stop_reason": stop_str,
                    }),
                });
                self.acc.phase = Phase::Idle;
            }

            RecordBody::ContextCompaction(_) => {
                events.push(make_pattern("compact", &self.acc.session_id, ts, &[id],
                    self.acc.turn_number, self.acc.scope_depth, self.acc.env_size));
            }

            _ => {}
        }

        events
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        if !self.legacy_turn_ids.is_empty() {
            let ids = std::mem::take(&mut self.legacy_turn_ids);
            let start = self.legacy_turn_start.take().unwrap_or_default();
            let stop_str = "end_turn → TERMINATE";
            vec![PatternEvent {
                pattern_type: "eval_apply.turn_end".to_string(),
                session_id: self.acc.session_id.clone(),
                event_ids: ids,
                started_at: start.clone(),
                ended_at: start,
                summary: format!(
                    "Turn {} (depth {}): {} | env: {} messages",
                    self.acc.turn_number, self.acc.scope_depth, stop_str, self.acc.env_size,
                ),
                metadata: serde_json::json!({
                    "phase": "turn_end",
                    "turn": self.acc.turn_number,
                    "scope_depth": self.acc.scope_depth,
                    "env_size": self.acc.env_size,
                    "stop_reason": stop_str,
                }),
            }]
        } else {
            vec![]
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tests — driven by the Scheme prototype simulations
// ═══════════════════════════════════════════════════════════════════
//
// Each test maps to a scenario from 07-simulation.scm:
//   Simulation 1: Simple tool-using conversation
//   Simulation 2: Multi-step tool chain
//   Simulation 3: Max-turns guard (not applicable — detector doesn't enforce)
//   Simulation 4: Sub-agent delegation
//   Simulation 5: Context compaction

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_views::unified::*;
    use open_story_views::view_record::ViewRecord;

    fn make_ctx<'a>(
        record: &'a ViewRecord,
        depth: u16,
        parent_uuid: Option<&'a str>,
    ) -> FeedContext<'a> {
        FeedContext {
            record,
            depth,
            parent_uuid,
        }
    }

    fn user_msg(id: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq: 1,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::UserMessage(UserMessage {
                content: MessageContent::Text("test input".into()),
                images: vec![],
            }),
        }
    }

    fn assistant_msg(id: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq: 2,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:01Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::AssistantMessage(Box::new(AssistantMessage {
                model: "claude-4".into(),
                content: vec![ContentBlock::Text {
                    text: "response".into(),
                }],
                stop_reason: Some("end_turn".into()),
                end_turn: Some(true),
                phase: None,
            })),
        }
    }

    fn tool_call_vr(id: &str, name: &str) -> ViewRecord {
        let input = serde_json::json!({"command": "ls"});
        ViewRecord {
            id: id.into(),
            seq: 3,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:02Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolCall(Box::new(ToolCall {
                call_id: format!("toolu_{id}"),
                name: name.into(),
                input: input.clone(),
                raw_input: input,
                typed_input: None,
                status: None,
            })),
        }
    }

    fn tool_result_vr(id: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq: 4,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:03Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ToolResult(ToolResult {
                call_id: format!("toolu_{id}"),
                output: Some("result output".into()),
                is_error: false,
                tool_outcome: None,
            }),
        }
    }

    fn turn_end(id: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq: 5,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:04Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::TurnEnd(TurnEnd {
                turn_id: None,
                reason: Some("end_turn".into()),
                duration_ms: Some(1000),
            }),
        }
    }

    fn compaction(id: &str) -> ViewRecord {
        ViewRecord {
            id: id.into(),
            seq: 6,
            session_id: "sess-1".into(),
            timestamp: "2026-01-01T00:00:05Z".into(),
            agent_id: None,
            is_sidechain: false,
            body: RecordBody::ContextCompaction(ContextCompaction {
                reason: Some("auto".into()),
                message: Some("Compacted 10 messages".into()),
            }),
        }
    }

    /// Helper: feed a sequence of records and collect all emitted patterns.
    fn feed_all(det: &mut EvalApplyDetector, records: &[ViewRecord]) -> Vec<PatternEvent> {
        let mut all = Vec::new();
        for r in records {
            all.extend(det.feed(&make_ctx(r, 0, None)));
        }
        all
    }

    /// Helper: feed records with explicit depths.
    fn feed_with_depths(
        det: &mut EvalApplyDetector,
        records: &[(ViewRecord, u16)],
    ) -> Vec<PatternEvent> {
        let mut all = Vec::new();
        for (r, depth) in records {
            all.extend(det.feed(&make_ctx(r, *depth, None)));
        }
        all
    }

    fn pattern_types(events: &[PatternEvent]) -> Vec<&str> {
        events.iter().map(|e| e.pattern_type.as_str()).collect()
    }

    // ── Simulation 1: Simple text response ──
    // Scheme: user → model text → end_turn
    // Expected: eval + turn_end

    #[test]
    fn simple_text_response_produces_eval_and_turn_end() {
        let mut det = EvalApplyDetector::new();
        let events = feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),
            turn_end("te1"),
        ]);
        let types = pattern_types(&events);
        assert!(types.contains(&"eval_apply.eval"), "should emit eval");
        assert!(types.contains(&"eval_apply.turn_end"), "should emit turn_end");
        assert!(
            !types.contains(&"eval_apply.apply"),
            "no apply for text-only response"
        );
    }

    // ── Simulation 1: Tool-using conversation ──
    // Scheme: user → model tool_use → Bash → result → model text → end_turn
    // Expected: eval + apply + eval + turn_end

    #[test]
    fn tool_use_produces_eval_then_apply() {
        let mut det = EvalApplyDetector::new();
        let events = feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),      // eval: model requests tool
            tool_call_vr("tc1", "Bash"),  // apply: Bash dispatched
            tool_result_vr("tr1"),
            assistant_msg("a2"),      // eval: model summarizes
            turn_end("te1"),
        ]);
        let types = pattern_types(&events);
        assert_eq!(
            types.iter().filter(|t| **t == "eval_apply.eval").count(),
            2,
            "two eval phases"
        );
        assert_eq!(
            types.iter().filter(|t| **t == "eval_apply.apply").count(),
            1,
            "one apply phase"
        );
        assert!(types.contains(&"eval_apply.turn_end"));
    }

    // ── Simulation 2: Multi-step tool chain ──
    // Scheme: user → Grep → result → Read → result → summarize → end
    // Expected: eval, apply, eval, apply, eval, turn_end

    #[test]
    fn multi_step_tool_chain_produces_multiple_eval_apply_cycles() {
        let mut det = EvalApplyDetector::new();
        let events = feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),         // eval 1: requests Grep
            tool_call_vr("tc1", "Grep"),
            tool_result_vr("tr1"),
            assistant_msg("a2"),         // eval 2: requests Read
            tool_call_vr("tc2", "Read"),
            tool_result_vr("tr2"),
            assistant_msg("a3"),         // eval 3: summarizes
            turn_end("te1"),
        ]);
        let types = pattern_types(&events);
        assert_eq!(
            types.iter().filter(|t| **t == "eval_apply.eval").count(),
            3,
            "three eval phases: Grep request, Read request, summary"
        );
        assert_eq!(
            types.iter().filter(|t| **t == "eval_apply.apply").count(),
            2,
            "two apply phases: Grep and Read"
        );
    }

    // ── Simulation 4: Sub-agent delegation ──
    // Scheme: model → Agent tool → nested eval-apply → result
    // Expected: scope_open when depth increases, scope_close when it decreases

    #[test]
    fn agent_tool_opens_and_closes_scope() {
        let mut det = EvalApplyDetector::new();
        let events = feed_with_depths(&mut det, &[
            (user_msg("u1"), 0),
            (assistant_msg("a1"), 0),               // eval: requests Agent
            (tool_call_vr("tc1", "Agent"), 0),      // apply: Agent dispatched
            (user_msg("sub-u1"), 1),                 // nested scope begins
            (assistant_msg("sub-a1"), 1),             // nested eval
            (tool_call_vr("sub-tc1", "Bash"), 1),   // nested apply
            (tool_result_vr("sub-tr1"), 1),
            (turn_end("sub-te1"), 1),
            (tool_result_vr("tr1"), 0),              // back to root
            (turn_end("te1"), 0),
        ]);
        let types = pattern_types(&events);
        assert!(types.contains(&"eval_apply.scope_open"), "should emit scope_open");
        assert!(types.contains(&"eval_apply.scope_close"), "should emit scope_close");
    }

    // ── Environment size tracking ──
    // Scheme: the environment is a list of messages that grows with each turn.

    #[test]
    fn env_size_tracks_user_assistant_and_result_messages() {
        let mut det = EvalApplyDetector::new();
        feed_all(&mut det, &[
            user_msg("u1"),          // env: 1
            assistant_msg("a1"),     // env: 2
            tool_call_vr("tc1", "Bash"),
            tool_result_vr("tr1"),   // env: 3
            assistant_msg("a2"),     // env: 4
        ]);
        let events = det.flush();
        // The flush turn_end should report env_size = 4
        let turn_end_event = events.iter().find(|e| e.pattern_type == "eval_apply.turn_end");
        assert!(turn_end_event.is_some());
        assert_eq!(turn_end_event.unwrap().metadata["env_size"], 4);
    }

    // ── Simulation 5: Context compaction ──
    // Scheme: old messages summarized, environment shrinks.

    #[test]
    fn compaction_emits_compact_phase() {
        let mut det = EvalApplyDetector::new();
        let events = feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),
            compaction("gc1"),
        ]);
        let types = pattern_types(&events);
        assert!(
            types.contains(&"eval_apply.compact"),
            "should emit compact for ContextCompaction"
        );
    }

    // ── Turn numbers increment ──

    #[test]
    fn turn_numbers_increment_per_eval() {
        let mut det = EvalApplyDetector::new();
        let events = feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),  // turn 1
            tool_call_vr("tc1", "Bash"),
            tool_result_vr("tr1"),
            assistant_msg("a2"),  // turn 2
            tool_call_vr("tc2", "Read"),
            tool_result_vr("tr2"),
            assistant_msg("a3"),  // turn 3
            turn_end("te1"),
        ]);
        let evals: Vec<&PatternEvent> = events
            .iter()
            .filter(|e| e.pattern_type == "eval_apply.eval")
            .collect();
        assert_eq!(evals.len(), 3);
        assert_eq!(evals[0].metadata["turn"], 1);
        assert_eq!(evals[1].metadata["turn"], 2);
        assert_eq!(evals[2].metadata["turn"], 3);
    }

    // ── Flush emits incomplete turn ──

    #[test]
    fn flush_emits_incomplete_turn() {
        let mut det = EvalApplyDetector::new();
        feed_all(&mut det, &[
            user_msg("u1"),
            assistant_msg("a1"),
        ]);
        // No turn_end — flush should emit one
        let flushed = det.flush();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].pattern_type, "eval_apply.turn_end");
    }

    // ── No events when flush called on clean state ──

    #[test]
    fn flush_on_empty_state_emits_nothing() {
        let mut det = EvalApplyDetector::new();
        let flushed = det.flush();
        assert!(flushed.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════
    // CloudEvent path tests — StructuralTurn production
    // ═══════════════════════════════════════════════════════════════

    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{
        AgentPayload, ClaudeCodePayload, EventData,
    };

    fn make_cloud_event(subtype: &str, overrides: impl FnOnce(&mut ClaudeCodePayload)) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        overrides(&mut payload);
        let data = EventData::with_payload(
            serde_json::json!({}),
            1,
            "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "test".to_string(),
            "io.arc.event".to_string(),
            data,
            Some(subtype.to_string()),
            None,
            Some("2026-01-01T00:00:00Z".to_string()),
            None,
            None,
            Some("claude-code".to_string()),
        )
    }

    fn user_prompt_ce(text: &str) -> CloudEvent {
        make_cloud_event("message.user.prompt", |p| {
            p.text = Some(text.to_string());
        })
    }

    fn assistant_text_ce(text: &str) -> CloudEvent {
        make_cloud_event("message.assistant.text", |p| {
            p.text = Some(text.to_string());
            p.stop_reason = Some(serde_json::json!("end_turn"));
        })
    }

    fn assistant_tool_use_ce(text: &str, tool: &str, args: serde_json::Value) -> CloudEvent {
        make_cloud_event("message.assistant.tool_use", |p| {
            p.text = Some(text.to_string());
            p.tool = Some(tool.to_string());
            p.args = Some(args);
            p.stop_reason = Some(serde_json::json!("tool_use"));
        })
    }

    fn tool_result_ce(text: &str, outcome: Option<open_story_core::event_data::ToolOutcome>) -> CloudEvent {
        make_cloud_event("message.user.tool_result", |p| {
            p.text = Some(text.to_string());
            p.tool_outcome = outcome;
        })
    }

    fn turn_complete_ce() -> CloudEvent {
        make_cloud_event("system.turn.complete", |p| {
            p.duration_ms = Some(1000.0);
        })
    }

    #[test]
    fn ce_simple_text_produces_terminal_turn() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("What is a coalgebra?"));
        det.feed_cloud_event(&assistant_text_ce("A coalgebra is the dual of an algebra."));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert!(t.is_terminal, "text-only response should be terminal");
        assert!(t.applies.is_empty(), "no applies for text-only");
        assert_eq!(t.human.as_ref().unwrap().content, "What is a coalgebra?");
        assert!(t.eval.as_ref().unwrap().content.contains("coalgebra"));
    }

    #[test]
    fn ce_tool_use_produces_turn_with_applies() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("List the files"));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "Let me check.",
            "Bash",
            serde_json::json!({"command": "ls -la"}),
        ));
        det.feed_cloud_event(&tool_result_ce(
            "file1.rs\nfile2.rs",
            Some(ToolOutcome::CommandExecuted {
                command: "ls -la".to_string(),
                succeeded: true,
            }),
        ));
        det.feed_cloud_event(&assistant_text_ce("Here are the files."));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert_eq!(t.applies.len(), 1);
        assert_eq!(t.applies[0].tool_name, "Bash");
        assert_eq!(t.applies[0].input_summary, "ls -la");
        assert!(t.applies[0].tool_outcome.is_some());
    }

    #[test]
    fn ce_turn_captures_human_content() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("Tell me about SICP"));
        det.feed_cloud_event(&assistant_text_ce("SICP is..."));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        assert_eq!(turns[0].human.as_ref().unwrap().content, "Tell me about SICP");
    }

    #[test]
    fn ce_turn_captures_eval_content() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("hello"));
        det.feed_cloud_event(&assistant_text_ce("Hello! How can I help?"));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        assert_eq!(turns[0].eval.as_ref().unwrap().content, "Hello! How can I help?");
    }

    #[test]
    fn ce_flush_yields_incomplete_turn() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("hello"));
        det.feed_cloud_event(&assistant_text_ce("hi"));
        // No turn_complete — flush should yield the incomplete turn
        let turns = det.flush_turns();
        assert_eq!(turns.len(), 1);
        assert!(turns[0].human.is_some());
        assert!(turns[0].eval.is_some());
    }
}
