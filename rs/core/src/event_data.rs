//! Typed payload for CloudEvent.data — open schema with named common fields.
//!
//! Agent-specific and unknown fields land in `extra` via `#[serde(flatten)]`,
//! so the JSON shape is identical to the old `Value::Object(Map)` format.
//! New fields from evolving agent protocols are captured automatically.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed event data payload with open schema.
///
/// Common fields shared by all agents are named. Everything else
/// (agent-specific keys, future additions) flows into `extra`.
/// Serialization produces the same flat JSON as the old untyped Map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    // ── Always present ──────────────────────────────
    /// Original transcript line, never mutated.
    pub raw: Value,
    /// Sequence number within the translation session.
    pub seq: u64,
    /// Session identifier.
    pub session_id: String,

    // ── Common optional fields (both agents) ────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Token usage — stays as Value because internal keys differ by agent
    /// (Claude Code: `input_tokens`, pi-mono: `input`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_types: Option<Vec<String>>,

    // ── Open schema catch-all ───────────────────────
    /// Agent-specific fields, new fields, anything not matched above.
    /// `#[serde(flatten)]` keeps the JSON shape flat — no nesting.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl EventData {
    /// Create a new EventData with the three required fields.
    pub fn new(raw: Value, seq: u64, session_id: String) -> Self {
        Self {
            raw,
            seq,
            session_id,
            uuid: None,
            parent_uuid: None,
            cwd: None,
            text: None,
            model: None,
            stop_reason: None,
            token_usage: None,
            tool: None,
            args: None,
            content_types: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Merge a map of key-value pairs, routing known keys to named fields
    /// and unknown keys to `extra`.
    pub fn merge(&mut self, map: serde_json::Map<String, Value>) {
        for (k, v) in map {
            match k.as_str() {
                "raw" => self.raw = v,
                "seq" => {
                    if let Some(n) = v.as_u64() {
                        self.seq = n;
                    }
                }
                "session_id" => {
                    if let Some(s) = v.as_str() {
                        self.session_id = s.to_string();
                    }
                }
                "uuid" => self.uuid = v.as_str().map(|s| s.to_string()),
                "parent_uuid" => self.parent_uuid = v.as_str().map(|s| s.to_string()),
                "cwd" => self.cwd = v.as_str().map(|s| s.to_string()),
                "text" => self.text = v.as_str().map(|s| s.to_string()),
                "model" => self.model = v.as_str().map(|s| s.to_string()),
                "stop_reason" => self.stop_reason = v.as_str().map(|s| s.to_string()),
                "token_usage" => self.token_usage = Some(v),
                "tool" => self.tool = v.as_str().map(|s| s.to_string()),
                "args" => self.args = Some(v),
                "content_types" => {
                    if let Value::Array(arr) = &v {
                        let strings: Vec<String> = arr
                            .iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect();
                        self.content_types = Some(strings);
                    }
                }
                _ => {
                    self.extra.insert(k, v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_round_trip_matches_old_value_format() {
        // Build the same data the old way (Value::Object(Map))
        let mut old_map = serde_json::Map::new();
        old_map.insert("raw".to_string(), json!({"message": "hello"}));
        old_map.insert("seq".to_string(), json!(1));
        old_map.insert("session_id".to_string(), json!("sess-123"));
        old_map.insert("model".to_string(), json!("claude-opus-4-6"));
        old_map.insert("text".to_string(), json!("Hello world"));
        old_map.insert("agent_id".to_string(), json!("agent-abc")); // goes to extra

        // Build the new way
        let mut data = EventData::new(json!({"message": "hello"}), 1, "sess-123".to_string());
        data.model = Some("claude-opus-4-6".to_string());
        data.text = Some("Hello world".to_string());
        data.extra
            .insert("agent_id".to_string(), json!("agent-abc"));

        // Serialize both and compare
        let old_json = serde_json::to_value(Value::Object(old_map)).unwrap();
        let new_json = serde_json::to_value(&data).unwrap();
        assert_eq!(old_json, new_json, "Serialization must produce identical JSON");
    }

    #[test]
    fn test_deserialize_old_format_json() {
        let old_json = json!({
            "raw": {"message": "hello"},
            "seq": 5,
            "session_id": "sess-456",
            "uuid": "u-001",
            "model": "claude-opus-4-6",
            "token_usage": {"input_tokens": 100},
            "agent_id": "agent-xyz",
            "is_sidechain": false
        });

        let data: EventData = serde_json::from_value(old_json).unwrap();
        assert_eq!(data.seq, 5);
        assert_eq!(data.session_id, "sess-456");
        assert_eq!(data.uuid, Some("u-001".to_string()));
        assert_eq!(data.model, Some("claude-opus-4-6".to_string()));
        assert!(data.token_usage.is_some());
        // Agent-specific fields land in extra
        assert_eq!(data.extra.get("agent_id"), Some(&json!("agent-xyz")));
        assert_eq!(data.extra.get("is_sidechain"), Some(&json!(false)));
    }

    #[test]
    fn test_unknown_fields_captured_in_extra() {
        let json_with_new_fields = json!({
            "raw": {},
            "seq": 1,
            "session_id": "s",
            "some_future_field": "surprise!",
            "another_new_thing": 42
        });

        let data: EventData = serde_json::from_value(json_with_new_fields).unwrap();
        assert_eq!(
            data.extra.get("some_future_field"),
            Some(&json!("surprise!"))
        );
        assert_eq!(data.extra.get("another_new_thing"), Some(&json!(42)));
    }

    #[test]
    fn test_merge_routes_known_and_unknown_keys() {
        let mut data = EventData::new(json!({}), 1, "s".to_string());
        let mut map = serde_json::Map::new();
        map.insert("model".to_string(), json!("gpt-4"));
        map.insert("text".to_string(), json!("hi"));
        map.insert("agent_id".to_string(), json!("a-1"));
        map.insert("token_usage".to_string(), json!({"input": 50}));

        data.merge(map);

        assert_eq!(data.model, Some("gpt-4".to_string()));
        assert_eq!(data.text, Some("hi".to_string()));
        assert_eq!(data.token_usage, Some(json!({"input": 50})));
        assert_eq!(data.extra.get("agent_id"), Some(&json!("a-1")));
    }

    #[test]
    fn test_merge_envelope_overrides_session_id() {
        let mut data = EventData::new(json!({}), 1, "file-session".to_string());
        let mut envelope = serde_json::Map::new();
        envelope.insert("session_id".to_string(), json!("real-session"));
        envelope.insert("uuid".to_string(), json!("u-001"));

        data.merge(envelope);

        assert_eq!(data.session_id, "real-session");
        assert_eq!(data.uuid, Some("u-001".to_string()));
    }

    #[test]
    fn test_new_creates_minimal_valid_data() {
        let data = EventData::new(json!({"line": 1}), 0, "test".to_string());
        assert_eq!(data.seq, 0);
        assert!(data.model.is_none());
        assert!(data.extra.is_empty());

        // Serializes without optional fields
        let json = serde_json::to_value(&data).unwrap();
        assert!(!json.as_object().unwrap().contains_key("model"));
        assert!(!json.as_object().unwrap().contains_key("text"));
    }
}
