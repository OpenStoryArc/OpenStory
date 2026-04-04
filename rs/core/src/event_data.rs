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

// ── Foundation ─────────────────────────────────────────────────────

/// The event data envelope. Foundation is always present.
/// The agent_payload is the monadic lift — absent means "couldn't type it."
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadMeta {
    /// SICP type tag: "claude-code" or "pi-mono".
    pub agent: String,
}

// ── The Lift: Agent Payload ────────────────────────────────────────

/// Tagged union of agent-specific payloads.
/// Dispatch on `meta.agent` to determine variant.
///
/// Serializes as: `{ "meta": { "agent": "claude-code" }, "text": "...", ... }`
/// The tag is inside the payload, making it self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "_variant")]
pub enum AgentPayload {
    #[serde(rename = "claude-code")]
    ClaudeCode(ClaudeCodePayload),
    #[serde(rename = "pi-mono")]
    PiMono(PiMonoPayload),
}

// ── Claude Code Payload ────────────────────────────────────────────

/// Typed extraction for Claude Code events.
/// All fields translated from the agent transcript — no interpretation.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// ── Convenience accessors ──────────────────────────────────────────

impl AgentPayload {
    /// Get the agent tag string.
    pub fn agent(&self) -> &str {
        match self {
            AgentPayload::ClaudeCode(p) => &p.meta.agent,
            AgentPayload::PiMono(p) => &p.meta.agent,
        }
    }

    /// Get text from either payload variant.
    pub fn text(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.text.as_deref(),
            AgentPayload::PiMono(p) => p.text.as_deref(),
        }
    }

    /// Get model from either payload variant.
    pub fn model(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.model.as_deref(),
            AgentPayload::PiMono(p) => p.model.as_deref(),
        }
    }

    /// Get tool name from either payload variant.
    pub fn tool(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.tool.as_deref(),
            AgentPayload::PiMono(p) => p.tool.as_deref(),
        }
    }

    /// Get tool args from either payload variant.
    pub fn args(&self) -> Option<&Value> {
        match self {
            AgentPayload::ClaudeCode(p) => p.args.as_ref(),
            AgentPayload::PiMono(p) => p.args.as_ref(),
        }
    }

    /// Get token usage from either payload variant.
    pub fn token_usage(&self) -> Option<&Value> {
        match self {
            AgentPayload::ClaudeCode(p) => p.token_usage.as_ref(),
            AgentPayload::PiMono(p) => p.token_usage.as_ref(),
        }
    }

    /// Get uuid from either payload variant.
    pub fn uuid(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.uuid.as_deref(),
            AgentPayload::PiMono(p) => p.uuid.as_deref(),
        }
    }

    /// Get parent_uuid from either payload variant.
    pub fn parent_uuid(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.parent_uuid.as_deref(),
            AgentPayload::PiMono(p) => p.parent_uuid.as_deref(),
        }
    }

    /// Get cwd from either payload variant.
    pub fn cwd(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.cwd.as_deref(),
            AgentPayload::PiMono(p) => p.cwd.as_deref(),
        }
    }

    /// Get stop_reason as a string from either variant.
    pub fn stop_reason_str(&self) -> Option<&str> {
        match self {
            AgentPayload::ClaudeCode(p) => p.stop_reason.as_ref().and_then(|v| v.as_str()),
            AgentPayload::PiMono(p) => p.stop_reason.as_deref(),
        }
    }

    /// Get content_types from either variant.
    pub fn content_types(&self) -> Option<&[String]> {
        match self {
            AgentPayload::ClaudeCode(p) => p.content_types.as_deref(),
            AgentPayload::PiMono(p) => p.content_types.as_deref(),
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
