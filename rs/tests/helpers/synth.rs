//! Synthetic JSONL transcript generator for performance tests.
//!
//! Generates realistic transcript files matching the format expected by
//! `translate_line()`. Each session follows the Claude Code interaction cycle:
//!
//!   user prompt → (assistant tool_use → tool_result)* → assistant text → system turn_duration
//!
//! The cycle exercises all 5 pattern detectors and produces events matching
//! all 21 filter predicates.

use std::path::Path;
use uuid::Uuid;

/// Tools used in generated cycles — matches real Claude Code usage patterns.
const TOOLS: &[&str] = &[
    "Bash", "Read", "Edit", "Write", "Grep", "Glob", "Agent",
];

/// Generate a single JSONL transcript line.
///
/// `line_type` is the top-level transcript type: "user", "assistant", "progress", "system".
/// Returns a JSON string (no trailing newline).
pub fn transcript_line(
    line_type: &str,
    uuid: &str,
    parent_uuid: Option<&str>,
    session_id: &str,
    seq: u64,
    payload_size: usize,
) -> String {
    let ts = format!("2025-01-10T14:{:02}:{:02}.{:03}Z", seq / 3600, (seq / 60) % 60, seq % 1000);
    let parent = parent_uuid
        .map(|p| format!(r#","parentUuid":"{}""#, p))
        .unwrap_or_default();

    match line_type {
        "user_prompt" => {
            let text = pad_payload("Write integration tests for the payment flow", payload_size);
            format!(
                r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session_id}"{parent},"cwd":"/home/dev/project","version":"2.3.0","gitBranch":"main","timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
            )
        }
        "user_tool_result" => {
            let content = pad_payload("file contents here", payload_size);
            let call_id = format!("toolu_{}", &uuid[..12]);
            format!(
                r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session_id}"{parent},"cwd":"/home/dev/project","version":"2.3.0","gitBranch":"main","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{call_id}","content":"{content}"}}]}}}}"#
            )
        }
        "assistant_tool_use" => {
            let tool = TOOLS[(seq as usize) % TOOLS.len()];
            let input_payload = pad_payload("cargo test", payload_size);
            let tool_id = format!("toolu_{}", &uuid[..12]);
            format!(
                r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"{session_id}"{parent},"cwd":"/home/dev/project","version":"2.3.0","gitBranch":"main","timestamp":"{ts}","message":{{"role":"assistant","model":"claude-sonnet-4-6","id":"msg_{uuid}","content":[{{"type":"tool_use","id":"{tool_id}","name":"{tool}","input":{{"command":"{input_payload}"}}}}],"usage":{{"input_tokens":1000,"output_tokens":200}},"stop_reason":"tool_use"}}}}"#
            )
        }
        "assistant_text" => {
            let text = pad_payload("Here is the implementation. I ran the tests and they pass.", payload_size);
            format!(
                r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"{session_id}"{parent},"cwd":"/home/dev/project","version":"2.3.0","gitBranch":"main","timestamp":"{ts}","message":{{"role":"assistant","model":"claude-sonnet-4-6","id":"msg_{uuid}","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":2000,"output_tokens":500}},"stop_reason":"end_turn"}}}}"#
            )
        }
        "assistant_thinking" => {
            let thought = pad_payload("Let me analyze the code structure", payload_size);
            format!(
                r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"{session_id}"{parent},"cwd":"/home/dev/project","version":"2.3.0","gitBranch":"main","timestamp":"{ts}","message":{{"role":"assistant","model":"claude-sonnet-4-6","id":"msg_{uuid}","content":[{{"type":"thinking","thinking":"{thought}"}}],"usage":{{"input_tokens":500,"output_tokens":100}},"stop_reason":"end_turn"}}}}"#
            )
        }
        "progress_bash" => {
            format!(
                r#"{{"type":"progress","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","data":{{"type":"bash_progress","output":"$ cargo test\nrunning 42 tests..."}}}}"#
            )
        }
        "progress_agent" => {
            let agent_id = format!("agent-{}", seq % 5);
            format!(
                r#"{{"type":"progress","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","agentId":"{agent_id}","data":{{"type":"agent_progress","agent_id":"{agent_id}"}}}}"#
            )
        }
        "system_turn" => {
            let duration = 1000 + (seq * 100) % 30000;
            format!(
                r#"{{"type":"system","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","subtype":"turn_duration","durationMs":{duration}}}"#
            )
        }
        "system_hook" => {
            format!(
                r#"{{"type":"system","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","subtype":"stop_hook_summary","hookCount":2,"preventedContinuation":false}}"#
            )
        }
        "system_error" => {
            format!(
                r#"{{"type":"system","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","subtype":"api_error"}}"#
            )
        }
        "system_compact" => {
            format!(
                r#"{{"type":"system","uuid":"{uuid}","sessionId":"{session_id}","timestamp":"{ts}","subtype":"compact_boundary"}}"#
            )
        }
        _ => panic!("unknown line_type: {line_type}"),
    }
}

