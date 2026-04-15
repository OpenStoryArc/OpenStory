# Schema Map

Stage 1 of the schema-registry work. **Mapping only — no tests, no code, no schemas.**

Goal: name every type that crosses a serialization boundary, so Stage 2 (test design) and Stage 3 (TDD implementation) have an accurate surface to work against.

Related: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

---

## Principles for deciding what gets a schema

1. **Own-format in, opaque-format preserved**. Agents' native JSONL (Claude Code, pi-mono, Hermes) are not schemas we author — they're what the agent writes. We never validate them; we pass them through as `data.raw`. That field stays schema-free (`object`, no further constraints).
2. **Every boundary we serialize across gets a schema.** NATS bytes, SQLite TEXT blobs, REST JSON, WebSocket JSON, JSONL escape hatch.
3. **Rust types are the source of truth.** The schema is *derived* (generated), not hand-authored. Hand-authored schemas drift; generated schemas fail the diff-check when a Rust type changes without a schema regen.
4. **Schema artifacts are committed files**, not runtime objects or a registry service. Sovereignty principle: no external dependency, grep-friendly, diff-able in PRs.

---

## The type inventory

Everything that gets serialized, grouped by layer.

### Layer A — CloudEvent envelope

> Source of truth: `rs/core/src/cloud_event.rs`

| Type | Shape | Notes |
|------|-------|-------|
| `CloudEvent` | `{ specversion, id, source, type, time, datacontenttype, data, subtype?, subject?, dataschema?, agent? }` | `type` is always `"io.arc.event"`. `agent` is the extension attribute (`"claude-code"` \| `"pi-mono"` \| `"hermes"`). |
| `EventData` | `{ raw, seq, session_id, agent_payload? }` | `raw` is the untyped opaque pass-through. `agent_payload` is the typed lift. |
| `PayloadMeta` | `{ agent }` | Just the variant tag, nested inside each AgentPayload. |

### Layer B — Agent payloads (tagged union)

> Source of truth: `rs/core/src/event_data.rs`
> Serde: `#[serde(tag = "_variant")]` — internally tagged.

| Variant | `_variant` | Shape |
|---------|-----------|-------|
| `AgentPayload::ClaudeCode` | `"claude-code"` | `ClaudeCodePayload` — ~30 optional fields (uuid, parent_uuid, cwd, timestamp, version, text, model, stop_reason, tool, args, token_usage, command, duration_ms, is_error, tool_outcome, agent_id, is_sidechain, plan, hook_event_name, transcript_path, …) |
| `AgentPayload::PiMono` | `"pi-mono"` | `PiMonoPayload` — similar surface, plus `content_types`, `tool_call_id`, `provider`, `thinking_level`, `model_id`, `exit_code`, `summary`, `tokens_before`, `first_kept_entry_id` |
| `AgentPayload::Hermes` | `"hermes"` | `HermesPayload` — delegated event shape (varies by subtype) |

Every variant carries `meta: PayloadMeta` as a required field.

### Layer C — Domain event (tool outcome)

> Source of truth: `rs/core/src/event_data.rs::ToolOutcome`
> Serde: `#[serde(tag = "type")]` — internally tagged.

8 variants: `FileCreated`, `FileModified`, `FileRead`, `FileWriteFailed`, `FileReadFailed`, `SearchPerformed`, `CommandExecuted`, `SubAgentSpawned`. Attached to `ToolResult` records after pairing with their call.

### Layer D — ViewRecord (BFF output)

> Source of truth: `rs/views/src/view_record.rs`, `rs/views/src/unified.rs`

| Type | Shape | Notes |
|------|-------|-------|
| `ViewRecord` | `{ id, seq, session_id, timestamp, agent_id?, is_sidechain, …body }` | `body` is `#[serde(flatten)]` so `record_type` + `payload` appear at top level. |
| `RecordBody` | tagged `record_type`, content `payload` | 13 variants — see below. |
| `MessageContent` | untagged: `String` \| `Vec<ContentBlock>` | **serde-untagged** — worth flagging for schema tooling (JSON Schema handles this as `oneOf`). |
| `ContentBlock` | tagged `type` | `text` \| `code_block` \| `image`. |
| `TokenScope` | enum string | `turn` \| `session_total`. |

**RecordBody variants** (`record_type` discriminator): `session_meta`, `turn_start`, `turn_end`, `user_message`, `assistant_message`, `reasoning`, `tool_call`, `tool_result`, `token_usage`, `context_compaction`, `file_snapshot`, `system_event`, `error`.

### Layer E — WireRecord (WS output)

