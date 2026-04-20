# T5 — Wire Record ↔ Projection Sync

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## The concern

`to_wire_record(vr, projection)` uses `projection.node_depth(vr.id)` and `projection.node_parent(vr.id)` to enrich the wire record. Both fall back to `0` / `None` if the event hasn't been projected yet. A wire record delivered before its own CloudEvent has been projected silently carries stale (zero) tree metadata.

## Recon — what's actually live

The actor-consumer architecture is documented in CLAUDE.md as four independent tokio tasks on `events.>`. In practice, the wiring at `rs/src/server/mod.rs:240-270` reveals:

```
Actor 1 (persist)      — fully independent, owns EventStore
Actor 2 (patterns)     — fully independent, owns PatternPipeline
Actor 3 (projections)  — independent, updates its OWN projection copy
Actor 4 (broadcast)    — still uses ingest_events() on shared AppState
                         (comment: "This is the last consumer to decompose")
```

Actor 4's `ingest_events()` at `rs/server/src/ingest.rs:136-253` does both steps in one synchronous loop iteration per event:

```rust
let proj = state.store.projections.entry(...);
let append_result = proj.append(&val);        // line 136
// ... builds view_records ...
to_wire_record(vr, proj)                       // line 253
```

**→ No race today.** The same `proj` reference sees its own append before the wire record is built.

## The drift that looks like a race but isn't

Actor 3 (`ProjectionsConsumer`) *also* subscribes to `events.>` and updates its own in-memory `SessionProjection`. Nothing reads that projection — the wire-record construction goes through Actor 4's copy (owned by `state.store.projections`). Actor 3 is doing duplicated work that never surfaces.

This is worth flagging:
- When someone decomposes Actor 4 (stated goal — comment at `mod.rs:242`), the natural swap is "broadcast reads Actor 3's projection." The moment that swap happens, the race opens: Actor 3 and Actor 4 run as independent subscribers, so Actor 4 can fire `to_wire_record` against an Actor 3 projection that hasn't yet processed the event.
- Until then: dead code.

## Audit shape

Characterization test: enforce the current invariant — when `to_wire_record` is called on a ViewRecord, the projection handed in must already contain that event. Test passes today; if someone ever splits `append` and `to_wire_record` across actors, the test becomes the right place to explain why the new wiring must include a barrier.

```rust
#[test]
fn to_wire_record_sees_the_event_it_describes() {
    let mut proj = SessionProjection::new("sess-t5");
    let event = make_event_with_parent("e1", Some("e0"));
    let vr = make_view_record("e1");

    // If the caller forgets to append before building the wire record,
    // depth is 0 and parent_uuid is None — silent data loss.
    proj.append(&event);
    let wire = to_wire_record(&vr, &proj);

    assert!(wire.parent_uuid.is_some() || proj.node_parent("e1").is_none(),
        "wire.parent_uuid must reflect projection state AFTER this event was appended");
}
```

Plus a second test documenting the reverse failure mode (forgetting the append):

```rust
#[test]
fn to_wire_record_without_projection_append_produces_zero_depth() {
    let proj = SessionProjection::new("sess-t5");  // empty
    let vr = make_view_record("unprojected-id");
    let wire = to_wire_record(&vr, &proj);

    assert_eq!(wire.depth, 0, "depth is 0 when event isn't in projection");
    assert_eq!(wire.parent_uuid, None,
        "parent_uuid is None when event isn't in projection — this is the \
         failure mode to guard against when decomposing broadcast from projections");
}
```

## Proposal for the "when we decompose Actor 4" day

One of three shapes, in order of preference:

1. **Single projection, read-locked** — Actor 3 writes, Actor 4 reads. A `tokio::sync::RwLock` around the projection. Actor 4 blocks until Actor 3 processes the same batch. Simplest, safe, slight latency coupling.
2. **Per-batch barrier via NATS sequence** — Actor 4 watches the projection's last-processed NATS sequence and waits until Actor 3 catches up. No shared state, more moving parts.
3. **Actor 4 computes its own projection from scratch** — stateless, no sync concern. Cost: projections compute twice per event. Same cost as today's dead-code Actor 3, just with one consumer instead of two.

Option 3 is the honest version of what's already happening — maintains invariant by construction. But premature given Actor 4 hasn't been decomposed yet.

## Exit criteria

- Characterization tests inline in `rs/store/src/ingest.rs` (next to `to_wire_record`)
- BACKLOG entry noting the decomposition race when Actor 4 is split
- T5 section updated with outcome

## Outcome (2026-04-14)

Two tests added, both pass:

1. `t5_wire_record_without_projection_append_has_zero_depth` — characterizes the failure mode (silent data loss as depth=0 / parent_uuid=None) for the day the two steps cross an async boundary
2. `t5_wire_record_reflects_projection_state_when_parent_present` — shows both the "wrong order" and "right order" side by side in a single narrative test

**Most valuable find:** Actor 3 (`ProjectionsConsumer`) is dead code today. It subscribes to `events.>` and updates its own projection that nothing reads — Actor 4 still uses the shared `state.store.projections` updated inside `ingest_events`. Worth calling out in the backlog so whoever decomposes Actor 4 doesn't reuse the dead copy without first solving the race.

**Audit value:** the invariant is now executable. When the architectural decomposition happens (it will), these tests are the breadcrumb trail back to "why does this need a barrier." Also: surfacing Actor 3 as dead-but-wired is exactly the kind of ambient cost a boundary audit should catch.