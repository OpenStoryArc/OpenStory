//! SessionProjection — incremental cache with filter counts.
//!
//! Replaces ad-hoc recomputation with a stateful projection per session.
//! Maintains tree structure, filter counts, and full payloads incrementally.
//! All derived state is updated on append, never recomputed from scratch.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use open_story_core::cloud_event::CloudEvent;
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::{MessageContent, RecordBody};
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::TRUNCATION_THRESHOLD;

// ── Filter definitions ──────────────────────────────────────────────

/// All 21 filter names.
pub const FILTER_NAMES: &[&str] = &[
    // View
    "all", "patterns", "narrative", "user",
    // Activity
    "tools", "reading", "editing", "thinking", "deep",
    // Bash
    "bash.git", "bash.test", "bash.build", "bash.docker",
    // Results
    "compile_error", "test_pass", "test_fail", "file_create",
    // Patterns (populated when pattern detection exists)
    "tests", "errors", "agents", "git",
];

/// Check whether a ViewRecord matches a named filter.
pub fn filter_matches(name: &str, record: &ViewRecord) -> bool {
    match name {
        "all" => true,
        "user" => matches!(record.body, RecordBody::UserMessage(_)),
        "narrative" => matches!(
            record.body,
            RecordBody::UserMessage(_) | RecordBody::AssistantMessage(_)
        ),
        "tools" => matches!(
            record.body,
            RecordBody::ToolCall(_) | RecordBody::ToolResult(_)
        ),
        "thinking" => matches!(record.body, RecordBody::Reasoning(_)),
        "errors" => match &record.body {
            RecordBody::Error(_) => true,
            RecordBody::ToolResult(tr) => tr.is_error,
            _ => false,
        },
        "reading" => is_tool_named(record, &["Read", "Glob", "Grep"]),
        "editing" => is_tool_named(record, &["Edit", "Write"]),
        "file_create" => is_tool_named(record, &["Write"]),
        "deep" => is_tool_named(record, &["Agent"]),
        "bash.git" => bash_command_contains(record, &["git "]),
        "bash.test" => bash_command_contains(
            record,
            &["cargo test", "npm test", "npx vitest", "npx jest", "pytest"],
        ),
        "bash.build" => bash_command_contains(
            record,
            &["cargo build", "npm run build", "make ", "make\n"],
        ),
        "bash.docker" => bash_command_contains(record, &["docker "]),
        "compile_error" => result_output_contains(
            record,
            &["error[E", "error[", "TS2", "TS1", "SyntaxError"],
        ),
        "test_pass" => result_output_contains(
            record,
            &["test result: ok", "Tests  ", " passed"],
        ),
        "test_fail" => result_output_contains(
            record,
            &["FAILED", "failed", "Tests  "],
        ) && is_test_failure(record),
        // Pattern-based filters — always false until pattern detection exists
        "patterns" | "tests" | "agents" | "git" => false,
        _ => false,
    }
}

fn is_tool_named(record: &ViewRecord, names: &[&str]) -> bool {
    match &record.body {
        RecordBody::ToolCall(tc) => names.contains(&tc.name.as_str()),
        _ => false,
    }
}

fn bash_command_contains(record: &ViewRecord, patterns: &[&str]) -> bool {
    match &record.body {
        RecordBody::ToolCall(tc) if tc.name == "Bash" => {
            let cmd = tc
                .raw_input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            patterns.iter().any(|p| cmd.contains(p))
        }
        _ => false,
    }
}