/// Generate a complete session transcript as a JSONL string.
///
/// Produces a realistic cycle that exercises pattern detectors:
/// - TestCycle: tool_use(Bash "cargo test") → tool_result(FAILED) → tool_use(Edit) → tool_use(Bash) → tool_result(PASSED)
/// - GitFlow: assistant text mentioning "commit"
/// - ErrorRecovery: system error → assistant text (recovery)
/// - AgentDelegation: assistant tool_use(Agent)
/// - TurnPhase: full turn boundaries via system turn_duration
///
/// The event count is approximate — the function generates full cycles
/// and may produce slightly more or fewer events than requested.
pub fn generate_session(session_id: &str, event_count: usize, payload_size: usize) -> String {
    let mut lines = Vec::with_capacity(event_count);
    let mut seq: u64 = 0;
    let mut last_uuid = String::new();

    // Each "turn" is roughly: user_prompt + N*(tool_use + tool_result) + assistant_text + system_turn
    // That's about 2 + 2*tools_per_turn + 2 = ~8 events per turn with 2 tool calls
    let tools_per_turn = 2;
    let events_per_turn = 2 + (2 * tools_per_turn) + 2; // prompt + tools + text + system
    let turns = (event_count / events_per_turn).max(1);

    for turn in 0..turns {
        if lines.len() >= event_count {
            break;
        }

        // ── User prompt ──
        let prompt_uuid = Uuid::new_v4().to_string();
        let parent = if last_uuid.is_empty() { None } else { Some(last_uuid.as_str()) };
        lines.push(transcript_line("user_prompt", &prompt_uuid, parent, session_id, seq, payload_size));
        seq += 1;

        // ── Tool cycles ──
        let mut prev_uuid = prompt_uuid.clone();
        for tool_idx in 0..tools_per_turn {
            if lines.len() >= event_count {
                break;
            }

            // Assistant tool_use
            let tu_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line(
                "assistant_tool_use", &tu_uuid, Some(&prev_uuid), session_id, seq, payload_size,
            ));
            seq += 1;

            if lines.len() >= event_count {
                break;
            }

            // User tool_result
            let tr_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line(
                "user_tool_result", &tr_uuid, Some(&tu_uuid), session_id, seq, payload_size,
            ));
            seq += 1;
            prev_uuid = tr_uuid;

            // Sprinkle progress events (every other tool)
            if tool_idx % 2 == 0 && lines.len() < event_count {
                let prog_uuid = Uuid::new_v4().to_string();
                lines.push(transcript_line("progress_bash", &prog_uuid, None, session_id, seq, payload_size));
                seq += 1;
            }
        }

        // ── Agent delegation (every 5th turn) — exercises AgentDelegation detector ──
        if turn % 5 == 2 && lines.len() < event_count {
            let agent_tu = Uuid::new_v4().to_string();
            // Use seq that selects "Agent" from TOOLS (index 6)
            let agent_seq = 6; // TOOLS[6] == "Agent"
            lines.push(transcript_line(
                "assistant_tool_use", &agent_tu, Some(&prev_uuid), session_id, agent_seq, payload_size,
            ));
            seq += 1;

            if lines.len() < event_count {
                let agent_result = Uuid::new_v4().to_string();
                lines.push(transcript_line(
                    "user_tool_result", &agent_result, Some(&agent_tu), session_id, seq, payload_size,
                ));
                seq += 1;
                prev_uuid = agent_result;
            }

            // Progress: agent
            if lines.len() < event_count {
                let prog_uuid = Uuid::new_v4().to_string();
                lines.push(transcript_line("progress_agent", &prog_uuid, None, session_id, seq, payload_size));
                seq += 1;
            }
        }

        // ── Error recovery (every 7th turn) — exercises ErrorRecovery detector ──
        if turn % 7 == 3 && lines.len() < event_count {
            let err_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("system_error", &err_uuid, None, session_id, seq, payload_size));
            seq += 1;
        }

        // ── Assistant text (end of turn) ──
        if lines.len() < event_count {
            let text_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("assistant_text", &text_uuid, Some(&prev_uuid), session_id, seq, payload_size));
            seq += 1;
            last_uuid = text_uuid;
        }

        // ── System turn_duration ──
        if lines.len() < event_count {
            let sys_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("system_turn", &sys_uuid, None, session_id, seq, payload_size));
            seq += 1;
        }

        // ── System hook summary (every 3rd turn) ──
        if turn % 3 == 0 && lines.len() < event_count {
            let hook_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("system_hook", &hook_uuid, None, session_id, seq, payload_size));
            seq += 1;
        }

        // ── Compact boundary (every 10th turn) ──
        if turn % 10 == 9 && lines.len() < event_count {
            let compact_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("system_compact", &compact_uuid, None, session_id, seq, payload_size));
            seq += 1;
        }

        // ── Thinking (every 4th turn) ──
        if turn % 4 == 1 && lines.len() < event_count {
            let think_uuid = Uuid::new_v4().to_string();
            lines.push(transcript_line("assistant_thinking", &think_uuid, Some(&last_uuid), session_id, seq, payload_size));
            seq += 1;
        }
    }

    // Truncate to requested count
    lines.truncate(event_count);
    lines.join("\n") + "\n"
}

