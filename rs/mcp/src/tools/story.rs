//! Native `session_story` — composes EventStore reads into the same
//! fact-sheet shape `scripts/sessionstory.py` produces, without
//! shelling out to a Python subprocess.
//!
//! The Python script remains the canonical spec — this is a port that
//! must match the JSON output schema field-for-field so any consumer
//! that worked against the old Python session_story works against the
//! new Rust one.

use open_story_store::event_store::EventStore;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Default)]
pub struct PromptLine {
    pub timestamp: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SessionFacts {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_hours: f64,

    pub total_records: u64,
    pub record_type_counts: BTreeMap<String, u64>,
    pub tool_call_counts: BTreeMap<String, u64>,
    pub turn_count: u64,
    pub sidechain_count: u64,

    pub opening_prompt: Option<PromptLine>,
    pub closing_prompt: Option<PromptLine>,
    pub prompt_timeline: Vec<PromptLine>,

    pub pattern_total: u64,
    pub pattern_type_counts: BTreeMap<String, u64>,
    pub turn_phase_counts: BTreeMap<String, u64>,
    pub sample_sentences: Vec<String>,
    pub error_recovery_count: u64,
    pub test_cycle_count: u64,

    pub trailing_assistant: Vec<PromptLine>,
}

pub fn session_story_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Session UUID"},
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

/// Map an OpenStory event subtype to the scripts/sessionstory.py
/// record_type vocabulary, so the JSON schema stays stable across
/// the Python and Rust implementations.
fn record_type_for(subtype: &str) -> &'static str {
    match subtype {
        "message.user.prompt" => "user_message",
        "message.user.tool_result" => "tool_result",
        "message.assistant.text" => "assistant_message",
        "message.assistant.tool_use" => "tool_call",
        "message.assistant.thinking" => "thinking",
        "system.turn.complete" => "turn_end",
        "system.error" => "error",
        "system.compact" => "compact",
        "system.hook" => "hook",
        "system.session_start" => "session_start",
        "system.model_change" => "model_change",
        "progress.bash" => "progress",
        "progress.agent" => "progress",
        "progress.hook" => "progress",
        "file.snapshot" => "file_snapshot",
        "queue.enqueue" => "queue",
        "queue.dequeue" => "queue",
        _ => "other",
    }
}

fn extract_text(payload: &Value) -> Option<String> {
    // Try common shapes: message.content (string), message.content[].text,
    // message.text (string)
    if let Some(msg) = payload.get("message") {
        if let Some(s) = msg.get("content").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
        if let Some(arr) = msg.get("content").and_then(|v| v.as_array()) {
            let mut buf = String::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(t);
                }
            }
            if !buf.is_empty() {
                return Some(buf);
            }
        }
        if let Some(s) = msg.get("text").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    if let Some(s) = payload.get("text").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    None
}

fn is_noise(content: &str) -> bool {
    content.trim().is_empty()
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        // Truncate at a char boundary to avoid panicking on multi-byte chars.
        let mut end = n;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

pub async fn session_story(
    store: &Arc<dyn EventStore>,
    args: Value,
) -> Result<Value, String> {
    let session_id = args
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "session_story requires `session_id`".to_string())?;

    let events = store
        .session_events(session_id)
        .await
        .map_err(|e| format!("session_events failed: {e}"))?;

    let mut facts = SessionFacts {
        session_id: session_id.to_string(),
        total_records: events.len() as u64,
        ..Default::default()
    };

    // ── walk events ──
    for ev in &events {
        let subtype = ev.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
        let record_type = record_type_for(subtype).to_string();
        *facts.record_type_counts.entry(record_type.clone()).or_insert(0) += 1;

        let data = ev.get("data").cloned().unwrap_or(Value::Null);
        let raw = data.get("raw").cloned().unwrap_or_else(|| data.clone());

        match record_type.as_str() {
            "turn_end" => facts.turn_count += 1,
            "tool_call" => {
                // Tool name lives at data.agent_payload.tool OR
                // data.raw.message.content[].name. Try both.
                let name = data
                    .get("agent_payload")
                    .and_then(|p| p.get("tool"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        raw.get("message")
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("tool_use")))
                            .and_then(|tu| tu.get("name").and_then(|v| v.as_str()).map(str::to_string))
                    })
                    .unwrap_or_else(|| "?".to_string());
                *facts.tool_call_counts.entry(name).or_insert(0) += 1;
            }
            "user_message" => {
                if let Some(text) = extract_text(&raw) {
                    if !is_noise(&text) {
                        let ts = ev
                            .get("time")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        facts.prompt_timeline.push(PromptLine {
                            timestamp: ts,
                            content: truncate(&text, 200),
                        });
                    }
                }
            }
            "assistant_message" => {
                if let Some(text) = extract_text(&raw) {
                    let ts = ev
                        .get("time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    facts.trailing_assistant.push(PromptLine {
                        timestamp: ts,
                        content: truncate(&text, 300),
                    });
                }
            }
            _ => {}
        }

        if ev
            .get("data")
            .and_then(|d| d.get("is_sidechain"))
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            facts.sidechain_count += 1;
        }
    }

    facts.opening_prompt = facts.prompt_timeline.first().cloned();
    facts.closing_prompt = facts.prompt_timeline.last().cloned();

    // ── started/ended/duration ──
    if let (Some(first), Some(last)) = (events.first(), events.last()) {
        let started = first
            .get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ended = last
            .get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let (Ok(t0), Ok(t1)) = (
            chrono::DateTime::parse_from_rfc3339(&started),
            chrono::DateTime::parse_from_rfc3339(&ended),
        ) {
            let secs = (t1 - t0).num_seconds() as f64;
            facts.duration_hours = (secs / 3600.0 * 100.0).round() / 100.0;
        }
        facts.started_at = started;
        facts.ended_at = ended;
    }

    // ── walk patterns ──
    let patterns = store
        .session_patterns(session_id, None)
        .await
        .map_err(|e| format!("session_patterns failed: {e}"))?;
    facts.pattern_total = patterns.len() as u64;
    for p in &patterns {
        *facts.pattern_type_counts.entry(p.pattern_type.clone()).or_insert(0) += 1;
        match p.pattern_type.as_str() {
            "turn.phase" => {
                if let Some(phase) = p.metadata.get("phase").and_then(|v| v.as_str()) {
                    *facts.turn_phase_counts.entry(phase.to_string()).or_insert(0) += 1;
                }
            }
            "turn.sentence"
                if facts.sample_sentences.len() < 8 && !p.summary.is_empty() =>
            {
                facts.sample_sentences.push(p.summary.clone());
            }
            "error.recovery" => facts.error_recovery_count += 1,
            "test.cycle" => facts.test_cycle_count += 1,
            _ => {}
        }
    }

    serde_json::to_value(facts).map_err(|e| format!("serialize: {e}"))
}
