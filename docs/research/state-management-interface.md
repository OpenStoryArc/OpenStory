# State-Management Interface — reproject / verify / catch-up / prune

**Status:** design + first prototype (`reproject`). Motivated by the
architecture review on `test/arch-review-perf-hardening`.

## Why

OpenStory's state management is currently **implicit and boot-coupled**.
Several operations that, in mature event-sourced / distributed-log systems,
are *named, explicit, idempotent commands* happen here only as side effects of
boot or as hopes about the file watcher:

- **Dedup** happens at two layers (EventStore PK + projection `seen_ids`) but
  has no operation and no cross-device story.
- **Reconcile** (JSONL backup → SQLite) runs *only at boot* (`reconcile_local`).
- **Projection rebuild** happens *only* when the watcher re-reads a source
  file — never from the durable store. Confirmed consequence
  (`state.rs::tests::boot_does_not_rebuild_the_in_memory_projection`): after a
  restart, a session whose source is gone or beyond `boot_window` (pi-mono
  `--no-session`, old sessions) is durably present in SQLite yet shows
  `total_input_tokens: 0` in `/api/sessions`, because that field is served from
  the in-memory projection (`api.rs`), which boot never rehydrates. A
  stored-vs-live divergence — the exact merge hazard `CLAUDE.md` warns against.
- **Catch-up** across devices happens only via NATS JetStream's automatic
  bidirectional replication; there is no way to *ask* a device to reconcile
  with a peer, and no way to *verify* two devices agree.

Each of these is the same shape: a state operation that is a by-product rather
than a command you can run, observe, and test. The fix is to make them
**first-class operations** over the existing seams.

## Principles (why this fits OpenStory's soul)

1. **Idempotent by construction.** Events carry stable UUIDs, so every
   operation is set-union / set-membership, never append-or-double-count. Run
   any of these twice → same result.
2. **Observe, never interfere.** Every operation acts only on OpenStory's *own*
   store and projections. None touch the agent, its files, or its behavior.
3. **The durable log is the source of truth; projections are derived.** This is
   plain event-sourcing: the EventStore is the log, `SessionProjection` is a
   read model, and a read model is always rebuildable from the log.

## The operations

| Verb | What it does | Seam it uses | Distributed? | Status |
|------|--------------|--------------|--------------|--------|
| **reproject** | Rebuild in-memory projections (read model) from the durable EventStore | `EventStore::list_sessions` + `session_events` → `SessionProjection::append` | no (local) | **prototype** |
| **verify** | Diff the event-id set (or a Merkle summary) between this store and a peer/hub | `EventStore` event-id enumeration | yes | designed |
| **catch-up** | Pull the event-ids a peer/hub has that this device lacks | `Bus::replay` / an API range request | yes | designed |
| **prune** | Apply retention / eviction (age, size) | `EventStore::cleanup_old_sessions` + stream limits | local | exists (boot-time `retention_days`); promote to command |

### reproject (local, prototyped)

For each session in the store, load its events and fold them through a fresh
`SessionProjection::append` (which itself dedups by id), then publish the
rebuilt projection. Wired into boot after `boot_from_sqlite` so projections
reflect the durable store even when no source file can be re-read — closing the
token-0 divergence. The same function backs a future `open-story reproject`
command / `POST /api/admin/reproject` endpoint.

Idempotency: re-running is safe; `append`'s `seen_ids` makes a second fold a
no-op, and the watcher's later re-read dedups against the rebuilt `seen_ids`.

### verify (designed)

"Do two devices agree?" Naively: compare the full set of event-ids — O(n)
transfer. Efficiently (the standard answer): a **Merkle tree / range-hash** over
event-ids so divergence is found in O(log n) and only the differing ranges are
exchanged. Output: the id ranges each side is missing → the input to `catch-up`.
The leaf-cluster integrity test
(`leaf_to_hub_preserves_identity_order_and_origin`) is the manual, single-shot
version of this.

### catch-up (designed)

Given a `verify` diff, request the missing events from a peer/hub and insert
them (PK-deduped). Mechanically this is `Bus::replay` of a subject range, or an
API endpoint returning events by id/sequence. This is the genuine cross-device
piece and the foundation of multi-machine sovereignty (the lab).

## Prior art — a solved problem class

This is not novel; it sits at the intersection of three traditions, and the
mapping to OpenStory is direct because events are content-identified.

1. **Event sourcing / CQRS — projection rebuild.** Read models are rebuilt from
   the event log as a first-class action. EventStoreDB (catch-up subscriptions,
   projection reset), Axon (replay + tracking-token reset), Kafka Streams (state
   stores restored from changelog topics; `kafka-streams-application-reset`).
   `reproject` is exactly this.
2. **Distributed-DB anti-entropy — Merkle diff.** Dynamo, Cassandra, Riak use
   **Merkle trees** to find which key ranges diverge between replicas in
   O(log n), then repair only those (read-repair, hinted handoff,
   `nodetool repair`). This is `verify` + `catch-up`.
3. **Content-addressed sync.** The closest mental model, since events have
   stable UUIDs:
   - **Git** — `fetch` (catch-up), `gc` (prune), `fsck` (verify); immutable
     objects keyed by hash, dedup by identity. A device "fetches" missing
     event-ids from a peer.
   - **CouchDB / PouchDB replication** — `_changes` feed (catch-up by sequence),
     `_compact` (prune), revision trees; multi-device sync by design.
   - **CRDTs (Automerge, Yjs), Syncthing** — merge-by-construction.

## Surface

CLI subcommands plus matching API endpoints, all over the existing
`EventStore` trait + `Bus::replay`:

```
open-story reproject [--session <id>]      POST /api/admin/reproject
open-story verify --against <hub-url>      GET  /api/admin/verify?against=...
open-story catch-up --from <hub-url>       POST /api/admin/catch-up
open-story prune [--older-than <days>]     POST /api/admin/prune
```

Admin endpoints sit behind the existing bearer-token auth (and should *require*
it — see the security-hardening backlog).

## Sequencing (inside-out by distributed complexity)

1. **reproject** — purely local, fixes a confirmed bug, zero distributed risk. *(prototype landed)*
2. **verify** — event-id/Merkle diff vs hub; turns integrity into an operation.
3. **catch-up** — pull missing ids; the real cross-device piece, on `Bus::replay`.
4. **prune** — promote boot-time retention to an on-demand command.

## Related

- `[[perf-ingest-serial-ceiling]]`, `[[pi-mono-capture-asymmetry]]`,
  `[[arch-review-perf-hardening-branch]]` (memory).
- BACKLOG: `/api/health` (the read side of system state; this is the action side).