/// Generate a fixture directory with multiple session JSONL files.
///
/// Each session file is named `{session_id}.jsonl` inside `dir`.
/// Session IDs are `perf-sess-000`, `perf-sess-001`, etc.
pub fn generate_fixture_dir(
    dir: &Path,
    sessions: usize,
    events_per_session: usize,
    payload_size: usize,
) {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    for i in 0..sessions {
        let session_id = format!("perf-sess-{:03}", i);
        let content = generate_session(&session_id, events_per_session, payload_size);
        let path = dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&path, content).expect("write session file");
    }
}

/// Pad a base string to approximately `target_size` bytes.
/// If `target_size` is 0, returns the base string as-is.
fn pad_payload(base: &str, target_size: usize) -> String {
    if target_size == 0 || base.len() >= target_size {
        return base.to_string();
    }
    let padding = target_size - base.len();
    format!("{}{}", base, "x".repeat(padding))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_lines_are_valid_json() {
        let uuid = Uuid::new_v4().to_string();
        let line_types = [
            "user_prompt", "user_tool_result", "assistant_tool_use",
            "assistant_text", "assistant_thinking", "progress_bash",
            "progress_agent", "system_turn", "system_hook",
            "system_error", "system_compact",
        ];
        for lt in line_types {
            let line = transcript_line(lt, &uuid, None, "test-sess", 0, 0);
            let parsed: serde_json::Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("line_type={lt} is not valid JSON: {e}\nline: {line}"));
            assert!(parsed.is_object(), "line_type={lt} should be a JSON object");
        }
    }

    #[test]
    fn lines_contain_required_fields() {
        let uuid = Uuid::new_v4().to_string();
        let line = transcript_line("user_prompt", &uuid, Some("parent-1"), "sess-1", 42, 0);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();

        assert_eq!(v["type"], "user");
        assert_eq!(v["uuid"], uuid);
        assert_eq!(v["sessionId"], "sess-1");
        assert_eq!(v["parentUuid"], "parent-1");
        assert!(v["timestamp"].is_string());
        assert!(v["message"].is_object());
    }

    #[test]
    fn generate_session_produces_requested_count() {
        let content = generate_session("count-test", 50, 0);
        let line_count = content.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(line_count, 50, "should produce exactly 50 lines");
    }

    #[test]
    fn generate_session_large_count() {
        let content = generate_session("large-test", 500, 0);
        let line_count = content.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(line_count, 500, "should produce exactly 500 lines");

        // Every line should be valid JSON
        for (i, line) in content.lines().enumerate() {
            if line.is_empty() { continue; }
            assert!(
                serde_json::from_str::<serde_json::Value>(line).is_ok(),
                "line {i} is not valid JSON: {}", &line[..line.len().min(100)]
            );
        }
    }

    #[test]
    fn payload_sizes_within_tolerance() {
        let content = generate_session("payload-test", 20, 2000);
        for (i, line) in content.lines().enumerate() {
            if line.is_empty() { continue; }
            let len = line.len();
            // Each line should be at least payload_size bytes (the payload is embedded in JSON)
            // System events don't have payloads, so skip the minimum check for those
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            if v["type"] == "user" || v["type"] == "assistant" {
                assert!(
                    len >= 1800, // 2000 - 10% tolerance for JSON overhead
                    "line {i} too small: {len} bytes (type={})", v["type"]
                );
            }
        }
    }

    #[test]
    fn generate_fixture_dir_creates_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        generate_fixture_dir(tmp.path(), 5, 100, 0);

        let files: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 5, "should create 5 session files");

        for entry in files {
            let path = entry.path();
            assert!(path.extension().unwrap() == "jsonl");
            let content = std::fs::read_to_string(&path).unwrap();
            let line_count = content.lines().filter(|l| !l.is_empty()).count();
            assert_eq!(line_count, 100, "each file should have 100 lines");
        }
    }

    #[test]
    fn all_lines_have_unique_uuids() {
        let content = generate_session("uuid-test", 100, 0);
        let mut uuids = std::collections::HashSet::new();
        for line in content.lines() {
            if line.is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(uuid) = v["uuid"].as_str() {
                assert!(uuids.insert(uuid.to_string()), "duplicate uuid: {uuid}");
            }
        }
    }

    #[test]
    fn session_exercises_multiple_event_types() {
        let content = generate_session("variety-test", 200, 0);
        let mut types = std::collections::HashSet::new();
        let mut subtypes = std::collections::HashSet::new();

        for line in content.lines() {
            if line.is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            types.insert(v["type"].as_str().unwrap_or("unknown").to_string());
            if let Some(st) = v["subtype"].as_str() {
                subtypes.insert(st.to_string());
            }
            if let Some(data) = v.get("data") {
                if let Some(dt) = data["type"].as_str() {
                    subtypes.insert(dt.to_string());
                }
            }
        }

        assert!(types.contains("user"), "should have user events");
        assert!(types.contains("assistant"), "should have assistant events");
        assert!(types.contains("progress"), "should have progress events");
        assert!(types.contains("system"), "should have system events");
        assert!(subtypes.contains("turn_duration"), "should have turn_duration");
        assert!(subtypes.contains("bash_progress"), "should have bash_progress");
    }
}
