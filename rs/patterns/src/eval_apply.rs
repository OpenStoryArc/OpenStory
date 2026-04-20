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

use crate::PatternEvent;

// ═══════════════════════════════════════════════════════════════════
// StructuralTurn — the intermediate representation
// ═══════════════════════════════════════════════════════════════════
//
// One eval-apply step, aggregated. The natural unit of agent behavior.
// Downstream detectors (SentenceDetector, etc.) consume these.
//
// Prototype source: types.ts:127-157

/// One step of the eval-apply coalgebra, fully resolved.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Agent platform that produced this turn (e.g., "claude-code", "hermes").
    /// Used by the sentence builder to generate agent-appropriate subjects.
    #[serde(default)]
    pub agent: Option<String>,
}

/// The human message that prompted this turn.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HumanInput {
    pub content: String,
    pub timestamp: String,
}

/// The model's reasoning (thinking phase).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThinkingRecord {
    pub summary: String,
}

/// The model's response (eval phase output).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EvalOutput {
    pub content: String,
    pub timestamp: String,
    pub stop_reason: Option<String>,
    pub decision: String,
}

/// One tool dispatch (apply phase).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Agent platform (e.g., "claude-code", "hermes"). Set from the first
    /// event's agent field and carried through to StructuralTurn.
    pub agent: Option<String>,
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
            agent: None,
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
#[allow(clippy::large_enum_variant)]
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

    // Track session + agent
    if acc.session_id.is_empty() {
        acc.session_id = event.data.session_id.clone();
    }
    if acc.agent.is_none() {
        acc.agent = event.agent.clone();
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
                // Derive is_error from tool_outcome — the outcome already encodes success/failure.
                // For Hermes (which has no typed ToolOutcome), fall back to checking the
                // tool result content JSON for an "error" key — Hermes encodes errors
                // inside the content string (e.g., {"error": "File not found: ..."}).
                let is_error = match &tool_outcome {
                    Some(ToolOutcome::FileWriteFailed { .. }) => true,
                    Some(ToolOutcome::FileReadFailed { .. }) => true,
                    Some(ToolOutcome::CommandExecuted { succeeded, .. }) => !succeeded,
                    None => {
                        // Hermes fallback: check if content JSON has a non-null "error" key
                        let text = ap.and_then(|p| p.text()).unwrap_or("");
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                            v.get("error")
                                .map(|e| !e.is_null() && e.as_str() != Some(""))
                                .unwrap_or(false)
                        } else {
                            false
                        }
                    }
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
                agent: acc.agent.clone(),
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
}

