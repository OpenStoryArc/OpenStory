# Idea A — JetStream sources as a native federation transport

**Status:** prototype (branch `prototype/jetstream-sources-federation`)
**Origin:** session `c5428c80`, prompt *"why not have NATS be another durable store and enable replay?"* → *"I want A."* The fork was Idea A (durable **replicated** NATS for the federation layer) vs Idea B (NATS as the system of record — rejected on sovereignty grounds). This doc designs and measures Idea A.
**Compares against:** the shipped app-level catch-up anti-entropy (`024fcc2`, `rs/server/src/catch_up.rs`).

## The gap we're closing

The faithful lab (`rs/tests/test_federation_lab.rs`) asserts *every machine sees all team data*: hub **and** every leaf must mirror all N sessions. Today that bidirectional convergence rides on **NATS core interest-propagation** across the leafnode connection. Finding #9 confirmed it's a timing race: cold 10-node boots fail 4/4 (hub 10/10, slowest node 8–9/10), warm converges. Catch-up fixes it at the *application* layer (digest-diff + HTTP pull every 10s). Idea A asks whether the *transport* can close it natively, with no application-level reconciliation at all.

## Why core propagation races (and why JetStream sources don't)

A JetStream stream is **domain-local**. When `node-0` does a JetStream `publish` to `events.proj.node-0.main`, the message lands in `node-0`'s *local* `events` stream. The leafnode connection propagates **core NATS pub/sub** — it forwards a message to a peer only if that peer has *registered interest* at the moment the message flows. So convergence depends on the hub's (and every leaf's) `events.>` subscription interest having propagated across the leafnode mesh *before* a peer's burst. Lose the race on cold boot and those events are **permanently** absent from the late subscriber's stream — nothing backfills them. That is exactly the 8/10.

JetStream **sources** are pull-based and gap-filling: a stream configured to *source* from another stream maintains a cursor and pulls **all** messages from a starting sequence, retrying across restarts. There is no interest-timing window — a source that comes up late still pulls the full history. That's the property we want.

But sources operate on **streams across JetStream domains**, not on core subjects. So Idea A is really two changes:

1. **Give every NATS server a JetStream domain** (`hub`, `leaf-0`, `leaf-1`, …). Domains make a remote stream addressable as `{stream: "events", domain: "leaf-0"}` and route the JetStream API over the leafnode via `$JS.<domain>.API.>`.
2. **Wire the source graph** so the hub aggregates every leaf and every leaf re-derives the aggregate.

## The real design content: the enumeration problem

Sourcing is directional and must name a concrete remote stream+domain. Three ways to wire the graph:

### Option 1 — Hub enumerates leaves (rejected)
Hub's `events` stream lists `sources: [{stream: events, domain: leaf-0}, …, {domain: leaf-N}]`; each leaf sources back from `{stream: events, domain: hub}`. **Problem:** the hub must statically know every leaf domain at stream-creation time. Leaves join/leave dynamically (laptops, the lab grows). This recreates the very coupling federation is supposed to avoid, and it's a config-edit-per-node operational tax.

### Option 2 — Shared single domain at the hub (rejected)
Leaves run no local JetStream; everyone publishes/consumes against the hub's stream over the leafnode. Converges trivially but **kills local durability** — a leaf offline from the hub captures nothing locally, breaking the "complete local mirror with its own dashboard" promise (and brushing against the sovereignty line: your machine's record shouldn't require the hub to exist).

### Option 3 — Decentralized self-registration (chosen)
Each leaf knows two things at boot: **its own domain** and **the hub's domain** (one env var, `OPEN_STORY_HUB_DOMAIN`). On `ensure_streams`, a leaf:

- creates its **local** `events` stream (unchanged — local durability preserved);
- **sources from the hub aggregate**: `source { stream: "events-agg", domain: hub }` → pulls everyone else's events down, gap-free;
- **self-registers into the hub aggregate**: via the cross-domain JetStream API (`$JS.hub.API`), adds *itself* as a source on the hub's `events-agg` stream (`source { stream: "events", domain: leaf-i }`), idempotently.

The hub creates only an empty `events-agg` aggregate stream and never needs to know leaf identities — **each leaf registers itself**. Enumeration is distributed to the nodes that hold the knowledge. A leaf joining late simply registers and its source begins pulling; a leaf leaving leaves a dormant source (cheap; prunable later).

This mirrors how catch-up is *peer-generic* — no node hardcodes the fleet — but pushes the mechanism down to the transport so there's no 10s reconciliation cadence and no HTTP round-trip.

```
                 ┌─────────────────────────────┐
                 │  hub domain                  │
                 │  events-agg  ◄──source──┐    │
                 │     ▲   ▲   ▲           │    │
                 └─────┼───┼───┼───────────┼────┘
        source(agg)    │   │   │   self-register
        ▼              │   │   │   source(events@leaf-i)
   ┌────────┐    ┌────────┐   ┌────────┐
   │ leaf-0 │    │ leaf-1 │ … │ leaf-N │
   │ events │    │ events │   │ events │   (local, durable)
   └────────┘    └────────┘   └────────┘
```

Each leaf's local `events` consumer now sees: its own published events **+** everything pulled from `events-agg`. Bidirectional convergence becomes a property the broker guarantees, not a race the application patches.

## What stays untouched (sovereignty + solo mode)

- **Solo mode** (no `OPEN_STORY_HUB_DOMAIN`): no domains, no sources, identical to today. Idea A is dormant exactly like catch-up is dormant without `OPEN_STORY_CATCH_UP_PEER`.
- **JSONL stays the system of record.** This is Idea A, *not* Idea B. NATS becoming durable+replicated changes the **transport**, not the source of truth. JSONL remains the grep-able sovereign log; SQLite remains the query index; reconcile-on-boot still rebuilds from JSONL.
- **`max_bytes` retention** unchanged on the local stream; the aggregate gets its own budget. (A real follow-up: sources mean the aggregate can be shrunk since completeness no longer depends on its retention window — see the NATS-role analysis in session `c5428c80`.)

## The experiment

Add a lab variant that flips exactly one lever versus the existing test:

| Variant | core propagation | JetStream sources+domains | catch-up (`OPEN_STORY_CATCH_UP_PEER`) |
|---|---|---|---|
| baseline (existing, pre-catch-up) | yes | no | **off** → 8/10 |
| catch-up (shipped, `024fcc2`) | yes | no | **on** → 10/10 in 12.8s |
| **sources (this prototype)** | yes | **yes** | **off** |

If the sources variant reaches `fully_mirrored` 10/10 cold with catch-up **off**, Idea A closes the gap at the transport layer. Then we compare convergence time and operational weight against catch-up and decide what to keep (or keep both: sources as the fast path, catch-up as a topology-agnostic backstop for non-NATS peers).

### Metrics to capture
- cold-boot `fully_mirrored` (the 4/4-fail scenario)
- time-to-converge vs catch-up's 12.8s
- behavior of a **late-joining** leaf (start one node 30s after the rest)
- aggregate stream bytes vs sum of leaf streams (replication overhead)

## Validated findings (spike: `scripts/spike_jetstream_sources.sh`, NATS 2-alpine)

A Docker spike (hub + 2 leaves, JetStream domains over leafnodes) settled all three open risks. Final converged state, publishing 5 events to **leaf-0 only**, catch-up **off**:

```
leaf-0 events:5 mirror:0 | leaf-1 events:0 mirror:5 | hub agg:5
fleet view (local ∪ mirror) → leaf-0:5  leaf-1:5   ✅
```

1. **Cross-domain API reachability — CONFIRMED, no special export needed.** `jetstream { domain: … }` on each server plus a normal token-authed leafnode connection is sufficient; a leaf reaches `$JS.hub.API` and can create/edit hub-domain streams. So **Option 3 self-registration is viable** (a leaf can register itself as a source on the hub aggregate). The spike enumerates leaves at the hub for simplicity, but the reachability that self-registration needs is proven.

2. **Source loops — solved by topology, not by filters.** The split into a publish-only local `events` and a **source-only** `events-mirror` (no subjects → no publishers → structurally cannot loop) is the mechanism. JetStream additionally applies **self-origin loop prevention**: a leaf's mirror *excludes events that originated on that leaf*. So the complete picture on any node is `local events ∪ events-mirror` — **the OpenStory consumer must read both streams** (or read the hub aggregate directly when online). This is the one consumer-side change Idea A forces.

3. **NEW, load-bearing: per-node subject namespacing is mandatory.** First spike run double-counted (`hub agg:10` for 5 events) because the *old* core leafnode subject propagation still cross-pollinated every stream binding `events.>`. Fix: each leaf binds **only its own namespace** `events.<node>.>`. This both removes reliance on the racy core mechanism and stops double-counting. **Schema implication:** today's subject `events.{project}.{session}.main` has no node component; federation needs `events.{node}.{project}.{session}.main` (or equivalent). This is the biggest production change Idea A requires and must land before wiring `ensure_streams`.

- **Min NATS version:** validated on `nats:2-alpine` (the lab's pin) — no version bump needed.

## Next steps (implementation)

1. **Subject schema:** add a node component to event subjects (`events.{node}.…`). Gate on federation mode so solo subjects are unchanged.
2. **`ensure_streams` (federation mode):** local `events` bound to `events.{node}.>`; source-only `events-mirror` sourcing the hub aggregate; self-register into the hub aggregate via the hub-domain API.
3. **Consumer:** union `events` + `events-mirror` for the fleet view (the persist/broadcast consumers subscribe to both).
4. **Lab variant:** `lab_federation_jetstream_sources_10_nodes` — domains on, catch-up **off**; assert `fully_mirrored` cold. Compare convergence time + late-joiner behavior against catch-up's 12.8s.
