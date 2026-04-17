# T6 — SQLite Round-Trip for Agent Payloads

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## The concern

`insert_event` serializes the full CloudEvent as a TEXT blob in the `events.payload` column. `session_events` reads the TEXT back and parses JSON. Generic fields (id, type, subtype, time) round-trip because they're primitives.

The risky part is `data.agent_payload`, an internally-tagged enum (`#[serde(tag = "_variant")]`). It must:

1. Serialize with `_variant` set correctly
2. Deserialize back into the same variant after coming out of SQLite
3. Preserve **all** typed fields (not just the ones touched by the test)

The existing conformance suite has `it_round_trips_an_event_payload_losslessly` but it tests a generic synthesized event — no agent-specific payload, no `_variant` check, no typed-field round-trip.

That's the gap: if someone adds a new `PiMonoPayload` field and forgets a `#[serde(default)]` attribute, or renames `_variant` targets, or tweaks the Hermes shape — nothing catches it.

## Test shape

One test per agent (ClaudeCode, PiMono, Hermes):
1. Build a CloudEvent with a fully-populated `AgentPayload::$Variant`
2. Insert via `SqliteStore::insert_event`
3. Read back via `session_events`
4. Deserialize the returned Value as `CloudEvent`
5. Assert `data.agent_payload` matches the original variant + fields

This catches:
- Missing `#[serde(default)]` on newly added Option fields
- Variant tag drift (someone changing `"pi-mono"` to `"pimono"`)
- Type width issues (u64 vs i64 on edge inputs — unlikely with SQLite TEXT, common with Mongo BSON)
- Silent field loss

## Where to add

Inline in `rs/store/src/sqlite_store.rs` tests module. The conformance suite lives at `rs/store/tests/event_store_conformance.rs` and runs against both SQLite and Mongo — the *right* long-term home is there, so the same guarantees apply to both backends. But (a) that requires writing agent-payload builders conformance-style and (b) Mongo uses BSON which has real type-width quirks worth a separate pass. Starting inline in SQLite is the smaller step.

Follow-up (backlog): promote these three tests to `event_store_conformance.rs` so Mongo inherits them.

## Exit criteria

- Three round-trip tests in `sqlite_store.rs` tests
- Each asserts variant + at least three typed fields
- BACKLOG entry to promote into the conformance suite
- T6 section updated with outcome

## Outcome (2026-04-14)

All three tests pass on current code. Nothing broken, contract locked in for all three agents:

- `t6_pi_mono_agent_payload_round_trips` — PiMono variant, tool + tool_call_id + args + parent_uuid survive
- `t6_claude_code_agent_payload_round_trips` — ClaudeCode variant, tool + args survive
- `t6_hermes_agent_payload_round_trips` — Hermes variant, tool + tool_use_id survive

**Surface rub:** the first run failed with `missing field specversion` — my fixtures didn't have the CloudEvent 1.0 envelope fields. Real stored events always do (they come from `CloudEvent::new()`), so this was test-fixture incompleteness, not a contract issue. Fixed.

**Audit value:** before this, no test verified that any agent-specific typed field round-trips through SQLite. The generic conformance test covered id/subtype/time/raw fragments — but a regression in, say, `PiMonoPayload.tool_call_id` serde tags would pass the conformance suite and break the UI silently. Now a rename or attribute drop fails here first.

**Bonus:** the tests double as worked examples of the right CloudEvent JSON shape for each agent. Useful for future fixture authors.