impl Default for EvalApplyDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalApplyDetector {
    pub fn new() -> Self {
        Self {
            acc: Accumulator::default(),
            completed_turns: Vec::new(),
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
                agent: self.acc.agent.clone(),
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
            if cmd.chars().count() > 80 {
                let truncated: String = cmd.chars().take(77).collect();
                format!("{truncated}...")
            } else {
                cmd.to_string()
            }
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
            if s.chars().count() > 80 {
                let truncated: String = s.chars().take(77).collect();
                format!("{truncated}...")
            } else {
                s
            }
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

    // ── Audit walk #7 (2026-04-15) — coverage for state-machine edges ─

    // F-1 (real bug): tool_result pairing is FIFO with no call_id check.
    // For pi-mono parallel toolCalls where results arrive in different
    // order than calls, the wrong outcome attaches to the wrong call.

    #[test]
    fn parallel_tool_results_in_call_order_pair_correctly() {
        // Baseline: when results arrive in the same order as calls,
        // FIFO works and is_error matches the right call. This test
        // passes today and should keep passing under any fix.
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("Read both files"));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "reading foo",
            "Read",
            serde_json::json!({"file_path": "/foo.rs"}),
        ));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "reading bar",
            "Read",
            serde_json::json!({"file_path": "/bar.rs"}),
        ));
        // Results in CALL order
        det.feed_cloud_event(&tool_result_ce(
            "foo content",
            Some(ToolOutcome::FileRead { path: "/foo.rs".to_string() }),
        ));
        det.feed_cloud_event(&tool_result_ce(
            "bar content",
            Some(ToolOutcome::FileRead { path: "/bar.rs".to_string() }),
        ));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        let t = &turns[0];
        assert_eq!(t.applies.len(), 2);
        // Both should match: input_summary (call) ↔ tool_outcome.path (result)
        if let Some(ToolOutcome::FileRead { path }) = &t.applies[0].tool_outcome {
            assert_eq!(path, "/foo.rs", "first call's outcome should be foo");
            assert_eq!(t.applies[0].input_summary, "/foo.rs");
        } else {
            panic!("expected FileRead outcome");
        }
        if let Some(ToolOutcome::FileRead { path }) = &t.applies[1].tool_outcome {
            assert_eq!(path, "/bar.rs", "second call's outcome should be bar");
            assert_eq!(t.applies[1].input_summary, "/bar.rs");
        } else {
            panic!("expected FileRead outcome");
        }
    }

    #[test]
    fn parallel_tool_results_out_of_call_order_currently_misattribute() {
        // BUG CHARACTERIZATION (not aspiration): with results arriving
        // in REVERSE order vs calls, the FIFO matching at line 243
        // attaches the wrong outcome to each call. Documents today's
        // broken behavior so a fix that switches to call_id-based
        // matching flips this test red — and the developer can then
        // delete it.
        //
        // See docs/research/architecture-audit/EVAL_APPLY_WALK.md F-1.
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("Read both"));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "reading foo",
            "Read",
            serde_json::json!({"file_path": "/foo.rs"}),
        ));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "reading bar",
            "Read",
            serde_json::json!({"file_path": "/bar.rs"}),
        ));
        // Results in REVERSE order — bar finishes first
        det.feed_cloud_event(&tool_result_ce(
            "bar content",
            Some(ToolOutcome::FileRead { path: "/bar.rs".to_string() }),
        ));
        det.feed_cloud_event(&tool_result_ce(
            "foo content",
            Some(ToolOutcome::FileRead { path: "/foo.rs".to_string() }),
        ));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        let t = &turns[0];
        assert_eq!(t.applies.len(), 2);

        // CURRENT (broken) BEHAVIOR: foo's input is paired with bar's outcome
        // because pending_applies[0] = foo and the first arriving result
        // (which was bar's) gets popped off the front.
        assert_eq!(t.applies[0].input_summary, "/foo.rs", "call order preserved");
        if let Some(ToolOutcome::FileRead { path }) = &t.applies[0].tool_outcome {
            assert_eq!(
                path, "/bar.rs",
                "BUG: first call (foo) gets attributed bar's outcome — \
                 fix should switch to call_id-based matching"
            );
        }
    }

    // F-2 (medium): assistant text + tool_use in the same turn — only
    // the LAST one's content survives in pending_eval. For pi-mono
    // decomposed events where text + tool_use both arrive in one turn,
    // the text content is silently dropped.

    #[test]
    fn assistant_text_then_tool_use_overwrites_pending_eval_content() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("List the files please"));
        det.feed_cloud_event(&assistant_text_ce("I'll check the directory."));
        det.feed_cloud_event(&assistant_tool_use_ce(
            "", // empty text on the tool_use
            "Bash",
            serde_json::json!({"command": "ls"}),
        ));
        det.feed_cloud_event(&tool_result_ce(
            "file1.rs",
            Some(ToolOutcome::CommandExecuted {
                command: "ls".to_string(),
                succeeded: true,
            }),
        ));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        let eval = turns[0].eval.as_ref().expect("eval present");
        // CURRENT: tool_use's empty text overwrites the prior text content.
        // The "I'll check the directory." narrative is lost from eval.content.
        assert_eq!(
            eval.content, "",
            "BUG: tool_use overwrote text's content — eval narrative lost"
        );
    }

    // F-3 (info): system.compact emits a pattern but doesn't shrink env_size.
    // After compaction the accumulator's env_size is still the pre-compact
    // count. Worth a test so a future fix that decrements env_size has a
    // visible flip.

    #[test]
    fn system_compact_does_not_decrement_env_size() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&user_prompt_ce("p1"));
        det.feed_cloud_event(&assistant_text_ce("a1"));
        det.feed_cloud_event(&turn_complete_ce());
        det.feed_cloud_event(&user_prompt_ce("p2"));
        det.feed_cloud_event(&assistant_text_ce("a2"));
        det.feed_cloud_event(&turn_complete_ce());
        // Now env_size = 4. Compact would normally shrink that.
        det.feed_cloud_event(&make_cloud_event("system.compact", |_| {}));
        det.feed_cloud_event(&user_prompt_ce("p3"));
        det.feed_cloud_event(&assistant_text_ce("a3"));
        det.feed_cloud_event(&turn_complete_ce());

        let turns = det.take_completed_turns();
        // env_size on the third turn is 6 (4 + 2), not 2 (post-compact + 2)
        // because system.compact didn't reset env_size.
        assert_eq!(
            turns[2].env_size, 6,
            "compact does not shrink env_size today — accumulator carries \
             pre-compact count forward; documented gap"
        );
    }

    // F-4 (info): turn_complete with no events accumulated. What happens?

    #[test]
    fn turn_complete_with_no_prior_events_emits_empty_terminal_turn() {
        let mut det = EvalApplyDetector::new();
        det.feed_cloud_event(&turn_complete_ce());
        let turns = det.take_completed_turns();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert!(t.is_terminal);
        assert!(t.human.is_none());
        assert!(t.eval.is_none());
        assert!(t.applies.is_empty());
        // turn_number incremented to 1 from 0
        assert_eq!(t.turn_number, 1);
    }

    // F-5 (test gap): summarize_tool_input was 50 LOC of pure helper with
    // zero direct tests. Coverage was indirect via the turn assertions.

    #[test]
    fn summarize_tool_input_extracts_file_path_for_file_tools() {
        for tool in ["Read", "Write", "Edit"] {
            let s = summarize_tool_input(tool, &serde_json::json!({"file_path": "/x.rs"}));
            assert_eq!(s, "/x.rs", "{tool} should extract file_path");
        }
    }

    #[test]
    fn summarize_tool_input_truncates_long_bash_commands() {
        let long_cmd = "echo ".to_string() + &"x".repeat(200);
        let s = summarize_tool_input("Bash", &serde_json::json!({"command": long_cmd}));
        assert!(s.ends_with("..."), "long bash should end in ellipsis");
        assert_eq!(s.chars().count(), 80, "truncated to 80 chars including ellipsis");
    }

    #[test]
    fn summarize_tool_input_extracts_pattern_for_search_tools() {
        for tool in ["Grep", "Glob"] {
            let s = summarize_tool_input(tool, &serde_json::json!({"pattern": "fn main"}));
            assert_eq!(s, "fn main", "{tool} should extract pattern");
        }
    }

    #[test]
    fn summarize_tool_input_extracts_query_for_websearch_and_url_for_webfetch() {
        let s = summarize_tool_input("WebSearch", &serde_json::json!({"query": "rust async"}));
        assert_eq!(s, "rust async");
        let s = summarize_tool_input("WebFetch", &serde_json::json!({"url": "https://example.com"}));
        assert_eq!(s, "https://example.com");
    }

    #[test]
    fn summarize_tool_input_extracts_description_for_agent() {
        let s = summarize_tool_input("Agent", &serde_json::json!({"description": "do the thing"}));
        assert_eq!(s, "do the thing");
    }

    #[test]
    fn summarize_tool_input_falls_back_to_json_for_unknown_tools() {
        let s = summarize_tool_input("CustomTool", &serde_json::json!({"foo": "bar"}));
        assert_eq!(s, r#"{"foo":"bar"}"#);
    }

    #[test]
    fn summarize_tool_input_handles_missing_fields_gracefully() {
        // Read without file_path → empty string, not panic
        let s = summarize_tool_input("Read", &serde_json::json!({}));
        assert_eq!(s, "");
    }
}
