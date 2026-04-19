//! Monadic event payload: Foundation + Option<AgentPayload>.
//!
//! Three layers (SICP 2.4 — dispatch on type with tagged data):
//!
//!   Foundation:  raw, seq, session_id — always present, never mutated.
//!   The lift:    agent_payload — None if unknown agent, Some if recognized.
//!   The tag:     meta.agent inside the payload — self-describing dispatch.
//!
//! The translator reads the raw transcript, determines the agent (auto-detect),
//! extracts typed fields into the payload, and wraps it in Some. If extraction
//! fails or the agent is unknown, agent_payload is None — you still have raw.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Domain Events ─────────────────────────────────────────────────
//
// What changed in the world. Derived deterministically from tool_call
// + tool_result pairs. Same input → same output. No heuristics.
//
// Maps to: 03-tools.scm (tool dispatch returns typed results)
// Prototype: docs/research/eval-apply-prototype/domain.ts

/// What a tool call changed in the world.
/// Deterministic projection: tool name + input + result → ToolOutcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ToolOutcome {
    /// Write tool + "created successfully" in result
    FileCreated { path: String },
    /// Write tool + "updated successfully", or any Edit tool
    FileModified { path: String },
    /// Read tool (successful)
    FileRead { path: String },
    /// Write/Edit tool with is_error
    FileWriteFailed { path: String, reason: String },
    /// Read tool with is_error
    FileReadFailed { path: String, reason: String },
    /// Grep, Glob, WebSearch, WebFetch
    SearchPerformed { pattern: String, source: String },
    /// Bash tool
    CommandExecuted { command: String, succeeded: bool },
    /// Agent tool — agent_id links to the subagent session ("agent-{agent_id}")
    SubAgentSpawned { description: String, #[serde(default)] agent_id: String },
}

/// Derive the domain event from a tool call + result pair.
/// Pure function: same input → same output. No heuristics.
///
/// Maps to: prototype `toDomainEvent()` in `domain.ts`
/// Scheme parallel: 03-tools.scm — tool dispatch returns typed results
pub fn derive_tool_outcome(
    tool_name: &str,
    tool_input: &Value,
    result_output: &str,
    is_error: bool,
) -> Option<ToolOutcome> {
    match tool_name {
        "Write" => {
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed {
                    path,
                    reason: result_output.to_string(),
                })
            } else if result_output.contains("created successfully") {
                Some(ToolOutcome::FileCreated { path })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Edit" => {
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed {
                    path,
                    reason: result_output.to_string(),
                })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Read" => {
            let path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileReadFailed {
                    path,
                    reason: result_output.to_string(),
                })
            } else {
                Some(ToolOutcome::FileRead { path })
            }
        }
        "Grep" | "Glob" => {
            let pattern = tool_input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SearchPerformed {
                pattern,
                source: "filesystem".to_string(),
            })
        }
        "WebSearch" | "WebFetch" => {
            let query = tool_input
                .get("query")
                .or_else(|| tool_input.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SearchPerformed {
                pattern: query,
                source: "web".to_string(),
            })
        }
        "Bash" => {
            let command = tool_input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::CommandExecuted {
                command,
                succeeded: !is_error,
            })
        }
        "Agent" => {
            let description = tool_input
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SubAgentSpawned { description, agent_id: String::new() })
        }
        _ => None, // Unknown tool — no domain event
    }
}

// ── Foundation ─────────────────────────────────────────────────────

/// The event data envelope. Foundation is always present.
/// The agent_payload is the monadic lift — absent means "couldn't type it."
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EventData {
    /// Original transcript line, never mutated. The foundation.
    pub raw: Value,
    /// Sequence number within the translation session.
    pub seq: u64,
    /// Session identifier.
    pub session_id: String,
    /// The lift: typed agent-specific payload. None = unknown agent or parse failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_payload: Option<AgentPayload>,
}

impl EventData {
    /// Create foundation-only EventData (no payload yet).
    pub fn new(raw: Value, seq: u64, session_id: String) -> Self {
        Self {
            raw,
            seq,
            session_id,
            agent_payload: None,
        }
    }

