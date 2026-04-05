# Spec: Domain Events for OpenStory

*Adding a deterministic "what changed in the world" layer to OpenStory's event pipeline.*

## Context

OpenStory currently has **technical events** — `tool_call`, `tool_result`, `user_message`, `assistant_message`. These describe what the machine did. They're the API contract.

This spec adds **domain events** — `FileCreated`, `FileModified`, `CommandExecuted`, `SearchPerformed`. These describe what changed in the world. They're deterministic projections derived from the technical events.

The concept was prototyped in `scheme/prototype/domain.ts` (20 tests, all passing) and validated against real session data (1900+ records). The mapping from tool calls to domain events is a pure function — same input, same output, no heuristics.

The monadic EventData refactor (`feat/monadic-event-data` branch, commit `e118fbc`) provides the typed foundation. `ClaudeCodePayload` already has `tool: Option<String>` and `args: Option<Value>`. This spec extends that with a `tool_outcome` field.

## Design Principle

Technical events and domain events are separate. One is what the machine did. The other is what it meant. Both are emitted. Neither replaces the other.

```
Technical (Layer 2):  tool_call { name: "Write", input: { file_path: "01-types.scm" } }
                      tool_result { output: "File created successfully..." }

Domain (Layer 4):     FileCreated { path: "01-types.scm" }
```

The technical event is always emitted. The domain event is derived from it. You can query either independently.

## Domain Event Types

```rust
/// What changed in the world. Derived from tool_call + tool_result pairs.
/// Deterministic: same input → same output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolOutcome {
    /// Write tool + "created successfully" in result
    FileCreated { path: String },
    /// Write tool + "updated successfully" in result, or any Edit tool
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
    /// Agent tool
    SubAgentSpawned { description: String },
}
```

## Where It Lives

### In `ClaudeCodePayload` (rs/core/src/event_data.rs)

Add one field:

```rust
pub struct ClaudeCodePayload {
    // ... existing fields ...

    /// Domain event: what this tool call changed in the world.
    /// Only present on tool_result events (the outcome of a tool_call).
    /// Derived deterministically from tool name + result output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_outcome: Option<ToolOutcome>,
}
```

Same pattern as every other optional field on the payload. Same `skip_serializing_if`. Same `#[serde(default)]`.

### In `PiMonoPayload` (rs/core/src/event_data.rs)

Same field. Pi-mono has different tool names (`exec`, `read`, `write`, `edit`) but the same concept. The derivation function is agent-specific, matching the existing per-agent translator pattern.

### In `translate.rs` (rs/core/src/translate.rs)

New function following the `apply_*` pattern:

```rust
/// Derive the domain event from a tool_result.
/// Deterministic: same tool name + result → same outcome.
fn derive_tool_outcome(
    tool_name: &str,
    tool_input: &Value,
    result_output: &str,
    is_error: bool,
) -> Option<ToolOutcome> {
    match tool_name {
        "Write" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed { path, reason: result_output.to_string() })
            } else if result_output.contains("created successfully") {
                Some(ToolOutcome::FileCreated { path })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Edit" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed { path, reason: result_output.to_string() })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Read" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if is_error {
                Some(ToolOutcome::FileReadFailed { path, reason: result_output.to_string() })
            } else {
                Some(ToolOutcome::FileRead { path })
            }
        }
        "Grep" | "Glob" => {
            let pattern = tool_input.get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SearchPerformed { pattern, source: "filesystem".to_string() })
        }
        "WebSearch" | "WebFetch" => {
            let query = tool_input.get("query")
                .or_else(|| tool_input.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SearchPerformed { pattern: query, source: "web".to_string() })
        }
        "Bash" => {
            let command = tool_input.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::CommandExecuted { command, succeeded: !is_error })
        }
        "Agent" => {
            let description = tool_input.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolOutcome::SubAgentSpawned { description })
        }
        _ => None,  // Unknown tool — no domain event
    }
}
```

### Where to call it

In `translate.rs`, the translator currently processes tool_result lines within the `"result"` type branch. The tool_result has access to the preceding tool_call's name and input via the transcript structure (Claude Code's JSONL includes the tool name and input on result lines, or they can be correlated by `tool_use_id`).

The call site:

```rust
// In the tool_result branch of translate_line():
if let Some(outcome) = derive_tool_outcome(
    &tool_name,
    &tool_input,
    &result_text,
    is_error,
) {
    payload.tool_outcome = Some(outcome);
}
```

### In `translate_pi.rs`

Same function, adapted for pi-mono's tool names:

```rust
fn derive_tool_outcome_pi(
    tool_name: &str,
    tool_input: &Value,
    result_output: &str,
    is_error: bool,
) -> Option<ToolOutcome> {
    match tool_name {
        "exec" => { /* → CommandExecuted */ }
        "read" => { /* → FileRead */ }
        "write" => { /* → FileCreated or FileModified */ }
        "edit" => { /* → FileModified */ }
        "web_search" | "web_fetch" => { /* → SearchPerformed */ }
        _ => None,
    }
}
```

### In the Views Layer (rs/views/src/from_cloud_event.rs)

The `tool_outcome` is available on the payload. Two options:

**Option A (minimal):** Surface `tool_outcome` as a field on the existing `ToolResult` RecordBody:

```rust
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
    pub tool_outcome: Option<ToolOutcome>,  // NEW
}
```

**Option B (richer):** Add new RecordBody variants for domain events, emitted alongside the technical ToolResult:

```rust
pub enum RecordBody {
    // ... existing variants ...
    FileCreated(FileCreatedRecord),
    FileModified(FileModifiedRecord),
    CommandExecuted(CommandExecutedRecord),
    // etc.
}
```

