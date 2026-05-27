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

3. **NEW, load-bearing: per-node subject namespacing is mandatory.** First spike run double-counted (`hub agg:10` for 5 events) because the *old* core leafnode subject propagation still cross-pollinated every stream binding `events.>`. Fix: each leaf binds **only its own namespace** `events.<node>.>`. This both removes reliance on the racy core mechanism and stops double-counting. **Schema implication:** today's subject `events.{project}.{session}.main` has no node component; federation needs `events.{host}.{project}.{session}.main`. The `<node>` token **is** the existing `host` primitive (`rs/core/src/host.rs`) — already resolved, cached, and normalized to be *"safe to compose into a NATS subject token"* (its docstring literally anticipates this). So this is a small change (thread `host::host()` into `nats_subject_from_path`), not a new identity invention, and it must land before wiring `ensure_streams`. **Blast radius confirmed low:** nothing parses the subject positionally — routing is wildcard (`events.>`) and `project`/`session` derive from the file path, not the subject — so inserting `{host}` leaves existing subscriptions and consumers intact.

- **Min NATS version:** validated on `nats:2-alpine` (the lab's pin) — no version bump needed.

## Layered identity: federation → permissions → personhood

Federation is not a standalone feature — it is the **first layer of the identity model** the project has been quietly building toward (`docs/research/personhood-and-principals.md`, `docs/research/nats-permissions-spike.md`). All four layers express themselves through the **same NATS mechanism**: the subject string + the account boundary. The subject is the universal coordinate — routing key (core), stream/source filter (JetStream), and permission unit (security) at once. Designing the subject for federation alone would force re-migrating the most load-bearing string in the system three more times. So we design it once, with all four layers in view.

### The codebase already encoded the intent

Two orthogonal, subject-safe identity primitives exist, both stamped on the payload today, both waiting to become subject tokens:

- **`host`** (`rs/core/src/host.rs`) — *"which machine produced this event?"*
- **`user`** (`rs/core/src/user.rs`) — *"which human did the work?"*

Both normalize identically and document themselves as *"safe to compose into a NATS subject token."* They are the building blocks of the layers below.

### The four layers (each rides subjects + sources + accounts)

| Layer | NATS primitive | Identity primitive | Sovereignty meaning |
|---|---|---|---|
| **1. Federation / routing** | subject token + JetStream source filter | `host` in subject | which device, where events flow |
| **2. Person isolation** | **account** (JetStream is per-account; cross-account only via export/import) | person = account | hard isolation; sharing is deliberate |
| **3. Permissions / roles** | pub/sub allow-deny over subject patterns; users in account | principal = user-in-account | "persons own, principals act" |
| **4. Edge sovereignty** | `filter_subject` on sources (egress + ingress) | session-granular | each device decides what it shares & stores |

### The sharpening: `host` ≠ person

The sovereignty boundary is the **person**, not the machine — a person owns many machines. So:

- **Account = person** → the hard isolation boundary. Cross-person convergence is **consent-bound export/import**, not raw mirroring.
- **`host` = subject token** → the device axis *within* an account — the federation routing this doc's spike needs.
- **`user` = payload stamp (already done)** → attribution that **survives cross-account sharing** (Katie sees your shared stream; events still read `user=max`).

This reframes the lab's "every machine sees all team data": **intra-person** (your own fleet, one account) is free device mirroring — exactly the JetStream-sources mechanism here. **Inter-person** (a team) is consent-bound sharing layered on top via accounts. Nothing in this design is thrown away when permissions arrive; it gets *scoped*.

### Layer 4: per-device share/store selectivity

Each node decides, at **session granularity**, two orthogonal, **edge-enforced** policies — both implemented with the *same* `filter_subject` lever the spike used for loop-prevention:

- **Share** (egress): whether a session's subject is sourced **up** into the hub aggregate. **A session not shared never leaves the device** — enforced by omission; the hub cannot source what the leaf doesn't expose. This is the strongest form of the soul's "your data is yours."
- **Store** (ingress): `filter_subject` on the mirror's source **down** from the aggregate, plus store retention — which fleet sessions this device keeps locally.

**Authority vs. enforcement:** the *capability* is enforced at the device (sovereignty is edge-local); the *authority* to set policy is the person's (the directory). "Persons own, principals act."

### Three consistency invariants the implementation MUST uphold

1. **Catch-up respects share.** `/api/digests` + `/api/sessions/{id}/events` (used by `catch_up.rs`) must filter to the **shared set**, or the app-level backstop leaks exactly what the transport correctly withheld.
2. **Store retention consults the policy.** Always keep your *own* sessions (sovereign local record); mirror others' only where opted in.
3. **Revocation is a known hard edge.** Un-sharing drops the source (stops new flow instantly); purging already-propagated copies from peers/hub is the revocability problem (personhood Q1/Q9) — named, not assumed free.

## Implementation plan — TDD + testcontainers throughout

**Methodology (non-negotiable, per CLAUDE.md):** red → green → refactor. Every phase starts with a failing test. Convergence/topology behavior is proven with **testcontainers**, not assertions about config — the project's whole integration net is container-based (`rs/tests/test_federation_lab.rs`, `test_leaf_cluster`, `test_deployment_states.rs`, the `nats-permissions` harness on `wip/shape-layers-and-friends`). New behavior gets a container test that *fails first*. Keep the shipped catch-up (`024fcc2`) as the topology-agnostic backstop; sources become the fast native path.

Each phase = its own branch/PR, atomic commits, `just test` before push.

**Phase 1 — `host` in the subject** *(foundation; unblocks all)*
- RED: extend `rs/tests/test_subject_hierarchy.rs` — assert `events.{host}.{project}.{session}.main` and that `events.{host}.{project}.{session}.>` still fans in main + subagents.
- GREEN: thread `host::host()` into `nats_subject_from_path` (`rs/core/src/paths.rs`).
- Blast radius low (no positional subject parsers; wildcard routing unaffected). Solo mode unchanged.

**Phase 2 — JetStream sources federation** *(spike → production; depends on P1)*
- GREEN: `ensure_streams` federation mode (`OPEN_STORY_HUB_DOMAIN`): local `events` bound to `events.{host}.>`; source-only `events-mirror` sourcing the hub aggregate; self-register into the aggregate via the cross-domain API. Consumers read `events ∪ events-mirror`.
- Faster inner loop: port the spike's hub+2-leaf shape into the reusable `nats-permissions` subprocess harness for sub-second red/green before the full container lab.

- **RED — exercise multiple topologies and scale, all via testcontainers** (`rs/tests/test_federation_lab.rs`). Each asserts `fully_mirrored` **cold with catch-up OFF** — pure transport convergence:

  | # | Topology | What it proves | Container shape |
  |---|---|---|---|
  | T1 | **Solo multi-device** — 1 person, N devices, one account, no hub | intra-person device mirroring (the common case) | N leaves peering, no central hub |
  | T2 | **Single-hub star** — N leaves → 1 hub (today's lab shape) | team aggregation + every-node-mirrors-all | N leaves + 1 hub (≈2N+1 containers) |
  | T3 | **Multi-hub / mesh** — leaves → 2 hubs, hubs source each other | hub-to-hub sourcing converges with **no double-count** across hubs (the loop-prevention + namespacing claim at the hub tier) | 2 hubs + leaves split across them |

  Plus the lifecycle + scale variants:
  - **Late-joiner**: one node starts +30s after the rest — sources must backfill it gap-free (the cold-boot race that fails 4/4 today).
  - **Scale ramp**: extend `lab_federation_ramp` to **10 → 25 → 50 → 100 nodes**, recording convergence time and the single-host container ceiling (where Docker/runner resources, not the protocol, break it). 100-node is the headline scale test — gate it behind `--ignored` (and a `RUN_SCALE_TESTS` env) so it's opt-in, not on every `cargo test`.

- **Measure** convergence vs catch-up's 12.8s, per topology, plus aggregate-bytes overhead and the 100-node ceiling. **→ v0 ship line is decided here, with numbers in hand.**

**Phase 3 — Accounts = person isolation** *(depends on P2)*
- RED: testcontainer with two accounts (two persons) — assert person A's JetStream is invisible to person B *without* an explicit export/import; assert export/import makes a shared stream visible. (Extend the `nats-permissions` harness — it already proves account isolation.)
- GREEN: account dimension in `Config` + `NatsBus::connect()`; v0 single-account but structured so per-person accounts are a config split. Source/mirror wiring parameterized by account.

**Phase 4 — Per-device share/store filters** *(edge sovereignty; depends on P1+P2)*
- RED: testcontainer — node marks session X private; assert X **never** appears on the hub or any peer (egress filter) and that catch-up/digests also exclude it (invariant ①). Second test: node opts out of storing project Y; assert Y is absent locally but still on the hub (ingress filter). Third: retention keeps own sessions regardless (invariant ②).
- GREEN: per-node share/store policy (config); `filter_subject` on source-out / source-in; digest + catch-up filtered to shared set; retention consults policy.

**Phase 5 — Grants / roles / permissions** *(fullest realization; depends on P3+P4)*
- RED: per-subject permission tests on the `nats-permissions` harness (publish/subscribe violations); cross-person export/import = consent-bound sharing; role = permission profile.
- GREEN: user/password→NKEYs auth; roles catalog; participant directory. Reconcile with in-flight `feat/person-id-fleet-view`.

### Tracking

These become `docs/BACKLOG.md` entries (one per phase, branch-per-item). The stale `BACKLOG.md:457` claim — *"all sessions land on every node, JetStream propagates bidirectionally"* — should be corrected: that's the racy core-propagation this work replaces.