fn result_output_contains(record: &ViewRecord, patterns: &[&str]) -> bool {
    match &record.body {
        RecordBody::ToolResult(tr) => {
            if let Some(output) = &tr.output {
                patterns.iter().any(|p| output.contains(p))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Distinguish test failure from test pass when output contains "Tests" or "failed".
fn is_test_failure(record: &ViewRecord) -> bool {
    match &record.body {
        RecordBody::ToolResult(tr) => {
            if let Some(output) = &tr.output {
                // Rust: "FAILED" in output
                // JS: "failed" with a count
                output.contains("FAILED")
                    || (output.contains("failed") && !output.contains("0 failed"))
            } else {
                false
            }
        }
        _ => false,
    }
}

// ── SessionProjection ───────────────────────────────────────────────

/// Per-session incremental projection. Updated on every event append.
#[derive(Clone)]
pub struct SessionProjection {
    session_id: String,
    event_count: usize,
    seen_ids: HashSet<String>,
    /// All ViewRecords produced from events in this session.
    records: Vec<ViewRecord>,
    /// Tree depth by event ID.
    depths: HashMap<String, u16>,
    /// Parent UUID by event ID.
    parents: HashMap<String, Option<String>>,
    /// Incrementally maintained filter counts.
    filter_counts: HashMap<String, usize>,
    /// Full payloads for truncated records (event_id → full content).
    full_payloads: HashMap<String, String>,
    /// Human-readable session label (first user prompt, truncated to 50 chars).
    label: Option<String>,
    /// Git branch name, captured from the first event that carries it.
    branch: Option<String>,
    /// Accumulated input token count across all turns.
    total_input_tokens: u64,
    /// Accumulated output token count across all turns.
    total_output_tokens: u64,
}

/// Result of appending a CloudEvent to the projection.
pub struct AppendResult {
    /// ViewRecords produced from this event.
    pub records: Vec<ViewRecord>,
    /// Filter count increments for this append.
    pub filter_deltas: HashMap<String, i32>,
    /// True when the session label was set for the first time by this append.
    pub label_changed: bool,
}

impl AppendResult {
    /// True when no new records were produced (e.g., duplicate event).
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    fn empty() -> Self {
        AppendResult {
            records: Vec::new(),
            filter_deltas: HashMap::new(),
            label_changed: false,
        }
    }
}

/// Cached metadata, returned in O(1) without iteration.
pub struct ProjectionMeta {
    pub event_count: usize,
    pub filter_counts: HashMap<String, usize>,
}

impl SessionProjection {
    pub fn new(session_id: &str) -> Self {
        SessionProjection {
            session_id: session_id.to_string(),
            event_count: 0,
            seen_ids: HashSet::new(),
            records: Vec::new(),
            depths: HashMap::new(),
            parents: HashMap::new(),
            filter_counts: HashMap::new(),
            full_payloads: HashMap::new(),
            label: None,
            branch: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
        }
    }

    /// Append a raw CloudEvent (as Value). Returns the new records and filter deltas.
    /// Deduplicates by CloudEvent ID — second append of same ID returns empty.
    pub fn append(&mut self, event: &Value) -> AppendResult {
        // 1. Extract event ID, check dedup
        let event_id = event
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !event_id.is_empty() && !self.seen_ids.insert(event_id.clone()) {
            return AppendResult::empty();
        }

        // 2. Extract parent_uuid (now under agent_payload), compute depth
        let parent_uuid = event
            .get("data")
            .and_then(|d| d.get("agent_payload"))
            .and_then(|ap| ap.get("parent_uuid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let depth = match &parent_uuid {
            Some(pid) => self.depths.get(pid).copied().unwrap_or(0) + 1,
            None => 0,
        };
        if !event_id.is_empty() {
            self.depths.insert(event_id.clone(), depth);
            self.parents.insert(event_id.clone(), parent_uuid.clone());
        }

        // 3. Transform to ViewRecords (deserialize to typed CloudEvent)
        self.event_count += 1;
        let view_records = match serde_json::from_value::<CloudEvent>(event.clone()) {
            Ok(ce) => from_cloud_event(&ce),
            Err(_) => vec![],
        };
        if view_records.is_empty() {
            return AppendResult::empty();
        }

        // 4. Capture full payloads for truncated records
        for vr in &view_records {
            if let RecordBody::ToolResult(tr) = &vr.body {
                if let Some(output) = &tr.output {
                    if output.len() > TRUNCATION_THRESHOLD {
                        self.full_payloads.insert(vr.id.clone(), output.clone());
                    }
                }
            }
        }

        // 4b. Accumulate token usage
        for vr in &view_records {
            if let RecordBody::TokenUsage(tu) = &vr.body {
                self.total_input_tokens += tu.input_tokens.unwrap_or(0);
                self.total_output_tokens += tu.output_tokens.unwrap_or(0);
            }
        }

        // 5. Extract labels (first prompt + git branch)
        let mut label_changed = false;
        if self.label.is_none() {
            for vr in &view_records {
                if let RecordBody::UserMessage(um) = &vr.body {
                    let text = match &um.content {
                        MessageContent::Text(t) => t.clone(),
                        MessageContent::Blocks(blocks) => {
                            blocks.iter()
                                .find_map(|b| match b {
                                    open_story_views::unified::ContentBlock::Text { text } => Some(text.clone()),
                                    open_story_views::unified::ContentBlock::CodeBlock { text, .. } => Some(text.clone()),
                                    _ => None,
                                })
                                .unwrap_or_default()
                        }
                    };
                    if !text.is_empty() {
                        self.label = Some(text.chars().take(50).collect());
                        label_changed = true;
                        break;
                    }
                }
            }
        }
        if self.branch.is_none() {
            if let Some(branch) = event
                .get("data")
                .and_then(|d| d.get("agent_payload"))
                .and_then(|ap| ap.get("git_branch"))
                .and_then(|v| v.as_str())
            {
                if !branch.is_empty() {
                    self.branch = Some(branch.to_string());
                    label_changed = true;
                }
            }
        }

        // 6. Compute filter deltas
        let mut filter_deltas: HashMap<String, i32> = HashMap::new();
        for name in FILTER_NAMES {
            let delta: i32 = view_records
                .iter()
                .filter(|r| filter_matches(name, r))
                .count() as i32;
            if delta > 0 {
                *self.filter_counts.entry(name.to_string()).or_insert(0) += delta as usize;
                filter_deltas.insert(name.to_string(), delta);
            }
        }

        // 7. Store records
        self.records.extend(view_records.clone());

        AppendResult {
            records: view_records,
            filter_deltas,
            label_changed,
        }
    }

    /// Number of CloudEvents appended (not ViewRecords).
    pub fn event_count(&self) -> usize {
        self.event_count
    }

    /// Tree depth for a given event ID.
    pub fn node_depth(&self, event_id: &str) -> u16 {
        self.depths.get(event_id).copied().unwrap_or(0)
    }

    /// Cached filter counts — O(1) read.
    pub fn filter_counts(&self) -> &HashMap<String, usize> {
        &self.filter_counts
    }

    /// All ViewRecords in this session (for full recount verification).
    pub fn timeline_rows(&self) -> &[ViewRecord] {
        &self.records
    }

    /// Cached metadata — O(1), no iteration.
    pub fn query_meta(&self) -> ProjectionMeta {
        ProjectionMeta {
            event_count: self.event_count,
            filter_counts: self.filter_counts.clone(),
        }
    }

    /// Full payload for a truncated event, if stored.
    pub fn full_payload(&self, event_id: &str) -> Option<&str> {
        self.full_payloads.get(event_id).map(|s| s.as_str())
    }

    /// Parent UUID for a given event ID.
    pub fn node_parent(&self, event_id: &str) -> Option<&str> {
        self.parents
            .get(event_id)
            .and_then(|opt| opt.as_deref())
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Human-readable session label (first user prompt, truncated).
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Git branch name for this session.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// Accumulated input token count.
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input_tokens
    }

    /// Accumulated output token count.
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens
    }
}

// ── Inline tests (audit walk #5, 2026-04-15) ──────────────────────
//
// 402-line file, zero #[cfg(test)] mod before this commit. Filter logic
// is ~100 LOC of pure string-matching that drives sidebar counts and
// the saved-filter UI. Untested means a typo in a substring breaks
// classification silently. These tests lock the boundary table per
// filter so additions/removals are explicit.

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_views::unified::{
        AssistantMessage, ContentBlock, ErrorRecord, MessageContent, RecordBody,
        Reasoning, ToolCall, ToolResult, UserMessage,
    };
    use serde_json::json;

    fn vr_with(body: RecordBody) -> ViewRecord {
        ViewRecord {
            id: "evt".to_string(),
            seq: 1,
            session_id: "s".to_string(),
            timestamp: "2026-04-15T00:00:00Z".to_string(),
            agent_id: None,
            is_sidechain: false,
            body,
        }
    }

    fn user_msg(text: &str) -> ViewRecord {
        vr_with(RecordBody::UserMessage(UserMessage {
            content: MessageContent::Text(text.to_string()),
            images: vec![],
        }))
    }

    fn asst_msg(text: &str) -> ViewRecord {
        vr_with(RecordBody::AssistantMessage(Box::new(AssistantMessage {
            model: "x".to_string(),
            content: vec![ContentBlock::Text { text: text.to_string() }],
            stop_reason: None,
            end_turn: None,
            phase: None,
        })))
    }

    fn tool_call(name: &str, input: serde_json::Value) -> ViewRecord {
        vr_with(RecordBody::ToolCall(Box::new(ToolCall {
            call_id: "c".to_string(),
            name: name.to_string(),
            input: input.clone(),
            raw_input: input,
            typed_input: None,
            status: None,
        })))
    }

    fn tool_result(output: &str, is_error: bool) -> ViewRecord {
        vr_with(RecordBody::ToolResult(ToolResult {
            call_id: "c".to_string(),
            output: Some(output.to_string()),
            is_error,
            tool_outcome: None,
        }))
    }

    fn reasoning() -> ViewRecord {
        vr_with(RecordBody::Reasoning(Reasoning {
            summary: vec![],
            content: Some("thinking".to_string()),
            encrypted: false,
        }))
    }

    fn error_record() -> ViewRecord {
        vr_with(RecordBody::Error(ErrorRecord {
            code: "x".to_string(),
            message: "boom".to_string(),
            details: None,
        }))
    }

    // ── filter_matches: family-level coverage ─────────────────────────

    #[test]
    fn filter_all_accepts_everything() {
        assert!(filter_matches("all", &user_msg("hi")));
        assert!(filter_matches("all", &asst_msg("yo")));
        assert!(filter_matches("all", &reasoning()));
    }

    #[test]
    fn filter_user_accepts_only_user_messages() {
        assert!(filter_matches("user", &user_msg("hi")));
        assert!(!filter_matches("user", &asst_msg("yo")));
        assert!(!filter_matches("user", &reasoning()));
    }

    #[test]
    fn filter_narrative_accepts_user_and_assistant_messages() {
        assert!(filter_matches("narrative", &user_msg("hi")));
        assert!(filter_matches("narrative", &asst_msg("yo")));
        assert!(!filter_matches("narrative", &reasoning()));
        assert!(!filter_matches("narrative", &tool_call("Read", json!({}))));
    }

    #[test]
    fn filter_tools_accepts_call_or_result() {
        assert!(filter_matches("tools", &tool_call("Read", json!({}))));
        assert!(filter_matches("tools", &tool_result("ok", false)));
        assert!(!filter_matches("tools", &user_msg("hi")));
    }

    #[test]
    fn filter_thinking_only_reasoning() {
        assert!(filter_matches("thinking", &reasoning()));
        assert!(!filter_matches("thinking", &user_msg("hmm")));
    }

    #[test]
    fn filter_errors_includes_error_record_and_failing_tool_results() {
        assert!(filter_matches("errors", &error_record()));
        assert!(filter_matches("errors", &tool_result("oops", true)));
        assert!(!filter_matches("errors", &tool_result("ok", false)));
        assert!(!filter_matches("errors", &user_msg("hi")));
    }

    // ── tool-name filters ─────────────────────────────────────────────

    #[test]
    fn filter_reading_matches_read_glob_grep() {
        for name in ["Read", "Glob", "Grep"] {
            assert!(filter_matches("reading", &tool_call(name, json!({}))), "tool {name}");
        }
        assert!(!filter_matches("reading", &tool_call("Bash", json!({"command": "ls"}))));
        assert!(!filter_matches("reading", &tool_call("Edit", json!({}))));
    }

    #[test]
    fn filter_editing_matches_edit_and_write() {
        assert!(filter_matches("editing", &tool_call("Edit", json!({}))));
        assert!(filter_matches("editing", &tool_call("Write", json!({}))));
        assert!(!filter_matches("editing", &tool_call("Read", json!({}))));
    }

    #[test]
    fn filter_deep_matches_agent_tool() {
        assert!(filter_matches("deep", &tool_call("Agent", json!({}))));
        assert!(!filter_matches("deep", &tool_call("Bash", json!({}))));
    }

    // ── bash command filters ──────────────────────────────────────────

    #[test]
    fn filter_bash_git_matches_git_commands() {
        let bash = |cmd: &str| tool_call("Bash", json!({"command": cmd}));
        assert!(filter_matches("bash.git", &bash("git status")));
        assert!(filter_matches("bash.git", &bash("git log --oneline")));
        // Substring match: "git " requires the trailing space.
        // This intentionally excludes "github" or "gitignore".
        assert!(!filter_matches("bash.git", &bash("github-cli pr list")));
        assert!(!filter_matches("bash.git", &bash("ls")));
    }

    #[test]
    fn filter_bash_test_matches_known_runners() {
        let bash = |cmd: &str| tool_call("Bash", json!({"command": cmd}));
        assert!(filter_matches("bash.test", &bash("cargo test")));
        assert!(filter_matches("bash.test", &bash("npm test --silent")));
        assert!(filter_matches("bash.test", &bash("pytest -k foo")));
        assert!(!filter_matches("bash.test", &bash("cargo build")));
    }

    #[test]
    fn filter_bash_only_matches_bash_tool() {
        // git as command outside a Bash tool should not match
        let other = tool_call("Read", json!({"command": "git status"}));
        assert!(!filter_matches("bash.git", &other));
    }

    // ── result-output filters (the heuristic-y ones) ──────────────────

    #[test]
    fn filter_compile_error_matches_known_signatures() {
        assert!(filter_matches("compile_error", &tool_result("error[E0277]: trait not satisfied", true)));
        assert!(filter_matches("compile_error", &tool_result("TS2345 incompatible types", true)));
        assert!(filter_matches("compile_error", &tool_result("SyntaxError: unexpected token", true)));
        assert!(!filter_matches("compile_error", &tool_result("compiled successfully", false)));
    }

    #[test]
    fn filter_test_pass_matches_pass_signatures() {
        assert!(filter_matches("test_pass", &tool_result("test result: ok. 5 passed; 0 failed", false)));
        assert!(filter_matches("test_pass", &tool_result("Tests  3 passed", false)));
        assert!(!filter_matches("test_pass", &tool_result("compiling", false)));
    }

    #[test]
    fn filter_test_fail_distinguishes_failure_from_zero_failed() {
        // "0 failed" should NOT trigger test_fail
        assert!(!filter_matches(
            "test_fail",
            &tool_result("Tests  3 passed, 0 failed", false)
        ));
        // Real failure should trigger
        assert!(filter_matches(
            "test_fail",
            &tool_result("Tests  1 failed, 2 passed", true)
        ));
        // Rust's FAILED uppercase
        assert!(filter_matches(
            "test_fail",
            &tool_result("test cycle::tests::it_works ... FAILED", true)
        ));
    }

    // ── pattern-based filters (always-false today) ────────────────────

    #[test]
    fn pattern_filters_are_currently_always_false() {
        // Documented to return false until pattern detection wires in.
        // If this ever changes, the change is visible here.
        for name in ["patterns", "tests", "agents", "git"] {
            assert!(!filter_matches(name, &user_msg("anything")));
            assert!(!filter_matches(name, &tool_call("Bash", json!({"command": "git status"}))));
        }
    }

    // ── unknown filter names ──────────────────────────────────────────

    #[test]
    fn unknown_filter_name_returns_false_silently() {
        // Documenting the behavior — unknown filters don't error,
        // they just match nothing. This is the safe default but
        // means typos in client filter requests are silent.
        assert!(!filter_matches("nonexistent_filter", &user_msg("hi")));
    }

    // ── SessionProjection: depth contract ─────────────────────────────

    #[test]
    fn projection_depth_is_zero_for_orphan_events() {
        // Latent contract: if a child event arrives BEFORE its parent,
        // the parent isn't in `depths` yet. Code at projection.rs:232
        // does `depths.get(pid).unwrap_or(0) + 1` — orphan child gets
        // depth 1, not "depth = parent+1 (currently unknown)".
        //
        // In production, events arrive in monotonic order from one
        // watcher, so this isn't hit. Documenting the assumption.
        let mut proj = SessionProjection::new("s");
        let orphan = json!({
            "id": "evt-child",
            "data": {
                "agent_payload": {
                    "_variant": "claude-code",
                    "meta": {"agent": "claude-code"},
                    "parent_uuid": "evt-parent-not-yet-seen",
                    "text": "hi"
                }
            }
        });
        proj.append(&orphan);
        // Reads back as 1 (default-0 + 1), not as some "unknown" sentinel.
        assert_eq!(proj.node_depth("evt-child"), 1);
    }
}

/// Classify whether an event subtype is ephemeral (progress) or durable.
/// Ephemeral events are shown transiently in the UI but not accumulated in state.
pub fn is_ephemeral(subtype: Option<&str>) -> bool {
    subtype
        .map(|s| s.starts_with("progress."))
        .unwrap_or(false)
}