**Recommendation: Option A first.** It's additive — no new record types, no new UI components needed. The `tool_outcome` rides on the existing `ToolResult` record. Pattern detectors and the UI can read it if they want, ignore it if they don't. Option B is a future step if domain events need their own lifecycle (their own filters, their own views, their own queries).

### In the Pattern Detectors

The eval-apply detector (from `scheme/DESIGN_EVAL_APPLY_VIEW.md` and `scheme/IMPLEMENTATION.md`) can consume `tool_outcome` directly instead of re-deriving domain events from tool names and results:

```rust
RecordBody::ToolResult(tr) => {
    if let Some(ref outcome) = tr.tool_outcome {
        match outcome {
            ToolOutcome::FileCreated { path } => { /* track */ }
            ToolOutcome::CommandExecuted { succeeded, .. } => { /* track */ }
            // etc.
        }
    }
}
```

The existing detectors (TestCycleDetector, etc.) can also benefit. TestCycleDetector currently pattern-matches on Bash command strings to detect test runs. With `tool_outcome`, it could match on `CommandExecuted { command, succeeded }` directly — cleaner and more reliable.

## Subtype Extension (Optional, Future)

For richer querying, the CloudEvent subtype could be extended:

```
Existing:
  message.assistant.tool_use

New (on tool_result events):
  tool.outcome.file_created
  tool.outcome.file_modified
  tool.outcome.command_executed
  tool.outcome.search_performed
  tool.outcome.agent_spawned
```

This enables SQL queries like:

```sql
SELECT * FROM events WHERE subtype = 'tool.outcome.file_created'
```

But this is an additive future step. The `tool_outcome` field on the payload is sufficient for the first iteration.

## Test Strategy

### Unit tests (rs/core/src/translate.rs)

```rust
#[test]
fn write_created_successfully_produces_file_created() {
    let outcome = derive_tool_outcome(
        "Write",
        &json!({"file_path": "/scheme/01-types.scm"}),
        "File created successfully at: /scheme/01-types.scm",
        false,
    );
    assert!(matches!(outcome, Some(ToolOutcome::FileCreated { path }) if path == "/scheme/01-types.scm"));
}

#[test]
fn edit_produces_file_modified() {
    let outcome = derive_tool_outcome(
        "Edit",
        &json!({"file_path": "/README.md"}),
        "The file has been updated successfully.",
        false,
    );
    assert!(matches!(outcome, Some(ToolOutcome::FileModified { path }) if path == "/README.md"));
}

#[test]
fn bash_error_produces_command_failed() {
    let outcome = derive_tool_outcome(
        "Bash",
        &json!({"command": "cargo test"}),
        "test result: FAILED",
        true,
    );
    assert!(matches!(outcome, Some(ToolOutcome::CommandExecuted { succeeded: false, .. })));
}

#[test]
fn unknown_tool_produces_none() {
    let outcome = derive_tool_outcome("FutureTool", &json!({}), "ok", false);
    assert!(outcome.is_none());
}
```

### Integration tests

Run the full translate pipeline on fixture data, verify `tool_outcome` is present on tool_result events and absent on other events.

### Prototype validation

Our `scheme/prototype/domain.ts` has 20 tests covering every tool type. The Rust implementation should produce identical results for identical inputs. A cross-validation script could feed the same tool_call/tool_result pairs through both and compare outputs.

## Implementation Sequence

1. **Add `ToolOutcome` enum** to `event_data.rs`. Add `tool_outcome: Option<ToolOutcome>` to `ClaudeCodePayload` and `PiMonoPayload`. Update `new()` constructors.

2. **Add `derive_tool_outcome()`** to `translate.rs`. Call it in the tool_result branch. Unit tests.

3. **Add `derive_tool_outcome_pi()`** to `translate_pi.rs`. Same pattern for pi-mono tool names. Unit tests.

4. **Surface in views**: Add `tool_outcome` field to `ToolResult` RecordBody in `from_cloud_event.rs`.

5. **Verify**: `cargo test` — all existing tests pass + new tests pass. No breaking changes to existing consumers.

6. **Future**: Eval-apply detector consumes `tool_outcome`. TestCycleDetector simplified. UI shows domain events. Subtype extension for SQL queries.

## Files Modified

| File | Change |
|------|--------|
| `rs/core/src/event_data.rs` | Add `ToolOutcome` enum + field on both payloads |
| `rs/core/src/translate.rs` | Add `derive_tool_outcome()` + call site |
| `rs/core/src/translate_pi.rs` | Add `derive_tool_outcome_pi()` + call site |
| `rs/views/src/from_cloud_event.rs` | Surface `tool_outcome` on `ToolResult` RecordBody |
| `rs/views/src/view_record.rs` | Add field to `ToolResult` struct |
| `rs/tests/test_translate.rs` | Add domain event tests |
| `rs/tests/test_translate_pi.rs` | Add domain event tests |

## Relationship to Prototype

The TypeScript prototype (`scheme/prototype/domain.ts`) is the executable spec. The Rust implementation should produce the same output for the same input. The mapping table:

| Prototype (`domain.ts`) | Production (`event_data.rs`) |
|--------------------------|------------------------------|
| `toDomainEvent()` | `derive_tool_outcome()` |
| `DomainEvent` type | `ToolOutcome` enum |
| `buildDomainTurn()` | Views layer + pattern detector |
| `AggregateChange` | `SessionProjection` enrichment |

The prototype proves the concept. The Rust implementation makes it durable.
