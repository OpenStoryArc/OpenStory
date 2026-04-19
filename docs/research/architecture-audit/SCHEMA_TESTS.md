# Schema Test Design — Stage 2

Paired with: [SCHEMA_MAP.md](./SCHEMA_MAP.md)

Stage 1 enumerated what gets a schema. Stage 2 describes the tests that will drive the schemas into existence. **No code or schemas yet — this doc is the RED phase plan.**

Stage 3 (next) executes this plan TDD-style: write the test, watch it fail (because schema file/infrastructure doesn't exist), make it pass, repeat per schema.

---

## Decisions locked (from Stage 1 open questions)

1. **Generator**: [`schemars`](https://graham.cool/schemars/) crate — derive `JsonSchema` on Rust types
2. **Narrowing**: start coarse (one schema per top-level type). Refine later only if validation proves too permissive in practice
3. **Location**: `/schemas/` at the repo root — language-neutral, grep-friendly, discoverable by external tools
4. **Test home**: new `rs/schemas/` crate — owns `build.rs`/generation binary + the schema tests. Keeps serde + schemars derive complexity out of `open-story-core`
5. **First target**: JSONL escape hatch (CloudEvent validation of `data/*.jsonl`) — most directly serves the sovereignty soul

---

## Global test infrastructure

Everything in Stage 3 is one of four test shapes. Define these once, reuse per schema.

### Shape A — Generation drift
**Property:** the committed schema file is identical to what regeneration produces from current Rust types.

```rust
#[test]
fn cloud_event_schema_is_up_to_date() {
    let expected = schema_for::<CloudEvent>();       // regenerate now
    let committed = load_schema_file("cloud_event.schema.json");
    assert_eq!(
        canonicalize(&expected), canonicalize(&committed),
        "schema drift — run `cargo run -p open-story-schemas --bin generate` and commit"
    );
}
```

`canonicalize` = sort map keys, normalize whitespace — so formatting changes in the committed file don't spuriously fail.

**Red criteria:** fails until Stage 3 lands (no schema files committed, no generator binary).

### Shape B — Known-good fixture validates
**Property:** a sample JSON object of this type passes schema validation.

```rust
#[test]
fn cloud_event_schema_accepts_real_pi_mono_event() {
    let fixture = load_fixture("pi_mono/real_event.json");
    let schema = load_schema("cloud_event.schema.json");
    let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
    assert!(
        validator.is_valid(&fixture),
        "real pi-mono event must validate against committed schema"
    );
}
```

Fixtures come from existing `rs/tests/fixtures/pi_mono/scenario_*.jsonl` — extract one line each.

**Red criteria:** fails until schema exists.

### Shape C — Known-bad fixture rejects
**Property:** an intentionally malformed fixture fails validation, and the error message points at the missing/wrong field.

```rust
#[test]
fn cloud_event_schema_rejects_missing_specversion() {
    let bad = json!({ /* no specversion */ "type": "io.arc.event", ... });
    let schema = load_schema("cloud_event.schema.json");
    let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
    let errors: Vec<_> = validator.validate(&bad).unwrap_err().collect();
    assert!(
        errors.iter().any(|e| e.instance_path.to_string().contains("specversion")
            || e.schema_path.to_string().contains("specversion")),
        "error should indicate missing specversion"
    );
}
```

Each schema gets three known-bad tests covering the three most important violations (missing required field, wrong type, unknown enum variant).

**Red criteria:** fails until schema exists.

### Shape D — Rust ↔ JSON round-trip via schema
**Property:** `Rust value → serde_json → validates → deserialize → equals input`.

```rust
#[test]
fn cloud_event_round_trips_through_schema_validation() {
    let original = make_test_event();
    let json = serde_json::to_value(&original).unwrap();

    let validator = load_validator("cloud_event.schema.json");
    assert!(validator.is_valid(&json), "serialized value must validate");

    let recovered: CloudEvent = serde_json::from_value(json).unwrap();
    assert_eq!(/* field-by-field */);
}
```

This closes the loop: the schema isn't just descriptive — it's consistent with what serde produces. Catches the subtle case where `schemars` and `serde` disagree (e.g., default handling, skip_serializing_if).

**Red criteria:** fails until schema exists and matches serde output exactly.

---

## Shared fixture strategy

- **Good fixtures** live at `rs/schemas/fixtures/good/{boundary}/{scenario}.json`
- **Bad fixtures** live at `rs/schemas/fixtures/bad/{boundary}/{violation}.json` — each with a matching `{violation}.expected.txt` naming the field/path the error must mention
- **Real fixtures** derived from `rs/tests/fixtures/pi_mono/*.jsonl` and `rs/tests/fixtures/hermes/*.jsonl` via a one-time extraction script (`rs/schemas/scripts/extract_fixtures.sh`). Re-runnable but committed.

---

## Per-schema test matrix

Eight schemas × four shapes = baseline 32 tests. Some boundaries (WireRecord) get extra coverage because flatten composition has historically bit us.

### S1. `cloud_event.schema.json`

| Shape | Test |
|-------|------|
| A drift | `cloud_event_schema_is_up_to_date` |
| B good | `cloud_event_accepts_claude_code_text_event` |
| B good | `cloud_event_accepts_pi_mono_tool_use_event` (from scenario_07) |
| B good | `cloud_event_accepts_hermes_delegated_event` |
| B good | `cloud_event_accepts_event_with_null_agent_payload` (raw-only, translator failed to type) |
| C bad | `cloud_event_rejects_missing_specversion` |
| C bad | `cloud_event_rejects_missing_id` |
| C bad | `cloud_event_rejects_agent_payload_variant_typo` (`_variant: "pimono"` vs `"pi-mono"`) |
| C bad | `cloud_event_rejects_unknown_subtype` — *only if SC-4 subtype enum narrowing lands* |
| D round-trip | `cloud_event_round_trips_each_agent_variant` (parameterized over 3 agents) |

**Notable:** the `null agent_payload` good-fixture is critical. `agent_payload: None` is a legitimate state (translator hit an unknown entry) and the schema must allow it.

### S2. `view_record.schema.json`

| Shape | Test |
|-------|------|
| A drift | `view_record_schema_is_up_to_date` |
| B good | `view_record_accepts_each_body_variant` — parameterized over all 13 `RecordBody` variants |
| B good | `view_record_accepts_message_content_as_string` (`MessageContent::Text` untagged path) |
| B good | `view_record_accepts_message_content_as_blocks` (`MessageContent::Blocks` untagged path) |
| C bad | `view_record_rejects_unknown_record_type` |
| C bad | `view_record_rejects_content_block_with_unknown_type` |
| C bad | `view_record_rejects_token_scope_outside_enum` |
| D round-trip | `view_record_round_trips_each_body_variant` |

**SC-2 call-out:** the untagged `MessageContent` is the highest-risk spot — two good-fixture tests lock in that both arms of the union validate.

### S3. `wire_record.schema.json`

| Shape | Test |
|-------|------|
| A drift | `wire_record_schema_is_up_to_date` |
| B good | `wire_record_accepts_flattened_view_record_plus_tree_meta` |
| B good | `wire_record_accepts_root_with_null_parent_uuid` |
| B good | `wire_record_accepts_truncated_tool_result` (`truncated: true`, `payload_bytes > TRUNCATION_THRESHOLD`) |
| C bad | `wire_record_rejects_negative_depth` (u16) |
| C bad | `wire_record_rejects_depth_above_u16_max` |
| C bad | `wire_record_rejects_missing_record_type` (proves the flatten actually propagates the tag to the schema) |
| D round-trip | `wire_record_round_trips_with_nested_body` |

**SC-1 call-out:** `wire_record_rejects_missing_record_type` is the litmus test that flatten + tagged composition survived generation. If this test fails after Stage 3, the schema is subtly wrong even if Shape B passes.

### S4. `broadcast_message.schema.json`

| Shape | Test |
|-------|------|
| A drift | `broadcast_message_schema_is_up_to_date` |
| B good | `broadcast_accepts_view_records_kind` |
| B good | `broadcast_accepts_enriched_kind_full` (all optional fields present) |
| B good | `broadcast_accepts_enriched_kind_minimal` (all optionals omitted) |
| B good | `broadcast_accepts_plan_saved_kind` |
| C bad | `broadcast_rejects_missing_kind_tag` |
| C bad | `broadcast_rejects_unknown_kind` |
| C bad | `broadcast_rejects_enriched_without_session_id` |
| D round-trip | `broadcast_round_trips_each_kind` |

### S5. `ingest_batch.schema.json`

Lightweight wrapper sent over NATS. Must contain valid CloudEvents.

| Shape | Test |
|-------|------|
| A drift | `ingest_batch_schema_is_up_to_date` |
| B good | `ingest_batch_accepts_multi_event_batch` |
| B good | `ingest_batch_accepts_single_event_batch` |
| B good | `ingest_batch_accepts_empty_events_array` |
| C bad | `ingest_batch_rejects_batch_with_malformed_cloud_event` (cross-schema — batch schema `$ref`s CloudEvent) |
| C bad | `ingest_batch_rejects_missing_session_id` |
| D round-trip | `ingest_batch_round_trips` |

**Note:** this is where `$ref` between schemas first appears. Stage 3 must verify that `jsonschema` crate resolves `$ref: "cloud_event.schema.json#"` correctly.

### S6. `pattern_event.schema.json`

| Shape | Test |
|-------|------|
| A drift | `pattern_event_schema_is_up_to_date` |
| B good | `pattern_event_accepts_each_detected_pattern_type` |
| C bad | `pattern_event_rejects_missing_session_id` |
| D round-trip | `pattern_event_round_trips` |

Thinner coverage than the others — PatternEvent is younger and less risk-prone.

### S7. `session_row.schema.json` + `fts_result.schema.json`

Both are thin record types for REST list endpoints.

Each gets: A drift + B good + C bad (missing required) + D round-trip. 4 tests per schema, 8 total.

### S8. `subtypes.schema.json`

This one is different — it's not a struct, it's a string enum. Schema is:

```json
{ "type": "string", "enum": ["message.user.prompt", "message.user.tool_result", ...] }
```

| Shape | Test |
|-------|------|
| A drift | `subtypes_enum_is_up_to_date` (parse `rs/core/src/` for known subtype string literals, compare to schema) |
| B good | `subtypes_accepts_every_live_subtype` — pull from live events in a real session fixture |
| C bad | `subtypes_rejects_typo` (`message.assitant.text`) |

**Drift detection here is different from A-shape elsewhere.** We're not generating this schema from a Rust type (no `JsonSchema` for an anonymous enum of strings). Options:
- Define a `Subtype` Rust enum with serde rename for each variant, derive `JsonSchema`
- Maintain the list in `rs/core/src/subtypes.rs` as `const` and write the schema by hand, with the drift test asserting the const matches the file

**Recommendation:** use the `Subtype` enum. It's more work up-front (adding a typed enum where today there are string literals), but it eliminates drift by construction and forces every translator/view-layer string comparison to go through the enum. This is a sovereignty-relevant improvement: the current sprinkle of `"message.assistant.tool_use"` literals across 40+ files is a latent bug farm.

Flag: this is potentially big refactor work. Tractable, but the largest implementation risk in Stage 3. **Propose:** land the schema with the hand-maintained `const` path for Stage 3, queue the `Subtype` enum refactor as a separate Stage 3.5 on its own branch.

---

## Cross-stack tests (Stage 4, not 3, but planned now)

Once Rust schemas are committed, the UI generates TS types from them and adds its own validation tests. Planned shape:

- `ui/src/generated/*.ts` — generated from `/schemas/*.schema.json` via [`json-schema-to-typescript`](https://github.com/bcherny/json-schema-to-typescript) at build time
- `ui/tests/schema/*.test.ts` — for each schema, a test asserting that a real captured WebSocket frame (recorded from a dev session) parses into the generated TS type without manual casts
- `ui/scripts/capture-ws-frame.ts` — helper that subscribes to a running dev server and saves one frame per kind to `ui/tests/fixtures/`

**Stage 4 exit criteria:** UI no longer hand-maintains `ui/src/types/cloud-event.ts`, `view-record.ts`, `websocket.ts` — they're all generated.

---

## E2E validation middleware (Stage 5)

Stage 5 lands a testcontainers helper that wraps the existing pipeline tests with schema validation at every boundary. Planned shape:

```rust
// rs/tests/audit/schema_validated_harness.rs
let server = start_open_story_with_schema_validation(&fixture_dir).await;
// Every NATS message published → validated against ingest_batch.schema.json
// Every WS frame delivered → validated against broadcast_message.schema.json
// Every /records response → validated against wire_record.schema.json
// Any failure = test failure with exact boundary + offending JSON in the message
```

This gives us T2 (multi-tool), T3 (NATS subjects), T6 (SQLite round-trip) for free, plus everything we haven't covered — because the validation is universal instead of per-test.

**Stage 5 is the payoff.** Stages 1–4 set the stage; Stage 5 is when the schema registry starts paying rent on every test run.

---

## Implementation order for Stage 3

TDD means red before green. To avoid one giant 32-test RED phase, we cycle small:

1. **Scaffolding cycle** (1 commit):
   - Create `rs/schemas/` crate
   - Add `schemars`, `jsonschema` dependencies
   - Write Shape A drift test for CloudEvent → fails (no schema file)
   - Add generation binary stub → still fails (no derive)
   - Derive `JsonSchema` on `CloudEvent`, `EventData`, `AgentPayload`, `PayloadMeta`, `ToolOutcome` → generator writes file → drift test passes
2. **CloudEvent cycle** (1 commit per shape):
   - Shape B tests → write fixtures → pass
   - Shape C tests → write bad fixtures → pass
   - Shape D round-trip → pass
3. **Repeat per schema** in the order: CloudEvent, ViewRecord, WireRecord, BroadcastMessage, IngestBatch, PatternEvent, SessionRow/FtsResult, Subtypes
4. **JSONL escape-hatch validation test** at the end of Stage 3 — the soul-relevant capstone: `validate_session_jsonl("data/sess-xxx.jsonl")` returns `Ok` on every line for every fixture session

Each schema completes in ~4 tests; cycling small keeps the RED phase bounded.

---

## What success looks like at the end of Stage 3

- `schemas/` directory with 8 schema files
- `rs/schemas/` crate with generator binary + ~32 tests, all green
- CI runs `cargo run -p open-story-schemas --bin generate && git diff --exit-code schemas/` — fails on drift
- An external tool (jq, ajv, vscode JSON extension) can point at one of our schemas and get autocompletion/validation for free
- The JSONL escape-hatch test proves a real committed session file validates every line against the schema — sovereignty contract is now executable

---

## What's still out of scope after Stage 3

- UI TS generation (Stage 4)
- E2E validation harness (Stage 5)
- Subtype enum refactor (Stage 3.5, separate branch)
- Schema versioning / `dataschema` wiring (Stage 6, if/when breaking changes happen)
- Per-subtype narrowed schemas (Stage 7, only if coarse proves too permissive)

---

## Review questions (none blocking)

Only one I'd want your opinion on before Stage 3 starts:

> **On the Subtype question in S8:** do I take the hand-maintained `const` shortcut for Stage 3 and queue the enum refactor separately, or is the refactor small enough that we'd rather do it in-flight?

I'd recommend the shortcut + separate branch. But you know the codebase's tolerance for refactor-while-adding-features better than I do.