    /// Create EventData with a typed agent payload.
    pub fn with_payload(raw: Value, seq: u64, session_id: String, payload: AgentPayload) -> Self {
        Self {
            raw,
            seq,
            session_id,
            agent_payload: Some(payload),
        }
    }
}

// ── The Tag ────────────────────────────────────────────────────────

/// Payload metadata — just the tag. Determined by format auto-detection,
/// not from the transcript. Enough to dispatch, nothing more.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PayloadMeta {
    /// SICP type tag: "claude-code", "pi-mono", or "hermes".
    pub agent: String,
}

// ── The Lift: Agent Payload ────────────────────────────────────────

/// Tagged union of agent-specific payloads.
/// Dispatch on `meta.agent` to determine variant.
///
/// Serializes as: `{ "meta": { "agent": "claude-code" }, "text": "...", ... }`
/// The tag is inside the payload, making it self-describing.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "_variant")]
pub enum AgentPayload {
    #[serde(rename = "claude-code")]
    ClaudeCode(ClaudeCodePayload),
    #[serde(rename = "pi-mono")]
    PiMono(PiMonoPayload),
    #[serde(rename = "hermes")]
    Hermes(HermesPayload),
}

// ── Claude Code Payload ────────────────────────────────────────────

/// Typed extraction for Claude Code events.
/// All fields translated from the agent transcript — no interpretation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClaudeCodePayload {
    /// The tag.
    pub meta: PayloadMeta,

    // ── Identity & tree ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,

    // ── Context ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    // ── Message content ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_types: Option<Vec<String>>,

    // ── Tool use ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,

    // ── Domain event: what this tool call changed in the world ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcome: Option<ToolOutcome>,

    // ── Token usage (Claude Code shape) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<Value>,

    // ── Session & agent identity ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_sidechain: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_type: Option<String>,

    // ── Progress & hooks ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prevented_continuation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,

    // ── Open schema catch-all ──
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for ClaudeCodePayload {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodePayload {
    pub fn new() -> Self {
        Self {
            meta: PayloadMeta {
                agent: "claude-code".to_string(),
            },
            uuid: None,
            parent_uuid: None,
            cwd: None,
            timestamp: None,
            version: None,
            text: None,
            model: None,
            stop_reason: None,
            content_types: None,
            tool: None,
            args: None,
            tool_outcome: None,
            token_usage: None,
            slug: None,
            message_id: None,
            git_branch: None,
            is_sidechain: None,
            agent_id: None,
            user_type: None,
            progress_type: None,
            parent_tool_use_id: None,
            operation: None,
            hook_count: None,
            prevented_continuation: None,
            duration_ms: None,
            extra: serde_json::Map::new(),
        }
    }
}

// ── Pi-Mono Payload ────────────────────────────────────────────────

/// Typed extraction for pi-mono (OpenClaw) events.
/// All fields translated from the agent transcript — no interpretation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PiMonoPayload {
    /// The tag.
    pub meta: PayloadMeta,

    // ── Identity & tree ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,

    // ── Context ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<Value>,

    // ── Message content ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_types: Option<Vec<String>>,

    // ── Tool use ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,

    // ── Domain event: what this tool call changed in the world ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcome: Option<ToolOutcome>,

    // ── Token usage (pi-mono shape) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<Value>,

    // ── Pi-mono specific ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    // ── Tool result fields ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,

    // ── Bash execution ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,

    // ── Compaction ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_entry_id: Option<String>,

    // ── Open schema catch-all ──
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for PiMonoPayload {
    fn default() -> Self {
        Self::new()
    }
}

impl PiMonoPayload {
    pub fn new() -> Self {
        Self {
            meta: PayloadMeta {
                agent: "pi-mono".to_string(),
            },
            uuid: None,
            parent_uuid: None,
            cwd: None,
            timestamp: None,
            version: None,
            text: None,
            model: None,
            stop_reason: None,
            content_types: None,
            tool: None,
            args: None,
            tool_outcome: None,
            token_usage: None,
            provider: None,
            thinking_level: None,
            model_id: None,
            tool_call_id: None,
            tool_name: None,
            is_error: None,
            command: None,
            exit_code: None,
            output: None,
            summary: None,
            tokens_before: None,
            first_kept_entry_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

// ── Hermes Payload ─────────────────────────────────────────────────

/// Typed extraction for Hermes Agent (NousResearch/hermes-agent) events.
///
/// Hermes is a self-improving agent that stores conversations in OpenAI shape
/// regardless of which provider produced the response. The Anthropic adapter
/// at `tests/agent/test_anthropic_adapter.py:575` is a one-way translator at
/// the API boundary; persisted state is always OpenAI shape (assistant
/// `tool_calls: [{id, function: {name, arguments}}]`, tool messages with
/// `tool_call_id` and optional `tool_name`, reasoning as a top-level string
/// field on the assistant message).
///
/// Verified against hermes-agent commit 6e3f7f36 on 2026-04-08.
/// See `docs/research/hermes-integration/SOURCE_VERIFICATION.md`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HermesPayload {
    /// The tag.
    pub meta: PayloadMeta,

    // ── Identity & sequencing ──
    /// Plugin-side monotonic per-session sequence number from the input envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    // ── Session metadata (system.session.start only) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hermes_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,

    // ── Message content ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Stop reason (`finish_reason`) — usually `"stop"` or `"tool_calls"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    // ── Tool use (assistant) ──
    /// Canonical OpenAI shape: tool name from `tool_calls[i].function.name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Parsed JSON args from `tool_calls[i].function.arguments` (which is a JSON string in the wire format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    /// `tool_calls[i].id` — used to link the tool_use to its later tool_result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Text content the assistant emitted alongside the tool call (eval phase preceding apply).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_text: Option<String>,

    // ── Tool result (role: tool) ──
    /// `tool_call_id` from the tool message — links back to the originating tool_use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// `tool_name` is OPTIONAL in Hermes — present in some code paths, absent in others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    // ── Thinking ──
    /// Reasoning content. Hermes stores this as a top-level string field on
    /// the assistant message (verified). The Anthropic SDK adapter converts
    /// `content_block_delta` thinking blocks to a flat string before storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    // ── Turn complete (system.turn.complete) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_count: Option<u64>,

    // ── Open schema catch-all ──
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for HermesPayload {
    fn default() -> Self {
        Self::new()
    }
}

impl HermesPayload {
    pub fn new() -> Self {
        Self {
            meta: PayloadMeta {
                agent: "hermes".to_string(),
            },
            seq: None,
            timestamp: None,
            model: None,
            platform: None,
            hermes_version: None,
            system_prompt_preview: None,
            tools: None,
            text: None,
            stop_reason: None,
            tool: None,
            args: None,
            tool_use_id: None,
            preceding_text: None,
            tool_call_id: None,
            tool_name: None,
            reasoning: None,
            reason: None,
            completed: None,
            interrupted: None,
            message_count: None,
            extra: serde_json::Map::new(),
        }
    }
}

// ── Convenience accessors ──────────────────────────────────────────

impl AgentPayload {
    /// Get the agent tag string.
    pub fn agent(&self) -> &str {
        match self {
            AgentPayload::ClaudeCode(p) => &p.meta.agent,
            AgentPayload::PiMono(p) => &p.meta.agent,
            AgentPayload::Hermes(p) => &p.meta.agent,
        }
    }

    /// Get text from either payload variant.
    pub fn text(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.text.as_deref(),
            AgentPayload::PiMono(p) => p.text.as_deref(),
            AgentPayload::Hermes(p) => p.text.as_deref(),
        }
    }

    /// Get model from either payload variant.
    pub fn model(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.model.as_deref(),
            AgentPayload::PiMono(p) => p.model.as_deref(),
            AgentPayload::Hermes(p) => p.model.as_deref(),
        }
    }

    /// Get tool name from either payload variant.
    pub fn tool(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.tool.as_deref(),
            AgentPayload::PiMono(p) => p.tool.as_deref(),
            AgentPayload::Hermes(p) => p.tool.as_deref(),
        }
    }

    /// Get tool args from either payload variant.
    pub fn args(&self) -> Option<&Value> {
        match self {
            AgentPayload::ClaudeCode(p) => p.args.as_ref(),
            AgentPayload::PiMono(p) => p.args.as_ref(),
            AgentPayload::Hermes(p) => p.args.as_ref(),
        }
    }

    /// Get token usage from either payload variant.
    ///
    /// Hermes does not track token usage at the per-message level in the
    /// way Claude Code and pi-mono do — it lives in the LLM provider's
    /// raw response, not the persisted message dict. Returns None for
    /// the Hermes variant.
    pub fn token_usage(&self) -> Option<&Value> {
        match self {
            AgentPayload::ClaudeCode(p) => p.token_usage.as_ref(),
            AgentPayload::PiMono(p) => p.token_usage.as_ref(),
            AgentPayload::Hermes(_) => None,
        }
    }

    /// Get uuid from either payload variant.
    ///
    /// Hermes uses synthetic event IDs derived from `(session_id, seq, subtype)`
    /// rather than UUIDs from the source data. Returns None for the Hermes
    /// variant — the deterministic ID lives on the CloudEvent envelope itself.
    pub fn uuid(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.uuid.as_deref(),
            AgentPayload::PiMono(p) => p.uuid.as_deref(),
            AgentPayload::Hermes(_) => None,
        }
    }

    /// Get parent_uuid from either payload variant.
    ///
    /// Hermes does not maintain a parent-child message tree the way Claude
    /// Code does (its message list is a flat sequence). Returns None.
    pub fn parent_uuid(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.parent_uuid.as_deref(),
            AgentPayload::PiMono(p) => p.parent_uuid.as_deref(),
            AgentPayload::Hermes(_) => None,
        }
    }

    /// Get cwd from either payload variant.
    ///
    /// Hermes does not record cwd in its message dicts (it's a process-level
    /// concept, not a message-level one). Returns None.
    pub fn cwd(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.cwd.as_deref(),
            AgentPayload::PiMono(p) => p.cwd.as_deref(),
            AgentPayload::Hermes(_) => None,
        }
    }

    /// Get stop_reason as a string from either variant.
    pub fn stop_reason_str(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.stop_reason.as_ref().and_then(|v| v.as_str()),
            AgentPayload::PiMono(p) => p.stop_reason.as_deref(),
            AgentPayload::Hermes(p) => p.stop_reason.as_deref(),
        }
    }

    /// Get content_types from either variant.
    ///
    /// Hermes uses flat OpenAI-shape messages without typed content blocks,
    /// so there's nothing to surface here. Returns None.
    pub fn content_types(&self) -> Option<&[String]> {
        match self {
            AgentPayload::ClaudeCode(p) => p.content_types.as_deref(),
            AgentPayload::PiMono(p) => p.content_types.as_deref(),
            AgentPayload::Hermes(_) => None,
        }
    }

    /// Get tool_outcome from either variant.
    ///
    /// Hermes does not currently produce typed `ToolOutcome` values — they're
    /// derived in the Claude Code translator from inspecting tool inputs.
    /// A future enhancement could derive them for Hermes too. For now: None.
    pub fn tool_outcome(&self) -> Option<&ToolOutcome> {
        match self {
            AgentPayload::ClaudeCode(p) => p.tool_outcome.as_ref(),
            AgentPayload::PiMono(p) => p.tool_outcome.as_ref(),
            AgentPayload::Hermes(_) => None,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_foundation_only_serialization() {
        let data = EventData::new(json!({"type": "user"}), 1, "sess-1".to_string());
        let json = serde_json::to_value(&data).unwrap();

        assert_eq!(json["raw"], json!({"type": "user"}));
        assert_eq!(json["seq"], 1);
        assert_eq!(json["session_id"], "sess-1");
        // No agent_payload key when None
        assert!(!json.as_object().unwrap().contains_key("agent_payload"));
    }

    #[test]
    fn test_claude_code_payload_serialization() {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("hello".to_string());
        payload.tool = Some("Bash".to_string());
        payload.is_sidechain = Some(false);
        payload.token_usage = Some(json!({"input_tokens": 100, "output_tokens": 50}));

        let data = EventData::with_payload(
            json!({"type": "assistant"}),
            1,
            "sess-1".to_string(),
            AgentPayload::ClaudeCode(payload),
        );

        let json = serde_json::to_value(&data).unwrap();
        let ap = &json["agent_payload"];

        // Meta tag is present
        assert_eq!(ap["meta"]["agent"], "claude-code");
        // Content fields
        assert_eq!(ap["text"], "hello");
        assert_eq!(ap["tool"], "Bash");
        assert_eq!(ap["is_sidechain"], false);
        assert_eq!(ap["token_usage"]["input_tokens"], 100);
        // Foundation stays at top level
        assert_eq!(json["seq"], 1);
        assert_eq!(json["session_id"], "sess-1");
    }

    #[test]
    fn test_pi_mono_payload_serialization() {
        let mut payload = PiMonoPayload::new();
        payload.text = Some("reading config".to_string());
        payload.tool = Some("read".to_string());
        payload.provider = Some("anthropic".to_string());
        payload.token_usage = Some(json!({"input": 150, "output": 75, "cacheRead": 0}));

        let data = EventData::with_payload(
            json!({"type": "message"}),
            2,
            "sess-2".to_string(),
            AgentPayload::PiMono(payload),
        );

        let json = serde_json::to_value(&data).unwrap();
        let ap = &json["agent_payload"];

        assert_eq!(ap["meta"]["agent"], "pi-mono");
        assert_eq!(ap["text"], "reading config");
        assert_eq!(ap["provider"], "anthropic");
        assert_eq!(ap["token_usage"]["cacheRead"], 0);
    }

    #[test]
    fn test_round_trip_claude_code() {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("test".to_string());
        payload.model = Some("claude-opus-4-6".to_string());
        payload.git_branch = Some("main".to_string());
        payload.extra.insert("custom_field".to_string(), json!(42));

        let data = EventData::with_payload(
            json!({}),
            0,
            "s".to_string(),
            AgentPayload::ClaudeCode(payload),
        );

        let serialized = serde_json::to_value(&data).unwrap();
        let deserialized: EventData = serde_json::from_value(serialized.clone()).unwrap();

        let reserialized = serde_json::to_value(&deserialized).unwrap();
        assert_eq!(serialized, reserialized, "Round-trip must preserve JSON shape");
    }

    #[test]
    fn test_round_trip_pi_mono() {
        let mut payload = PiMonoPayload::new();
        payload.text = Some("hi".to_string());
        payload.provider = Some("openai".to_string());
        payload.thinking_level = Some("high".to_string());

        let data = EventData::with_payload(
            json!({}),
            0,
            "s".to_string(),
            AgentPayload::PiMono(payload),
        );

        let serialized = serde_json::to_value(&data).unwrap();
        let deserialized: EventData = serde_json::from_value(serialized.clone()).unwrap();

        let reserialized = serde_json::to_value(&deserialized).unwrap();
        assert_eq!(serialized, reserialized, "Round-trip must preserve JSON shape");
    }

    #[test]
    fn test_convenience_accessors() {
        let mut cc = ClaudeCodePayload::new();
        cc.text = Some("hello".to_string());
        cc.tool = Some("Read".to_string());
        cc.model = Some("opus".to_string());

        let ap = AgentPayload::ClaudeCode(cc);
        assert_eq!(ap.agent(), "claude-code");
        assert_eq!(ap.text(), Some("hello"));
        assert_eq!(ap.tool(), Some("Read"));
        assert_eq!(ap.model(), Some("opus"));

        let mut pm = PiMonoPayload::new();
        pm.text = Some("hi".to_string());
        pm.provider = Some("anthropic".to_string());

        let ap2 = AgentPayload::PiMono(pm);
        assert_eq!(ap2.agent(), "pi-mono");
        assert_eq!(ap2.text(), Some("hi"));
        assert_eq!(ap2.tool(), None);
    }

    #[test]
    fn test_none_payload_fields_omitted() {
        let payload = ClaudeCodePayload::new();
        let data = EventData::with_payload(
            json!({}),
            0,
            "s".to_string(),
            AgentPayload::ClaudeCode(payload),
        );

        let json = serde_json::to_value(&data).unwrap();
        let ap = json["agent_payload"].as_object().unwrap();

        // Meta is always present
        assert!(ap.contains_key("meta"));
        // None fields are skipped
        assert!(!ap.contains_key("text"));
        assert!(!ap.contains_key("tool"));
        assert!(!ap.contains_key("model"));
        assert!(!ap.contains_key("is_sidechain"));
    }

    // ── derive_tool_outcome tests ──────────────────────────────────
    // Maps 1:1 to prototype domain-test.ts (20 tests).
    // Scheme parallel: 03-tools.scm — tool dispatch returns typed results.

    mod derive_tool_outcome_tests {
        use super::*;
        use crate::event_data::derive_tool_outcome;

        #[test]
        fn write_created_successfully_produces_file_created() {
            let outcome = derive_tool_outcome(
                "Write",
                &json!({"file_path": "/scheme/01-types.scm"}),
                "File created successfully at: /scheme/01-types.scm",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileCreated {
                    path: "/scheme/01-types.scm".to_string()
                })
            );
        }

        #[test]
        fn write_without_created_produces_file_modified() {
            let outcome = derive_tool_outcome(
                "Write",
                &json!({"file_path": "/README.md"}),
                "The file has been updated successfully.",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileModified {
                    path: "/README.md".to_string()
                })
            );
        }

        #[test]
        fn write_error_produces_file_write_failed() {
            let outcome = derive_tool_outcome(
                "Write",
                &json!({"file_path": "/readonly.txt"}),
                "Permission denied",
                true,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileWriteFailed {
                    path: "/readonly.txt".to_string(),
                    reason: "Permission denied".to_string(),
                })
            );
        }

        #[test]
        fn edit_produces_file_modified() {
            let outcome = derive_tool_outcome(
                "Edit",
                &json!({"file_path": "/src/main.rs", "old_string": "a", "new_string": "b"}),
                "The file has been updated successfully.",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileModified {
                    path: "/src/main.rs".to_string()
                })
            );
        }

        #[test]
        fn edit_error_produces_file_write_failed() {
            let outcome = derive_tool_outcome(
                "Edit",
                &json!({"file_path": "/src/main.rs"}),
                "old_string not found in file",
                true,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileWriteFailed {
                    path: "/src/main.rs".to_string(),
                    reason: "old_string not found in file".to_string(),
                })
            );
        }

        #[test]
        fn read_produces_file_read() {
            let outcome = derive_tool_outcome(
                "Read",
                &json!({"file_path": "/Cargo.toml"}),
                "[package]\nname = \"open-story\"",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileRead {
                    path: "/Cargo.toml".to_string()
                })
            );
        }

        #[test]
        fn read_error_produces_file_read_failed() {
            let outcome = derive_tool_outcome(
                "Read",
                &json!({"file_path": "/nonexistent.rs"}),
                "File not found: /nonexistent.rs",
                true,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::FileReadFailed {
                    path: "/nonexistent.rs".to_string(),
                    reason: "File not found: /nonexistent.rs".to_string(),
                })
            );
        }

        #[test]
        fn grep_produces_search_performed() {
            let outcome = derive_tool_outcome(
                "Grep",
                &json!({"pattern": "TODO"}),
                "src/main.rs:2: // TODO: fix this",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::SearchPerformed {
                    pattern: "TODO".to_string(),
                    source: "filesystem".to_string(),
                })
            );
        }

        #[test]
        fn glob_produces_search_performed() {
            let outcome = derive_tool_outcome(
                "Glob",
                &json!({"pattern": "**/*.rs"}),
                "src/main.rs\nsrc/lib.rs",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::SearchPerformed {
                    pattern: "**/*.rs".to_string(),
                    source: "filesystem".to_string(),
                })
            );
        }

        #[test]
        fn web_search_produces_search_performed() {
            let outcome = derive_tool_outcome(
                "WebSearch",
                &json!({"query": "rust async patterns"}),
                "Results...",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::SearchPerformed {
                    pattern: "rust async patterns".to_string(),
                    source: "web".to_string(),
                })
            );
        }

        #[test]
        fn web_fetch_uses_url_as_pattern() {
            let outcome = derive_tool_outcome(
                "WebFetch",
                &json!({"url": "https://example.com/docs"}),
                "<html>...</html>",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::SearchPerformed {
                    pattern: "https://example.com/docs".to_string(),
                    source: "web".to_string(),
                })
            );
        }

        #[test]
        fn bash_success_produces_command_executed() {
            let outcome = derive_tool_outcome(
                "Bash",
                &json!({"command": "cargo test"}),
                "test result: ok. 5 passed",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::CommandExecuted {
                    command: "cargo test".to_string(),
                    succeeded: true,
                })
            );
        }

        #[test]
        fn bash_error_produces_command_failed() {
            let outcome = derive_tool_outcome(
                "Bash",
                &json!({"command": "cargo test"}),
                "test result: FAILED",
                true,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::CommandExecuted {
                    command: "cargo test".to_string(),
                    succeeded: false,
                })
            );
        }

        #[test]
        fn agent_produces_sub_agent_spawned() {
            let outcome = derive_tool_outcome(
                "Agent",
                &json!({"description": "research task", "prompt": "Find all TODO items"}),
                "Found 3 TODOs...",
                false,
            );
            assert_eq!(
                outcome,
                Some(ToolOutcome::SubAgentSpawned {
                    description: "research task".to_string(),
                    agent_id: String::new(),
                })
            );
        }

        #[test]
        fn unknown_tool_produces_none() {
            let outcome = derive_tool_outcome("FutureTool", &json!({}), "ok", false);
            assert!(outcome.is_none());
        }

        #[test]
        fn tool_outcome_serializes_with_type_tag() {
            let outcome = ToolOutcome::FileCreated {
                path: "/test.rs".to_string(),
            };
            let json = serde_json::to_value(&outcome).unwrap();
            assert_eq!(json["type"], "FileCreated");
            assert_eq!(json["path"], "/test.rs");
        }

        #[test]
        fn tool_outcome_round_trips() {
            let outcomes = vec![
                ToolOutcome::FileCreated { path: "/a.rs".to_string() },
                ToolOutcome::FileModified { path: "/b.rs".to_string() },
                ToolOutcome::CommandExecuted { command: "ls".to_string(), succeeded: true },
                ToolOutcome::SearchPerformed { pattern: "TODO".to_string(), source: "filesystem".to_string() },
            ];
            for outcome in outcomes {
                let json = serde_json::to_value(&outcome).unwrap();
                let round_tripped: ToolOutcome = serde_json::from_value(json).unwrap();
                assert_eq!(outcome, round_tripped);
            }
        }
    }

    #[test]
    fn test_extra_fields_serialize_flat() {
        let mut payload = ClaudeCodePayload::new();
        payload.extra.insert("future_field".to_string(), json!("surprise"));
        payload.extra.insert("another".to_string(), json!(99));

        let data = EventData::with_payload(
            json!({}),
            0,
            "s".to_string(),
            AgentPayload::ClaudeCode(payload),
        );

        let json = serde_json::to_value(&data).unwrap();
        let ap = &json["agent_payload"];

        // Extra fields are flat in the payload, not nested under "extra"
        assert_eq!(ap["future_field"], "surprise");
        assert_eq!(ap["another"], 99);
    }
}
