---
slug: finish-cloudevent-typed-migration
spec: docs/specs/finish-cloudevent-typed-migration.md
---

# Finish CloudEvent::new Typed EventData Migration — Architecture Design

## Overview
This is a mechanical fix to complete an already-designed refactor. No new architecture decisions are needed. The typed `EventData` design is already in production in `rs/core/`; the remaining work is updating test fixtures and stale call sites in `rs/server/` and `rs/tests/` to match.

## Components
No new components. Affected modules:

| Module | Change |
|--------|--------|
| `rs/server/src/ingest.rs` | 7 `CloudEvent::new` call sites: replace `json!({...})` with `EventData::new(...)` |
| `rs/server/src/consumers/persist.rs` | 6 constructor call sites: update to current `PersistConsumer::new(store, session_store)` and `SessionStore::new(&Path) -> Result` signatures |
| `rs/tests/helpers/mod.rs` | Update shared test helpers to use typed constructors |
| `rs/tests/test_*.rs` (6 files) | Update `CloudEvent::new` calls to use `EventData::new(...)` |

## Data Model
No changes. The `EventData` type and `AgentPayload` enum already exist in `rs/core/`.

## API Contracts
No changes. This is internal test/fixture work only.

## Sequence Diagrams
N/A — no new flows.

## Non-Functional Requirements
- Latency: N/A
- Throughput: N/A
- Availability: N/A
- Security: N/A — no behavioral changes

## Technology Choices
| Choice | Option | Status |
|--------|--------|--------|
| No new technologies | Existing Rust types | user-approved |

## Constraints & Patterns
- Every fixture must wrap data in `AgentPayload::ClaudeCode` with `_variant: "claude-code"` and `meta.agent: "claude-code"`
- Use `EventData::new(raw, seq, session_id)` when constructing `CloudEvent::new` directly
- Reference `make_event_data` helper in `rs/views/src/from_cloud_event.rs` and `to_cloud_event` in `rs/store/src/ingest.rs` as canonical examples
- Pi-mono fixtures (if any) must use `AgentPayload::PiMono` variant
- Do not modify the `EventData` or `AgentPayload` types themselves — only update call sites
