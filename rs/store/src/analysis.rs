//! Pure analysis functions for arc event streams.
//!
//! Supports unified io.arc.event type (with hierarchical subtypes),
//! plus legacy hook event types (io.arc.session.start, etc.) and
//! legacy transcript event types (io.arc.transcript.assistant, etc.).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub status: String,
    pub start_time: Option<String>,
    pub duration_ms: Option<f64>,
    pub event_count: usize,
    pub error_count: usize,
    pub tool_calls: usize,
    pub files_edited: usize,
    pub unique_tools: Vec<String>,
    pub exit_code: Option<i64>,
    pub model: Option<String>,
    pub prompt_count: usize,
    pub response_count: usize,
    pub first_prompt: Option<String>,
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Classify event type for analysis purposes.
enum EventKind {
    SessionStart,
    SessionEnd,
    Error,
    ToolCall,
    FileEdit,
    PromptSubmit,
    ResponseComplete,
    Other,
}

fn classify_event(etype: &str, event: &Value) -> EventKind {
    let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

    // Unified io.arc.event type — classify by hierarchical subtype
    if etype == "io.arc.event" {
        if subtype.starts_with("message.user.") {
            if subtype == "message.user.tool_result" {
                return EventKind::Other;
            }
            return EventKind::PromptSubmit;
        }
        if subtype.starts_with("message.assistant.") {
            if subtype == "message.assistant.tool_use" {
                return EventKind::ToolCall;
            }
            return EventKind::ResponseComplete;
        }
        if subtype == "system.turn.complete" {
            return EventKind::Other;
        }
        if subtype == "system.error" {
            return EventKind::Error;
        }
        if subtype == "system.session.start" {
            return EventKind::SessionStart;
        }
        if subtype == "system.session.end" {
            return EventKind::SessionEnd;
        }
        if subtype.starts_with("file.edit") {
            return EventKind::FileEdit;
        }
        return EventKind::Other;
    }

    // Legacy: hook event types
    if etype.ends_with(".session.start") {
        return EventKind::SessionStart;
    }
    if etype.ends_with(".session.end") {
        return EventKind::SessionEnd;
    }
    if etype.ends_with(".error") {
        return EventKind::Error;
    }
    if etype.ends_with(".tool.call") {
        return EventKind::ToolCall;
    }
    if etype.ends_with(".file.edit") {
        return EventKind::FileEdit;
    }
    if etype.ends_with(".prompt.submit") {
        return EventKind::PromptSubmit;
    }
    if etype.ends_with(".response.complete") {
        return EventKind::ResponseComplete;
    }

    // Legacy: transcript event types
    if etype.ends_with(".transcript.user") {
        if subtype == "tool_result" || subtype == "message.user.tool_result" {
            return EventKind::Other;
        }
        return EventKind::PromptSubmit;
    }
    if etype.ends_with(".transcript.assistant") {
        if subtype == "tool_use" || subtype == "message.assistant.tool_use" {
            return EventKind::ToolCall;
        }
        return EventKind::ResponseComplete;
    }
    if etype.ends_with(".transcript.system") {
        if subtype == "turn_duration" || subtype == "system.turn.complete" {
            return EventKind::Other;
        }
        if subtype.contains("error") {
            return EventKind::Error;
        }
        return EventKind::Other;
    }

    EventKind::Other
}

/// Extract model from event data, supporting both hook and transcript formats.
fn extract_model(event: &Value) -> Option<String> {
    let data = event.get("data")?;
    // Hook format: data.model
    if let Some(m) = data.get("model").and_then(|v| v.as_str()) {
        return Some(m.to_string());
    }
    // Transcript format: data.raw.message.model
    if let Some(m) = data.get("raw")
        .and_then(|r| r.get("message"))
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
    {
        return Some(m.to_string());
    }
    None
}