> Source of truth: `rs/views/src/wire_record.rs`

```
WireRecord = ViewRecord (flattened) + depth: u16 + parent_uuid: Option<String>
           + truncated: bool + payload_bytes: u64
```

Same JSON shape as ViewRecord with four extra top-level keys. The flatten-plus-flatten (WireRecord flattens ViewRecord, ViewRecord flattens RecordBody) means the final JSON has `id`, `seq`, `session_id`, `timestamp`, `agent_id?`, `is_sidechain`, `record_type`, `payload`, `depth`, `parent_uuid`, `truncated`, `payload_bytes` — all at the top level.

### Layer F — BroadcastMessage (WS envelope)

> Source of truth: `rs/server/src/broadcast.rs`
> Serde: `#[serde(tag = "kind")]` — internally tagged.

| Variant | `kind` | Payload |
|---------|--------|---------|
| `ViewRecords` | `"view_records"` | `{ session_id, view_records, project_id?, project_name? }` |
| `Enriched` | `"enriched"` | `{ session_id, records: Vec<WireRecord>, ephemeral: Vec<ViewRecord>, filter_deltas, patterns, project_id?, project_name?, session_label?, session_branch?, total_input_tokens?, total_output_tokens? }` |
| `PlanSaved` | `"plan_saved"` | `{ session_id }` |

### Layer G — Patterns

> Source of truth: `rs/patterns/` crate
> `PatternEvent`, `StructuralTurn` — carried inside `BroadcastMessage::Enriched.patterns` and published to `patterns.{project}.{session}` on NATS.

### Layer H — Subtype taxonomy

Enumerated string constants for `CloudEvent.subtype`. Not a type — a closed set of allowed values. Worth its own schema file because these strings appear hard-coded across translator dispatch, from_cloud_event match arms, UI filters, analytics queries.

Current set:
- `message.user.prompt`, `message.user.tool_result`
- `message.assistant.text`, `message.assistant.thinking`, `message.assistant.tool_use`
- `system.turn.complete`, `system.error`, `system.compact`, `system.hook`, `system.session_start`, `system.model_change`
- `progress.bash`, `progress.agent`, `progress.hook`
- `file.snapshot`
- `queue.enqueue`, `queue.dequeue`

---

## The boundaries where schemas apply

Which type crosses which wire. This is the table Stage 2 tests will mirror.

