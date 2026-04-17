# Runtime Schema Validation — Plan

## The idea

Use the committed envelope schema at runtime to classify events at
the enrichment boundary. Events the full enrichment path can't handle
get validated against the envelope schema to decide: passthrough (Tier B)
or truly broken (Tier C). The schema is the classification decision, not
hardcoded match arms.

## Where to validate

**At `from_cloud_event` — the BFF transform.**

Not at the translator (too early — event isn't fully formed). Not at
NATS (would mean tagging events, violating sovereignty). Not at every
consumer (redundant). The BFF transform is where "how rich is the
output?" gets decided, which is exactly the classification question.

## Design: fast path + schema fallback

```rust
pub fn from_cloud_event(event: &CloudEvent) -> Vec<ViewRecord> {
    // ── Fast path: known subtypes via string match (O(1)) ──
    // The 15 match arms we have today. They fire for ~99% of events
    // because we know Claude Code + pi-mono + Hermes shapes. No
    // schema overhead on the hot path.
    let records = enrich_known_subtypes(event);
    if !records.is_empty() {
        return records;
    }

    // ── Slow path: unknown subtype → envelope schema check ──
    // The event doesn't match any known subtype. Is it at least a
    // valid CloudEvent envelope (id + type + time + data.raw)?
    // If yes: passthrough as SystemEvent. If no: truly broken.
    let json = serde_json::to_value(event).unwrap_or_default();
    if envelope_schema().is_valid(&json) {
        return vec![passthrough_record(event)];
    }

    // ── Tier C: fails envelope — truly broken ──
    log_event("views", &format!(
        "⚠ event {} fails envelope validation — dropping",
        event.id
    ));
    metrics::counter!("events_envelope_invalid").increment(1);
    vec![]
}
```

**Key insight**: the full schema (`cloud_event.schema.json`) is NOT
used at runtime. It stays a test-time artifact. Only the envelope
schema (33 lines, trivial to validate) runs in production. The full
schema's job is CI drift detection and dogfood validation — not
per-event classification.

## Schema loading strategy

Compile the envelope schema once, cache forever:

```rust
use std::sync::OnceLock;
use jsonschema::Validator;

static ENVELOPE: OnceLock<Validator> = OnceLock::new();

fn envelope_schema() -> &'static Validator {
    ENVELOPE.get_or_init(|| {
        let schema = include_str!("../../../schemas/cloud_event_envelope.schema.json");
        let json: serde_json::Value = serde_json::from_str(schema).expect("parse envelope schema");
        jsonschema::validator_for(&json).expect("compile envelope schema")
    })
}
```

`include_str!` embeds the schema at compile time — no filesystem
access at runtime, no schema-file-not-found failure mode. The schema
is literally baked into the binary.

`OnceLock` means the validator compiles once on first use, then every
subsequent call is a pointer deref. Cost: ~0 after first event.

## Performance implications

| Operation | Cost per event |
|-----------|---------------|
| Current string match | ~50ns (subtype comparison) |
| Envelope validation | ~2-5μs (JSON Schema check against 5-field schema) |
| Full schema validation | ~50-200μs (785-line schema with nested types) |

The design ensures the envelope validation (slow path) only runs on
**unknown** subtypes — events that fell through all 15 match arms.
For a normal session (99%+ known subtypes), the schema never fires.
It's insurance, not a tax.

For a session with a brand-new agent producing 100% unknown subtypes,
every event pays ~5μs. At 1000 events/second that's 5ms/s — 0.5%
overhead. Acceptable.

**Recommendation**: measure before committing by running the schema
dogfood test with timing. If 6,344 envelope validations take more
than 100ms total, something's wrong with the schema validator setup
and worth investigating before shipping.

## Changes required

### Production code (2 files)

**`rs/views/src/from_cloud_event.rs`**
- Add `jsonschema` as a dependency of `open-story-views`
- Add `envelope_schema()` function with `OnceLock` + `include_str!`
- Refactor the match dispatch:
  - Extract current match arms into `fn enrich_known_subtypes(event) -> Vec<ViewRecord>`
  - Add the schema fallback after it (as shown above)
  - The existing fuzzy-pipe wildcard (`_ => { SystemEvent }`) becomes the
    `passthrough_record` function, called when envelope validates
- Add Tier C logging + metric counter for envelope failures

**`rs/views/Cargo.toml`**
- Add `jsonschema = "0.28"` as a dependency (currently dev-only in schemas crate)

### Test code (1 file, + existing tests stay)

**`rs/views/tests/test_runtime_validation.rs`** (new)
- Test that `from_cloud_event` produces a record for an unknown-but-valid-envelope event
  (mirrors `test_fuzzy_pipe.rs::totally_unknown_prefix_still_produces_records`)
- Test that `from_cloud_event` returns empty + logs for an envelope-invalid event
  (new — currently untestable because the fuzzy pipe always produces a record)
- Test that known subtypes DON'T trigger schema validation (fast path)
  — can't directly observe this without instrumentation, but can verify by
  ensuring the schema validator isn't even compiled if all events are known subtypes

### Schema file

Already committed: `schemas/cloud_event_envelope.schema.json`. No changes needed.
The `include_str!` path references it at compile time.

## What this enables downstream

### Today (this plan)
- Runtime envelope classification at the BFF transform
- Unknown subtypes → validated passthrough (not hardcoded wildcard)
- Invalid envelopes → logged + counted (not silently dropped)
- Known subtypes → unchanged fast path (no schema overhead)

### Future (not this plan)
- **Per-subtype enrichment handlers as a registry**: instead of match arms,
  register handler functions keyed by subtype. Unknown subtypes fall through
  to the schema-classified passthrough. Adding a new subtype = registering
  a handler, not editing a match statement.
- **External producer validation**: an API endpoint that accepts CloudEvents
  from external tools can validate against the envelope schema before
  persisting. The schema IS the API contract.
- **Schema evolution signals**: if a committed schema changes shape (new
  required field, removed variant), runtime validation failures surface
  the drift immediately — not after a user reports missing data.

## Execution order

1. Add `jsonschema` to views Cargo.toml
2. Add `envelope_schema()` with `OnceLock` + `include_str!`
3. Measure: validate 6,344 events against the envelope, time it
4. Refactor `from_cloud_event` to use fast-path + schema-fallback
5. Test that unknown subtypes pass through via schema (not wildcard)
6. Test that broken envelopes get logged + counted
7. Run full test suite — existing fuzzy-pipe tests should still pass
8. Dogfood against running instance — every event still produces records

## Risk assessment

**Low risk.** The behavioral change is minimal — the fuzzy-pipe wildcard
already produces a SystemEvent for unknown subtypes. The schema
validation just replaces the implicit "everything falls through to the
wildcard" with an explicit "does this meet the envelope contract?"

The only new failure mode: an event that the current wildcard would pass
but the envelope schema would reject. Looking at the envelope schema's
requirements (`id` non-empty string, `type` string, `time` string,
`data.raw` object), the only events that fail this are ones missing `id`
or `data.raw` — which are genuinely broken and should NOT flow through.

**The schema is LESS permissive than the wildcard** — and that's correct.
The wildcard says "anything goes." The schema says "at least identify
yourself and carry your raw data." That's the sovereignty floor.

## Decision point

One question worth deciding before implementing:

> Should envelope validation failures produce a record or not?

Option A: **No record** (Tier C = truly broken, silent drop + log).
The pipeline has a floor: below the envelope, nothing flows.

Option B: **Minimal record even for envelope failures** — a SystemEvent
with `subtype: "envelope_validation_failure"` and the raw bytes as the
message. Maximally permissive: even broken events are visible.

My recommendation: **Option A.** The envelope's required fields (id, type,
time, data.raw) are the absolute minimum for an event to be meaningful.
Without an `id`, dedup can't work. Without `data.raw`, sovereignty is
already lost. Without `time`, ordering is undefined. An event missing
these isn't "an event we can't enrich" — it's "not an event." Logging it
is sufficient; rendering it would produce garbage.

If you want Option B, the implementation is trivial — the Tier C arm
returns a SystemEvent instead of `vec![]`. I'd want the metric counter
either way so you can watch the rate.