/// Extract prompt text from event data, supporting both formats.
fn extract_prompt_text(event: &Value) -> Option<String> {
    let data = event.get("data")?;
    // Hook format: data.text
    if let Some(t) = data.get("text").and_then(|v| v.as_str()) {
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    // Transcript format: data.raw.message.content (array of blocks or string)
    let raw = data.get("raw")?;
    let message = raw.get("message")?;
    let content = message.get("content")?;
    match content {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(blocks) => {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        if !t.is_empty() {
                            return Some(t.to_string());
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract tool names from a transcript assistant event with tool_use subtype.
fn extract_tool_names(event: &Value) -> Vec<String> {
    let data = match event.get("data") {
        Some(d) => d,
        None => return vec![],
    };

    // Hook format: data.tool
    if let Some(tool) = data.get("tool").and_then(|v| v.as_str()) {
        return vec![tool.to_string()];
    }

    // Transcript format: content blocks with type=tool_use
    let raw = match data.get("raw") {
        Some(r) => r,
        None => return vec![],
    };
    let message = match raw.get("message") {
        Some(m) => m,
        None => return vec![],
    };
    let content = match message.get("content") {
        Some(c) => c,
        None => return vec![],
    };

    let mut tools = Vec::new();
    if let Value::Array(blocks) = content {
        for block in blocks {
            if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
                    tools.push(name.to_string());
                }
            }
        }
    }
    tools
}

/// Extract CWD from event data, supporting both formats.
pub fn extract_cwd(event: &Value) -> Option<String> {
    let data = event.get("data")?;
    // Hook format: data.meta.cwd or data.cwd
    if let Some(cwd) = data.get("meta").and_then(|m| m.get("cwd")).and_then(|v| v.as_str()) {
        return Some(cwd.to_string());
    }
    if let Some(cwd) = data.get("cwd").and_then(|v| v.as_str()) {
        return Some(cwd.to_string());
    }
    // Transcript format: data.raw.cwd
    if let Some(cwd) = data.get("raw").and_then(|r| r.get("cwd")).and_then(|v| v.as_str()) {
        return Some(cwd.to_string());
    }
    None
}

/// Derive project_id from a cwd path (last non-empty path segment).
/// DEPRECATED: use `resolve_project` for correct subdirectory handling.
pub fn project_id_from_cwd(cwd: &str) -> Option<String> {
    // Split on both / and \ to handle Unix and Windows paths
    cwd.rsplit(&['/', '\\'][..])
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Encode a filesystem path to Claude Code's `.claude/projects/` directory name format.
///
/// Claude Code encodes absolute paths by replacing path separators with `-`
/// and `:` with `-`, producing names like `C--Users-dev-projects-my-project`.
pub fn encode_path_as_dir_name(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut result = String::new();
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();

    for (i, seg) in segments.iter().enumerate() {
        if i == 0 && seg.ends_with(':') {
            // Windows drive letter: "C:" → "C-"
            result.push_str(&seg[..seg.len() - 1]);
            result.push('-');
        } else {
            if !result.is_empty() || normalized.starts_with('/') {
                result.push('-');
            }
            result.push_str(seg);
        }
    }
    result
}

/// Resolved project identity: canonical grouping key + human-readable name.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProject {
    pub project_id: String,
    pub project_name: String,
}

/// Strip `--claude-worktrees-<name>` suffix from a watch directory entry.
///
/// Worktree entries like `C--Users-dev-projects-my-project--claude-worktrees-purrfect-noodling-badger`
/// should group with their parent project `C--Users-dev-projects-my-project`.
pub fn strip_worktree_suffix(entry: &str) -> &str {
    if let Some(pos) = entry.find("--claude-worktrees-") {
        &entry[..pos]
    } else {
        entry
    }
}

/// Derive a human-readable display name from a `.claude/projects/` directory name,
/// optionally using a cwd for accurate path-based extraction.
///
/// When a cwd is available, uses proper path parsing to find the project root.
/// Otherwise, falls back to pattern matching on the encoded directory name.
///
/// Examples:
/// - `("C--Users-dev-projects-my-project", Some("C:\\Users\\dev\\projects\\my-project\\ui"))` → `my-project`
/// - `("C--Users-dev-projects-my-project", None)` → `my-project` (via `-projects-` pattern)
/// - `("-workspace", None)` → `workspace`
pub fn display_name_from_entry(entry: &str, _all_entries: &[String]) -> String {
    display_name_from_entry_and_cwd(entry, None)
}

/// Display name with optional cwd for path-based extraction.
pub fn display_name_from_entry_and_cwd(entry: &str, cwd: Option<&str>) -> String {
    let base = strip_worktree_suffix(entry);

    // Strategy 1: Use the cwd (real path) when available — most robust
    if let Some(cwd) = cwd {
        if let Some(name) = project_name_from_cwd_path(cwd) {
            return name;
        }
    }

    // Strategy 2: Pattern-match the encoded directory name
    // Look for known parent directory patterns in the encoded name
    for parent in &["-projects-", "-repos-", "-src-", "-code-", "-work-", "-dev-", "-git-"] {
        if let Some(pos) = base.rfind(parent) {
            let after_pos = pos + parent.len();
            if after_pos < base.len() {
                return base[after_pos..].to_string();
            }
        }
    }

    // Strategy 3: Unix-style entries starting with "-" (from leading /)
    if let Some(stripped) = base.strip_prefix('-') {
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }

    // Strategy 4: Fallback — last segment split on "-"
    base.rsplit('-')
        .next()
        .unwrap_or(base)
        .to_string()
}

/// Extract the project name from a real filesystem path using path parsing.
///
/// Walks path segments looking for known project parent directories
/// (`projects`, `repos`, `src`, `code`, `work`, `dev`, `git`),
/// then returns the next segment as the project name.
///
/// Falls back to the last segment of the path.
fn project_name_from_cwd_path(cwd: &str) -> Option<String> {
    let normalized = cwd.replace('\\', "/");

    // Strip .claude/worktrees/<name> suffix — worktrees are subdirectories of the project
    let clean = if let Some(pos) = normalized.find("/.claude/worktrees/") {
        &normalized[..pos]
    } else {
        &normalized
    };

    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();

    // Walk segments, find the last known parent directory, return the next segment
    let parents = ["projects", "repos", "src", "code", "work", "dev", "git"];
    let mut best: Option<&str> = None;
    for (i, seg) in segments.iter().enumerate() {
        let lower = seg.to_ascii_lowercase();
        if parents.contains(&lower.as_str()) {
            if let Some(next) = segments.get(i + 1) {
                best = Some(next);
            }
        }
    }

    // Return the deepest match, or fall back to last segment
    best.or(segments.last().copied())
        .map(|s| s.to_string())
}

/// Resolve a session's project identity by matching its cwd against known
/// `.claude/projects/` directory names.
///
/// Encodes the cwd segment-by-segment and checks at each step whether the
/// accumulated encoding matches a known watch directory entry. This correctly
/// handles hyphenated project names (e.g., `open-story`) without ambiguity.
///
/// Returns `(project_id, project_name)` where project_id is the canonical
/// directory name and project_name is the human-readable last segment.
pub fn resolve_project(cwd: &str, watch_dir_entries: &[String]) -> ResolvedProject {
    let normalized = cwd.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();

    let mut encoded = String::new();
    let mut best_match: Option<ResolvedProject> = None;

    for (i, seg) in segments.iter().enumerate() {
        if i == 0 && seg.ends_with(':') {
            // Windows drive letter
            encoded.push_str(&seg[..seg.len() - 1]);
            encoded.push('-');
        } else {
            if !encoded.is_empty() || normalized.starts_with('/') {
                encoded.push('-');
            }
            encoded.push_str(seg);
        }

        // Track the longest matching watch directory entry
        // Also check entries after stripping worktree suffixes
        let has_match = watch_dir_entries.iter().any(|e| {
            *e == encoded || strip_worktree_suffix(e) == encoded
        });
        if has_match {
            best_match = Some(ResolvedProject {
                project_id: encoded.clone(),
                project_name: display_name_from_entry_and_cwd(&encoded, Some(cwd)),
            });
        }
    }

    // Return the longest (most specific) match, or fall back
    best_match.unwrap_or_else(|| {
        let name = display_name_from_entry_and_cwd(&encoded, Some(cwd));
        ResolvedProject {
            project_id: encoded,
            project_name: name,
        }
    })
}

/// Scan events for the first cwd value.
pub fn extract_cwd_from_events(events: &[Value]) -> Option<String> {
    events.iter().find_map(extract_cwd)
}

/// Staleness threshold: 5 minutes in seconds.
const STALE_THRESHOLD_SECS: i64 = 300;

/// Compute a summary from a list of CloudEvent dicts (serialized Value).
/// `now` is used for staleness detection; pass None to skip staleness checks.
pub fn session_summary(session_id_hint: &str, events: &[Value], now: Option<DateTime<Utc>>) -> SessionSummary {
    let mut session_id = String::new();
    let mut start_time: Option<String> = None;
    let mut duration_ms: Option<f64> = None;
    let mut exit_code: Option<i64> = None;
    let mut model: Option<String> = None;
    let mut error_count: usize = 0;
    let mut tool_calls: usize = 0;
    let mut files_edited: usize = 0;
    let mut prompt_count: usize = 0;
    let mut response_count: usize = 0;
    let mut first_prompt: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut tools_seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut status = "ongoing".to_string();

    for e in events {
        let etype = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let data = e.get("data").cloned().unwrap_or(Value::Object(Default::default()));

        match classify_event(etype, e) {
            EventKind::SessionStart => {
                session_id = e
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                start_time = e.get("time").and_then(|v| v.as_str()).map(|s| s.to_string());
                if let Some(m) = extract_model(e) {
                    model = Some(m);
                }
                if cwd.is_none() {
                    cwd = extract_cwd(e);
                }
            }
            EventKind::SessionEnd => {
                exit_code = data.get("exit_code").and_then(|v| v.as_i64());
                duration_ms = data
                    .get("duration_ms")
                    .and_then(|v| v.as_f64())
                    .or_else(|| data.get("durationMs").and_then(|v| v.as_f64()));
                let reason = data.get("reason");
                if reason.is_some() && !reason.unwrap().is_null() {
                    status = "completed".to_string();
                } else if let Some(code) = exit_code {
                    status = if code == 0 { "completed" } else { "error" }.to_string();
                } else {
                    status = "completed".to_string();
                }
            }
            EventKind::Error => {
                error_count += 1;
            }
            EventKind::ToolCall => {
                let names = extract_tool_names(e);
                if names.is_empty() {
                    tool_calls += 1;
                    let tool_name = data
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    *tools_seen.entry(tool_name).or_insert(0) += 1;
                } else {
                    tool_calls += names.len();
                    for name in names {
                        *tools_seen.entry(name).or_insert(0) += 1;
                    }
                }
                // Pick up model from assistant tool_use events too
                if model.is_none() {
                    if let Some(m) = extract_model(e) {
                        model = Some(m);
                    }
                }
            }
            EventKind::FileEdit => {
                files_edited += 1;
            }
            EventKind::PromptSubmit => {
                prompt_count += 1;
                if first_prompt.is_none() {
                    if let Some(text) = extract_prompt_text(e) {
                        first_prompt = Some(text.chars().take(100).collect());
                    }
                }
                // Pick up start_time from first user event if no session.start
                if start_time.is_none() {
                    start_time = e.get("time").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                if cwd.is_none() {
                    cwd = extract_cwd(e);
                }
            }
            EventKind::ResponseComplete => {
                response_count += 1;
                // Pick up model from first assistant response
                if model.is_none() {
                    if let Some(m) = extract_model(e) {
                        model = Some(m);
                    }
                }
            }
            EventKind::Other => {
                // Extract duration from turn.complete / turn_duration (they don't end the session)
                let subtype = e.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
                if subtype == "system.turn.complete" || subtype == "turn_duration" {
                    if let Some(d) = data.get("duration_ms").and_then(|v| v.as_f64())
                        .or_else(|| data.get("durationMs").and_then(|v| v.as_f64()))
                    {
                        duration_ms = Some(d);
                    }
                }
            }
        }
    }

    // --- Status heuristics (only if no explicit session.end set status) ---
    if status == "ongoing" {
        // Heuristic 1: last event is system.turn.complete → session completed
        if let Some(last) = events.last() {
            let last_subtype = last.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if last_subtype == "system.turn.complete" || last_subtype == "turn_duration" {
                status = "completed".to_string();
            }
        }

        // Heuristic 2: staleness — last event >5 minutes ago
        if status == "ongoing" {
            if let Some(now) = now {
                // Find the last event time
                let last_event_time = events.iter().rev().find_map(|e| {
                    e.get("time")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                });
                if let Some(last_time) = last_event_time {
                    let elapsed = now.signed_duration_since(last_time).num_seconds();
                    if elapsed > STALE_THRESHOLD_SECS {
                        status = "stale".to_string();
                    }
                }
            }
        }
    }

    if session_id.is_empty() {
        session_id = session_id_hint.to_string();
    }

    SessionSummary {
        session_id,
        status,
        start_time,
        duration_ms,
        event_count: events.len(),
        error_count,
        tool_calls,
        files_edited,
        unique_tools: tools_seen.keys().cloned().collect(),
        exit_code,
        model,
        prompt_count,
        response_count,
        first_prompt,
        cwd,
        project_id: None,
    }
}

/// Tool name → call count.
pub fn tool_call_distribution(events: &[Value]) -> BTreeMap<String, usize> {
    let mut dist = BTreeMap::new();
    for e in events {
        let etype = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if let EventKind::ToolCall = classify_event(etype, e) {
            let names = extract_tool_names(e);
            if names.is_empty() {
                let tool = e
                    .get("data")
                    .and_then(|d| d.get("tool"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                *dist.entry(tool).or_insert(0) += 1;
            } else {
                for name in names {
                    *dist.entry(name).or_insert(0) += 1;
                }
            }
        }
    }
    dist
}

#[derive(Debug, Clone, Serialize)]
pub struct FileTouched {
    pub path: String,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivitySummary {
    pub first_prompt: Option<String>,
    pub files_touched: Vec<FileTouched>,
    pub tool_breakdown: BTreeMap<String, usize>,
    pub error_messages: Vec<String>,
    pub last_response: Option<String>,
    pub conversation_turns: usize,
    pub plan_count: usize,
    pub duration_ms: Option<f64>,
    pub start_time: Option<String>,
}

/// Single-pass O(n) extraction of rich session activity data.
pub fn activity_summary(events: &[Value]) -> ActivitySummary {
    let mut first_prompt: Option<String> = None;
    let mut files_touched = Vec::new();
    let mut tools: BTreeMap<String, usize> = BTreeMap::new();
    let mut error_messages = Vec::new();
    let mut last_response: Option<String> = None;
    let mut prompt_count: usize = 0;
    let mut response_count: usize = 0;
    let mut plan_count: usize = 0;
    let mut duration_ms: Option<f64> = None;
    let mut start_time: Option<String> = None;

    for e in events {
        let etype = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let data = e.get("data").cloned().unwrap_or(Value::Object(Default::default()));

        match classify_event(etype, e) {
            EventKind::SessionStart => {
                start_time = e.get("time").and_then(|v| v.as_str()).map(|s| s.to_string());
            }
            EventKind::SessionEnd => {
                duration_ms = data
                    .get("duration_ms")
                    .and_then(|v| v.as_f64())
                    .or_else(|| data.get("durationMs").and_then(|v| v.as_f64()));
            }
            EventKind::PromptSubmit => {
                prompt_count += 1;
                if start_time.is_none() {
                    start_time = e.get("time").and_then(|v| v.as_str()).map(|s| s.to_string());
                }
                if first_prompt.is_none() {
                    if let Some(text) = extract_prompt_text(e) {
                        first_prompt = Some(text);
                    }
                }
            }
            EventKind::ResponseComplete => {
                response_count += 1;
                // Extract last response text
                let msg = data
                    .get("last_assistant_message")
                    .or_else(|| data.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !msg.is_empty() {
                    last_response = Some(msg.to_string());
                }
            }
            EventKind::ToolCall => {
                let names = extract_tool_names(e);
                if names.is_empty() {
                    let tool_name = data
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    *tools.entry(tool_name.clone()).or_insert(0) += 1;
                    if tool_name == "ExitPlanMode" {
                        plan_count += 1;
                    }
                } else {
                    for name in &names {
                        *tools.entry(name.clone()).or_insert(0) += 1;
                        if name == "ExitPlanMode" {
                            plan_count += 1;
                        }
                    }
                }

                // Check for file edit tools in transcript format
                let file_tool_names = if names.is_empty() {
                    vec![data.get("tool").and_then(|v| v.as_str()).unwrap_or("").to_string()]
                } else {
                    names
                };
                for tool_name in &file_tool_names {
                    if matches!(tool_name.as_str(), "Edit" | "Write" | "NotebookEdit") {
                        // Extract file path from tool_use input in transcript
                        if let Some(raw) = data.get("raw") {
                            if let Some(Value::Array(blocks)) = raw.get("message").and_then(|m| m.get("content")) {
                                    for block in blocks {
                                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use")
                                            && block.get("name").and_then(|v| v.as_str()) == Some(tool_name.as_str())
                                        {
                                            let input = block.get("input").unwrap_or(&Value::Null);
                                            let path = input
                                                .get("file_path")
                                                .or_else(|| input.get("path"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let operation = if tool_name == "Edit" { "modify" } else { "create" };
                                            files_touched.push(FileTouched {
                                                path,
                                                operation: operation.to_string(),
                                            });
                                        }
                                    }
                            }
                        }
                    }
                }
            }
            EventKind::FileEdit => {
                let path = data.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let op = data
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                files_touched.push(FileTouched {
                    path,
                    operation: op,
                });
            }
            EventKind::Error => {
                let msg = data.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !msg.is_empty() {
                    error_messages.push(msg);
                }
            }
            EventKind::Other => {}
        }
    }

    // Truncate last_response to 200 chars
    if let Some(ref mut resp) = last_response {
        if resp.len() > 200 {
            *resp = resp.chars().take(200).collect();
        }
    }

    ActivitySummary {
        first_prompt,
        files_touched,
        tool_breakdown: tools,
        error_messages,
        last_response,
        conversation_turns: prompt_count.min(response_count),
        plan_count,
        duration_ms,
        start_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Hook-format tests (backward compat) ---

    fn make_hook_events() -> Vec<Value> {
        vec![
            json!({
                "type": "io.arc.session.start",
                "source": "arc://hooks/test-sess",
                "time": "2026-01-01T00:00:00Z",
                "data": {"model": "claude-sonnet-4-20250514", "meta": {"cwd": "/home"}}
            }),
            json!({
                "type": "io.arc.prompt.submit",
                "data": {"text": "Hello world"}
            }),
            json!({
                "type": "io.arc.tool.call",
                "data": {"tool": "Read"}
            }),
            json!({
                "type": "io.arc.tool.call",
                "data": {"tool": "Read"}
            }),
            json!({
                "type": "io.arc.tool.call",
                "data": {"tool": "Edit"}
            }),
            json!({
                "type": "io.arc.file.edit",
                "data": {"path": "/tmp/test.rs", "operation": "modify"}
            }),
            json!({
                "type": "io.arc.response.complete",
                "data": {"text": "Done!"}
            }),
            json!({
                "type": "io.arc.session.end",
                "data": {"reason": "user_exit", "duration_ms": 5000.0}
            }),
        ]
    }

    #[test]
    fn test_session_summary_hooks() {
        let events = make_hook_events();
        let s = session_summary("", &events, None);
        assert_eq!(s.session_id, "test-sess");
        assert_eq!(s.status, "completed");
        assert_eq!(s.event_count, 8);
        assert_eq!(s.tool_calls, 3);
        assert_eq!(s.files_edited, 1);
        assert_eq!(s.prompt_count, 1);
        assert_eq!(s.response_count, 1);
        assert_eq!(s.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(s.first_prompt.as_deref(), Some("Hello world"));
        assert_eq!(s.cwd.as_deref(), Some("/home"));
        assert_eq!(s.duration_ms, Some(5000.0));
    }

    #[test]
    fn test_tool_call_distribution_hooks() {
        let events = make_hook_events();
        let dist = tool_call_distribution(&events);
        assert_eq!(dist.get("Read"), Some(&2));
        assert_eq!(dist.get("Edit"), Some(&1));
    }

    #[test]
    fn test_activity_summary_hooks() {
        let events = make_hook_events();
        let a = activity_summary(&events);
        assert_eq!(a.first_prompt.as_deref(), Some("Hello world"));
        assert_eq!(a.files_touched.len(), 1);
        assert_eq!(a.conversation_turns, 1);
        assert_eq!(a.last_response.as_deref(), Some("Done!"));
    }

    #[test]
    fn test_empty_events() {
        let s = session_summary("fallback", &[], None);
        assert_eq!(s.session_id, "fallback");
        assert_eq!(s.status, "ongoing");
        assert_eq!(s.event_count, 0);
    }

    // --- Unified io.arc.event tests ---

    fn make_arc_events() -> Vec<Value> {
        vec![
            json!({
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:00Z",
                "data": {
                    "text": "Build a URL shortener",
                    "session_id": "test-sess",
                    "cwd": "/projects/foo",
                    "raw": {
                        "type": "user",
                        "cwd": "/projects/foo",
                        "message": {
                            "content": [{"type": "text", "text": "Build a URL shortener"}]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "message.assistant.tool_use",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:01Z",
                "data": {
                    "model": "claude-sonnet-4-20250514",
                    "tool": "Read",
                    "args": {"file_path": "/tmp/test.rs"},
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "model": "claude-sonnet-4-20250514",
                            "content": [
                                {"type": "text", "text": "Let me read that file."},
                                {"type": "tool_use", "name": "Read", "input": {"file_path": "/tmp/test.rs"}}
                            ]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "message.assistant.tool_use",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:02Z",
                "data": {
                    "tool": "Edit",
                    "args": {"file_path": "/tmp/test.rs", "old_string": "a", "new_string": "b"},
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "content": [
                                {"type": "tool_use", "name": "Edit", "input": {"file_path": "/tmp/test.rs", "old_string": "a", "new_string": "b"}}
                            ]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "message.assistant.text",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:03Z",
                "data": {
                    "text": "All done!",
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "content": [{"type": "text", "text": "All done!"}]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "system.turn.complete",
                "time": "2026-01-01T00:00:04Z",
                "data": {"durationMs": 4000.0, "duration_ms": 4000.0}
            }),
        ]
    }

    #[test]
    fn test_summary_prompt_from_new_type() {
        let events = make_arc_events();
        let s = session_summary("test-sess", &events, None);
        assert_eq!(s.first_prompt.as_deref(), Some("Build a URL shortener"));
        assert_eq!(s.prompt_count, 1);
    }

    #[test]
    fn test_summary_status_completed_when_last_event_is_turn_complete() {
        let events = make_arc_events();
        let s = session_summary("test-sess", &events, None);
        // turn.complete as last event → session is completed (heuristic)
        assert_eq!(s.status, "completed");
        // duration is still captured
        assert_eq!(s.duration_ms, Some(4000.0));
    }

    #[test]
    fn test_session_summary_arc_event() {
        let events = make_arc_events();
        let s = session_summary("test-sess", &events, None);
        assert_eq!(s.session_id, "test-sess");
        assert_eq!(s.event_count, 5);
        assert_eq!(s.prompt_count, 1);
        assert_eq!(s.response_count, 1); // only text subtype counts as response
        assert_eq!(s.tool_calls, 2); // two tool_use assistant events
        assert_eq!(s.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(s.cwd.as_deref(), Some("/projects/foo"));
    }

    #[test]
    fn test_tool_call_distribution_arc_event() {
        let events = make_arc_events();
        let dist = tool_call_distribution(&events);
        assert_eq!(dist.get("Read"), Some(&1));
        assert_eq!(dist.get("Edit"), Some(&1));
    }

    #[test]
    fn test_activity_summary_arc_event() {
        let events = make_arc_events();
        let a = activity_summary(&events);
        assert_eq!(a.first_prompt.as_deref(), Some("Build a URL shortener"));
        assert_eq!(a.files_touched.len(), 1); // Edit tool detected
        assert_eq!(a.files_touched[0].path, "/tmp/test.rs");
        assert!(a.tool_breakdown.contains_key("Read"));
        assert!(a.tool_breakdown.contains_key("Edit"));
    }

    // --- Transcript-format tests (legacy backward compat) ---

    fn make_transcript_events() -> Vec<Value> {
        vec![
            json!({
                "type": "io.arc.transcript.user",
                "subtype": "text",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:00Z",
                "data": {
                    "raw": {
                        "type": "user",
                        "cwd": "/projects/foo",
                        "message": {
                            "content": [{"type": "text", "text": "Build a URL shortener"}]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.transcript.assistant",
                "subtype": "tool_use",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:01Z",
                "data": {
                    "model": "claude-sonnet-4-20250514",
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "model": "claude-sonnet-4-20250514",
                            "content": [
                                {"type": "text", "text": "Let me read that file."},
                                {"type": "tool_use", "name": "Read", "input": {"file_path": "/tmp/test.rs"}}
                            ]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.transcript.assistant",
                "subtype": "tool_use",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:02Z",
                "data": {
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "content": [
                                {"type": "tool_use", "name": "Edit", "input": {"file_path": "/tmp/test.rs", "old_string": "a", "new_string": "b"}}
                            ]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.transcript.assistant",
                "subtype": "text",
                "source": "arc://transcript/test-sess",
                "time": "2026-01-01T00:00:03Z",
                "data": {
                    "raw": {
                        "type": "assistant",
                        "message": {
                            "content": [{"type": "text", "text": "All done!"}]
                        }
                    }
                }
            }),
            json!({
                "type": "io.arc.transcript.system",
                "subtype": "turn_duration",
                "time": "2026-01-01T00:00:04Z",
                "data": {"durationMs": 4000.0}
            }),
        ]
    }

    #[test]
    fn test_session_summary_transcript() {
        let events = make_transcript_events();
        let s = session_summary("test-sess", &events, None);
        assert_eq!(s.session_id, "test-sess");
        assert_eq!(s.status, "completed"); // turn_duration as last event → completed (heuristic)
        assert_eq!(s.event_count, 5);
        assert_eq!(s.prompt_count, 1);
        assert_eq!(s.response_count, 1); // only text subtype counts as response
        assert_eq!(s.tool_calls, 2); // two tool_use assistant events
        assert_eq!(s.first_prompt.as_deref(), Some("Build a URL shortener"));
        assert_eq!(s.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(s.duration_ms, Some(4000.0)); // duration still captured
        assert_eq!(s.cwd.as_deref(), Some("/projects/foo"));
    }

    #[test]
    fn test_tool_call_distribution_transcript() {
        let events = make_transcript_events();
        let dist = tool_call_distribution(&events);
        assert_eq!(dist.get("Read"), Some(&1));
        assert_eq!(dist.get("Edit"), Some(&1));
    }

    #[test]
    fn test_activity_summary_transcript() {
        let events = make_transcript_events();
        let a = activity_summary(&events);
        assert_eq!(a.first_prompt.as_deref(), Some("Build a URL shortener"));
        assert_eq!(a.files_touched.len(), 1); // Edit tool detected
        assert_eq!(a.files_touched[0].path, "/tmp/test.rs");
        assert_eq!(a.files_touched[0].operation, "modify");
        assert!(a.tool_breakdown.contains_key("Read"));
        assert!(a.tool_breakdown.contains_key("Edit"));
    }

    // ── classify_event: parameterized boundary table ────────────────
    //
    // Every row: (type, subtype, expected_kind_name)
    // This table IS the spec for event classification.

    fn kind_name(k: &EventKind) -> &'static str {
        match k {
            EventKind::SessionStart => "SessionStart",
            EventKind::SessionEnd => "SessionEnd",
            EventKind::Error => "Error",
            EventKind::ToolCall => "ToolCall",
            EventKind::FileEdit => "FileEdit",
            EventKind::PromptSubmit => "PromptSubmit",
            EventKind::ResponseComplete => "ResponseComplete",
            EventKind::Other => "Other",
        }
    }

    #[test]
    fn test_classify_event_boundary_table() {
        let cases: Vec<(&str, Option<&str>, &str)> = vec![
            // ── Unified io.arc.event subtypes ──
            ("io.arc.event", Some("message.user.prompt"),        "PromptSubmit"),
            ("io.arc.event", Some("message.user.tool_result"),   "Other"),
            ("io.arc.event", Some("message.assistant.text"),      "ResponseComplete"),
            ("io.arc.event", Some("message.assistant.tool_use"), "ToolCall"),
            ("io.arc.event", Some("message.assistant.thinking"), "ResponseComplete"),
            ("io.arc.event", Some("system.error"),                "Error"),
            ("io.arc.event", Some("system.turn.complete"),        "Other"),
            ("io.arc.event", Some("system.session.start"),        "SessionStart"),
            ("io.arc.event", Some("system.session.end"),          "SessionEnd"),
            ("io.arc.event", Some("system.compact"),              "Other"),
            ("io.arc.event", Some("system.hook"),                 "Other"),
            ("io.arc.event", Some("file.edit"),                   "FileEdit"),
            ("io.arc.event", Some("file.snapshot"),               "Other"),
            ("io.arc.event", Some("progress.bash"),               "Other"),
            ("io.arc.event", Some("queue.enqueue"),               "Other"),
            ("io.arc.event", None,                                "Other"),

            // ── Legacy hook types ──
            ("io.arc.session.start",     None, "SessionStart"),
            ("io.arc.session.end",       None, "SessionEnd"),
            ("io.arc.error",             None, "Error"),
            ("io.arc.tool.call",         None, "ToolCall"),
            ("io.arc.file.edit",         None, "FileEdit"),
            ("io.arc.prompt.submit",     None, "PromptSubmit"),
            ("io.arc.response.complete", None, "ResponseComplete"),

            // ── Legacy transcript types ──
            ("io.arc.transcript.user",      Some("text"),         "PromptSubmit"),
            ("io.arc.transcript.user",      Some("tool_result"),  "Other"),
            ("io.arc.transcript.user",      None,                 "PromptSubmit"),
            ("io.arc.transcript.assistant", Some("text"),         "ResponseComplete"),
            ("io.arc.transcript.assistant", Some("tool_use"),     "ToolCall"),
            ("io.arc.transcript.assistant", Some("thinking"),     "ResponseComplete"),
            ("io.arc.transcript.system",    Some("turn_duration"), "Other"),
            ("io.arc.transcript.system",    Some("api_error"),    "Error"),
            ("io.arc.transcript.system",    Some("compact"),      "Other"),
            ("io.arc.transcript.system",    None,                 "Other"),
            ("io.arc.transcript.progress",  None,                 "Other"),
            ("io.arc.transcript.snapshot",  None,                 "Other"),
            ("io.arc.transcript.queue",     None,                 "Other"),

            // ── Unknown types ──
            ("io.arc.unknown.thing",  None, "Other"),
            ("completely.different",  None, "Other"),
        ];

        for (etype, subtype, expected) in &cases {
            let event = match subtype {
                Some(st) => json!({"type": etype, "subtype": st}),
                None => json!({"type": etype}),
            };
            let result = classify_event(etype, &event);
            assert_eq!(
                kind_name(&result), *expected,
                "classify_event({etype}, {subtype:?}) should be {expected}"
            );
        }
    }

    // ── extract helpers: boundary tables ─────────────────────────────

    #[test]
    fn test_extract_model_boundary_table() {
        let cases: Vec<(Value, Option<&str>)> = vec![
            // Hook format: data.model
            (json!({"data": {"model": "claude-opus-4"}}), Some("claude-opus-4")),
            // Transcript format: data.raw.message.model
            (json!({"data": {"raw": {"message": {"model": "claude-sonnet-4-6"}}}}), Some("claude-sonnet-4-6")),
            // No model
            (json!({"data": {}}), None),
            // No data
            (json!({}), None),
        ];
        for (event, expected) in &cases {
            assert_eq!(
                extract_model(event).as_deref(), *expected,
                "extract_model({event})"
            );
        }
    }

    #[test]
    fn test_extract_prompt_text_boundary_table() {
        let cases: Vec<(Value, Option<&str>)> = vec![
            // Hook format: data.text
            (json!({"data": {"text": "Hello"}}), Some("Hello")),
            // Transcript: string content
            (json!({"data": {"raw": {"message": {"content": "Direct text"}}}}), Some("Direct text")),
            // Transcript: array content with text block
            (json!({"data": {"raw": {"message": {"content": [{"type": "text", "text": "From block"}]}}}}), Some("From block")),
            // Transcript: array content without text block
            (json!({"data": {"raw": {"message": {"content": [{"type": "tool_use", "name": "Read"}]}}}}), None),
            // Empty text
            (json!({"data": {"text": ""}}), None),
            // No data
            (json!({}), None),
        ];
        for (event, expected) in &cases {
            assert_eq!(
                extract_prompt_text(event).as_deref(), *expected,
                "extract_prompt_text({event})"
            );
        }
    }

    #[test]
    fn test_extract_tool_names_boundary_table() {
        let cases: Vec<(Value, Vec<&str>)> = vec![
            // Hook format: data.tool
            (json!({"data": {"tool": "Read"}}), vec!["Read"]),
            // Transcript: content blocks with tool_use
            (json!({"data": {"raw": {"message": {"content": [
                {"type": "tool_use", "name": "Read"},
                {"type": "tool_use", "name": "Edit"},
            ]}}}}), vec!["Read", "Edit"]),
            // No tools
            (json!({"data": {}}), vec![]),
            // No data
            (json!({}), vec![]),
        ];
        for (event, expected) in &cases {
            let result = extract_tool_names(event);
            let expected_strings: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
            assert_eq!(result, expected_strings, "extract_tool_names({event})");
        }
    }

    #[test]
    fn test_extract_cwd_boundary_table() {
        let cases: Vec<(Value, Option<&str>)> = vec![
            // Hook format: data.meta.cwd
            (json!({"data": {"meta": {"cwd": "/home/user"}}}), Some("/home/user")),
            // Hook format: data.cwd
            (json!({"data": {"cwd": "/projects/foo"}}), Some("/projects/foo")),
            // Transcript format: data.raw.cwd
            (json!({"data": {"raw": {"cwd": "/other"}}}), Some("/other")),
            // No cwd
            (json!({"data": {}}), None),
            // No data
            (json!({}), None),
        ];
        for (event, expected) in &cases {
            assert_eq!(
                extract_cwd(event).as_deref(), *expected,
                "extract_cwd({event})"
            );
        }
    }

    // ── project_id derivation tests ──────────────────────────────────

    #[test]
    fn test_project_id_from_cwd_unix_path() {
        assert_eq!(
            project_id_from_cwd("/home/user/projects/open-story").as_deref(),
            Some("open-story")
        );
    }

    #[test]
    fn test_project_id_from_cwd_windows_path() {
        assert_eq!(
            project_id_from_cwd("C:\\Users\\dev\\projects\\my-app").as_deref(),
            Some("my-app")
        );
    }

    #[test]
    fn test_project_id_from_cwd_trailing_slash() {
        assert_eq!(
            project_id_from_cwd("/home/user/projects/open-story/").as_deref(),
            Some("open-story")
        );
    }

    #[test]
    fn test_project_id_from_cwd_empty() {
        assert_eq!(project_id_from_cwd(""), None);
    }

    #[test]
    fn test_extract_cwd_from_events_finds_first() {
        let events = vec![
            json!({"data": {}}),
            json!({"data": {"cwd": "/projects/foo"}}),
            json!({"data": {"cwd": "/projects/bar"}}),
        ];
        assert_eq!(extract_cwd_from_events(&events).as_deref(), Some("/projects/foo"));
    }

    #[test]
    fn test_extract_cwd_from_events_none() {
        let events = vec![json!({"data": {}}), json!({"data": {}})];
        assert_eq!(extract_cwd_from_events(&events), None);
    }

    // ── session status heuristic tests ───────────────────────────────

    // ── encode_path_as_dir_name tests ──────────────────────────────

    #[test]
    fn test_encode_path_windows_project_root() {
        assert_eq!(
            encode_path_as_dir_name(r"C:\Users\dev\projects\my-project"),
            "C--Users-dev-projects-my-project"
        );
    }

    #[test]
    fn test_encode_path_windows_subdirectory() {
        assert_eq!(
            encode_path_as_dir_name(r"C:\Users\dev\projects\my-project\ui"),
            "C--Users-dev-projects-my-project-ui"
        );
    }

    #[test]
    fn test_encode_path_unix() {
        assert_eq!(
            encode_path_as_dir_name("/home/user/projects/my-app"),
            "-home-user-projects-my-app"
        );
    }

    #[test]
    fn test_encode_path_workspace() {
        assert_eq!(encode_path_as_dir_name("/workspace"), "-workspace");
    }

    #[test]
    fn test_encode_path_unix_forward_slashes() {
        assert_eq!(
            encode_path_as_dir_name("C:/Users/dev/projects/webapp"),
            "C--Users-dev-projects-webapp"
        );
    }

    // ── resolve_project boundary table ──────────────────────────────

    fn watch_entries() -> Vec<String> {
        vec![
            "C--Users-dev-projects".to_string(), // parent dir also exists
            "C--Users-dev-projects-my-project".to_string(),
            "C--Users-dev-projects-my-project--claude-worktrees-purrfect-noodling-badger".to_string(),
            "C--Users-dev-projects-webapp".to_string(),
            "C--Users-dev-projects-side-project".to_string(),
            "-workspace".to_string(),
            "-workspace--claude-worktrees-abundant-splashing-fiddle".to_string(),
        ]
    }

    /// Boundary table: cwd → expected (project_id, project_name)
    #[test]
    fn test_resolve_project_boundary_table() {
        let entries = watch_entries();
        let cases: Vec<(&str, &str, &str)> = vec![
            // (cwd, expected_project_id, expected_project_name)
            //
            // Exact match: project root
            (r"C:\Users\dev\projects\my-project", "C--Users-dev-projects-my-project", "my-project"),
            // Subdirectory: should resolve to parent project
            (r"C:\Users\dev\projects\my-project\ui", "C--Users-dev-projects-my-project", "my-project"),
            (r"C:\Users\dev\projects\my-project\e2e", "C--Users-dev-projects-my-project", "my-project"),
            (r"C:\Users\dev\projects\my-project\rs", "C--Users-dev-projects-my-project", "my-project"),
            // Different project
            (r"C:\Users\dev\projects\webapp", "C--Users-dev-projects-webapp", "webapp"),
            (r"C:\Users\dev\projects\webapp\ui", "C--Users-dev-projects-webapp", "webapp"),
            (r"C:\Users\dev\projects\webapp\components", "C--Users-dev-projects-webapp", "webapp"),
            // Hyphenated project name
            (r"C:\Users\dev\projects\side-project", "C--Users-dev-projects-side-project", "side-project"),
            // Parent dir exists but child is more specific — prefer child
            (r"C:\Users\dev\projects", "C--Users-dev-projects", "projects"),
            // Sandbox: matches the -workspace entry
            ("/workspace", "-workspace", "workspace"),
            // Worktree cwd: should resolve to parent project via worktree entry match
            (r"C:\Users\dev\projects\my-project\.claude\worktrees\purrfect-noodling-badger",
             "C--Users-dev-projects-my-project", "my-project"),
            // Sandbox worktree: cwd is /workspace/.claude/worktrees/<name>
            ("/workspace/.claude/worktrees/abundant-splashing-fiddle",
             "-workspace", "workspace"),
            // Unix paths
            ("/home/user/projects/my-app", "-home-user-projects-my-app", "my-app"),
        ];

        for (cwd, expected_id, expected_name) in cases {
            let result = resolve_project(cwd, &entries);
            assert_eq!(
                result.project_id, expected_id,
                "project_id mismatch for cwd={cwd}"
            );
            assert_eq!(
                result.project_name, expected_name,
                "project_name mismatch for cwd={cwd}"
            );
        }
    }

    // ── strip_worktree_suffix tests ────────────────────────────────

    #[test]
    fn test_strip_worktree_suffix() {
        assert_eq!(
            strip_worktree_suffix("C--Users-dev-projects-my-project--claude-worktrees-purrfect-noodling-badger"),
            "C--Users-dev-projects-my-project"
        );
        assert_eq!(
            strip_worktree_suffix("-workspace--claude-worktrees-abundant-splashing-fiddle"),
            "-workspace"
        );
        // No suffix — returns unchanged
        assert_eq!(
            strip_worktree_suffix("C--Users-dev-projects-my-project"),
            "C--Users-dev-projects-my-project"
        );
    }

    // ── display_name_from_entry tests ───────────────────────────────

    #[test]
    fn test_display_name_boundary_table() {
        let entries = watch_entries();
        let cases: Vec<(&str, &str)> = vec![
            // (entry, expected_display_name)
            ("C--Users-dev-projects-my-project", "my-project"),
            ("C--Users-dev-projects-webapp", "webapp"),
            ("C--Users-dev-projects-side-project", "side-project"),
            ("C--Users-dev-projects", "projects"),
            ("-workspace", "workspace"),
            // Worktree entries: should strip suffix then decode
            ("C--Users-dev-projects-my-project--claude-worktrees-purrfect-noodling-badger", "my-project"),
            ("-workspace--claude-worktrees-abundant-splashing-fiddle", "workspace"),
            // Unix-style project
            ("-home-user-projects-my-app", "my-app"),
        ];
        for (entry, expected) in cases {
            assert_eq!(
                display_name_from_entry(entry, &entries),
                expected,
                "display_name mismatch for entry={entry}"
            );
        }
    }

    // ── resolve_project edge cases ──────────────────────────────────

    #[test]
    fn test_resolve_project_no_specific_match_uses_parent() {
        let entries = watch_entries();
        // "unknown-project" has no specific entry, but parent "C--Users-dev-projects" exists
        // project_id matches the parent, but display name is derived from cwd path parsing
        let result = resolve_project(r"C:\Users\dev\projects\unknown-project", &entries);
        assert_eq!(result.project_id, "C--Users-dev-projects");
        assert_eq!(result.project_name, "unknown-project");
    }

    #[test]
    fn test_resolve_project_no_matching_entry_falls_back() {
        // With entries that don't match at all
        let entries = vec!["some-other-project".to_string()];
        let result = resolve_project(r"C:\Users\dev\projects\unknown-project", &entries);
        assert_eq!(result.project_id, "C--Users-dev-projects-unknown-project");
        assert_eq!(result.project_name, "unknown-project");
    }

    #[test]
    fn test_resolve_project_empty_entries_falls_back() {
        let result = resolve_project(r"C:\Users\dev\projects\my-project\ui", &[]);
        // No entries to match — uses full encoded path as id,
        // but cwd-based path parsing finds "my-project" (segment after "projects")
        assert_eq!(result.project_id, "C--Users-dev-projects-my-project-ui");
        assert_eq!(result.project_name, "my-project");
    }

    // ── session status heuristic tests ───────────────────────────────

    #[test]
    fn test_status_ongoing_when_recent_events_no_session_end() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 2, 0).unwrap();
        let events = vec![
            json!({
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2026-01-01T00:00:00Z",
                "data": {"text": "hello"}
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "message.assistant.text",
                "time": "2026-01-01T00:01:00Z",
                "data": {"text": "hi"}
            }),
        ];
        let s = session_summary("test", &events, Some(now));
        assert_eq!(s.status, "ongoing");
    }

    #[test]
    fn test_status_stale_when_old_events_no_session_end() {
        use chrono::TimeZone;
        // 10 minutes after last event
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let events = vec![
            json!({
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2026-01-01T00:00:00Z",
                "data": {"text": "hello"}
            }),
        ];
        let s = session_summary("test", &events, Some(now));
        assert_eq!(s.status, "stale");
    }

    #[test]
    fn test_status_completed_when_session_end_present() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 1, 0, 0).unwrap();
        let events = vec![
            json!({
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2026-01-01T00:00:00Z",
                "data": {"text": "hello"}
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "system.session.end",
                "time": "2026-01-01T00:01:00Z",
                "data": {"reason": "user_exit"}
            }),
        ];
        let s = session_summary("test", &events, Some(now));
        assert_eq!(s.status, "completed");
    }

    #[test]
    fn test_status_completed_when_last_event_is_turn_complete_with_now() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 1, 0, 0).unwrap();
        let events = vec![
            json!({
                "type": "io.arc.event",
                "subtype": "message.user.prompt",
                "time": "2026-01-01T00:00:00Z",
                "data": {"text": "hello"}
            }),
            json!({
                "type": "io.arc.event",
                "subtype": "system.turn.complete",
                "time": "2026-01-01T00:01:00Z",
                "data": {"duration_ms": 5000.0}
            }),
        ];
        let s = session_summary("test", &events, Some(now));
        assert_eq!(s.status, "completed");
    }

    #[test]
    fn test_status_error_when_session_end_nonzero_exit() {
        let events = vec![
            json!({
                "type": "io.arc.event",
                "subtype": "system.session.end",
                "time": "2026-01-01T00:01:00Z",
                "data": {"exit_code": 1}
            }),
        ];
        let s = session_summary("test", &events, None);
        assert_eq!(s.status, "error");
    }
}