| # | Boundary | Shape on the wire | Schema needed |
|---|----------|-------------------|---------------|
| 1 | JSONL escape hatch (`data/*.jsonl`) | `CloudEvent` per line | `cloud_event.schema.json` |
| 2 | NATS `events.{project}.{session}.*` | `IngestBatch { session_id, project_id, events: Vec<CloudEvent> }` | `ingest_batch.schema.json` + CloudEvent |
| 3 | NATS `patterns.{project}.{session}` | `PatternEvent` (or batch) | `pattern_event.schema.json` |
| 4 | SQLite `events.payload` (TEXT blob) | `CloudEvent` | CloudEvent (same as #1) |
| 5 | REST `/api/sessions/{id}/events` | `Vec<CloudEvent>` (as JSON array) | CloudEvent |
| 6 | REST `/api/sessions/{id}/view-records` | `Vec<ViewRecord>` | `view_record.schema.json` |
| 7 | REST `/api/sessions/{id}/records` | `Vec<WireRecord>` | `wire_record.schema.json` |
| 8 | WebSocket frames | `BroadcastMessage` (tagged) | `broadcast_message.schema.json` |
| 9 | REST `/api/sessions` | `{ sessions: Vec<SessionRow> }` | `session_row.schema.json` |
| 10 | REST `/api/search` | `Vec<FtsSearchResult>` | `fts_result.schema.json` |

**Not schema'd** (by design):
- Agent JSONL inputs (`~/.claude/projects/**/*.jsonl`, `~/.pi/agent/sessions/**/*.jsonl`) — foreign formats, sovereignty
- `CloudEvent.data.raw` — opaque pass-through, inherits agent's native format
- UI → Server REST request bodies for now (no mutations of stored data; only lazy-load fetch by id)

---

## Special cases worth naming before Stage 2

These are the ones where "just generate from Rust" gets subtle.

### SC-1. `flatten` + `tagged` composition
`WireRecord` flattens `ViewRecord`, which flattens `RecordBody` (tagged). The final JSON is one flat object with `record_type` + `payload` at the top level. `schemars` handles this, but the generated schema needs review — historically, schema tooling has trouble with multi-level flatten + internally-tagged enums.

### SC-2. `#[serde(untagged)]` on `MessageContent`
`MessageContent = String | Vec<ContentBlock>`. Tooling that generates TypeScript or Python from JSON Schema handles `oneOf`, but not all generators produce idiomatic output. Worth checking early.

### SC-3. Internally-tagged payload with many optional fields
`PiMonoPayload` has ~30 `Option<T>` fields. The generated schema will be a huge `properties` block. That's fine for validation, awkward for humans. Consider whether the per-subtype narrowing proposed in SC-4 makes this tractable.

### SC-4. Should we narrow by subtype?
Today `CloudEvent` is one schema — every subtype uses the same envelope and the same `AgentPayload` variants with the same ~30 optional fields. Subtype-specific schemas (e.g. `message_assistant_tool_use_claude_code.schema.json` narrowing which AgentPayload fields are required/allowed) would be much tighter but multiply 13 subtypes × 3 agents = 39 schemas.

**Proposal:** start with the coarse schema (one CloudEvent schema). Add per-subtype narrowing as a Stage 4 refinement if coarse validation proves too permissive.

### SC-5. `data.raw` stays `object`
This is the most important single decision. Every agent's native format lives here, and we *do not* validate it. The schema says `type: object` and stops. Any future pressure to tighten this is a sovereignty red flag.

### SC-6. Versioning
CloudEvent has no schema version field today. Subtypes evolve (new ones added, old ones never removed but sometimes deprecated). We don't need versioning in Stage 3, but Stage 4+ may. Options:
- `dataschema` field on CloudEvent already exists per the CE 1.0 spec — we'd use it to point at the schema URL.
- Schema filenames carry a version: `cloud_event.v1.schema.json`. Bump on breaking change.
- Both. The spec-compliant move.

Flagging so we don't forget.

### SC-7. `depth: u16` and `payload_bytes: u64`
JSON Schema can express `maximum` / `minimum` for these. Worth adding (u16 max = 65535, u64 max = 2⁶⁴-1) so over-the-wire values are range-checked.

---

## What's out of scope for the schema map

- **UI types (TypeScript)**. Those are consumers of the schemas listed above. Stage 4 generates TS from these schemas. But for Stage 1 we're not enumerating TS-side types — they're downstream.
- **Config files** (`data/config.toml`, `nats.conf`). Not flowing data, not a serialization boundary.
- **Hook HTTP payloads** (`POST /hooks`). These are agent-written request bodies — in the spirit of "we don't validate agent output," they stay untyped (we extract what we need and ignore the rest).
- **Qdrant vectors** (if semantic is enabled). Embedding vectors are opaque floats; no schema needed.

---

## Summary — what Stage 2 will plan tests for

Starting coarse, the schema files to produce in Stage 3 are:

1. `cloud_event.schema.json` (envelope + `AgentPayload` enum + `ToolOutcome`)
2. `view_record.schema.json` (envelope + `RecordBody` enum + `ContentBlock`, `MessageContent`, `TokenScope`)
3. `wire_record.schema.json` (extends view_record)
4. `broadcast_message.schema.json` (the three `kind` variants)
5. `ingest_batch.schema.json` (NATS message envelope)
6. `pattern_event.schema.json` (from patterns crate)
7. `session_row.schema.json`, `fts_result.schema.json` (REST list/search)
8. `subtypes.schema.json` (enum of allowed subtype strings)

Eight schemas. Each will have a test of the shape:

- **Known-good fixture validates** (real sample JSON passes `.validate()`)
- **Known-bad fixture fails** (missing required field, wrong type, wrong enum value)
- **Round-trip lossless** (Rust → serialize → validate → deserialize back → equal to input)
- **Drift check** (regenerate schema from current Rust types, diff against committed file — identical or CI fails)

Stage 2 will lay out those tests per schema, per fixture, in detail. Stage 3 implements them TDD.

---

## Open questions for review before Stage 2

1. **Tooling decision**: `schemars` crate for generation. Acceptable? Only real alternative is hand-authored. I'm assuming generated.
2. **Coarse vs narrow**: start coarse (one CloudEvent schema), refine later. Acceptable?
3. **Schema file location**: proposing `schemas/` at repo root, committed, CI-checked for drift. Alternative: `rs/schemas/` under the Rust workspace.
4. **Test home**: per-crate inline tests, or a new `rs/schemas/` crate that holds tests and generation logic. I lean toward a new crate — keeps `build.rs` complexity off the main crates.
5. **Is the JSONL escape-hatch schema validation also a Stage 3 deliverable, or Stage 5?** It's the most sovereignty-relevant; I'd propose making it Stage 3's *first* test case so the principle drives the work.
