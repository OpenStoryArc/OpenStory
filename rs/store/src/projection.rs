//! SessionProjection — incremental cache with filter counts.
//!
//! Replaces ad-hoc recomputation with a stateful projection per session.
//! Maintains tree structure, filter counts, and full payloads incrementally.
//! All derived state is updated on append, never recomputed from scratch.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

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

        // 3. Transform to ViewRecords
        self.event_count += 1;
        let view_records = from_cloud_event(event);
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

/// Classify whether an event subtype is ephemeral (progress) or durable.
/// Ephemeral events are shown transiently in the UI but not accumulated in state.
pub fn is_ephemeral(subtype: Option<&str>) -> bool {
    subtype
        .map(|s| s.starts_with("progress."))
        .unwrap_or(false)
}
