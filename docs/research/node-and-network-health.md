# Node & Network Health Read

**Status:** design + first implementation (node `/api/health`). Companion to
`[[state-management-interface]]` — the *read* side to that doc's *act* side.

## Why

Operating a fleet of OpenStory nodes is two halves: **observe** (health) and
**act** (reproject / verify / catch-up / prune). This doc is the observe half.
The architecture review kept surfacing failure modes that are currently
*silent* — stream eviction at the 1 GB cap, a leaf whose hub link dropped
(`leafs: 0`), projections left stale after a restart (the token-0 divergence),
ingest backpressure. A health read turns each of those from a silent failure
into a visible signal. It is pure observation — it watches, never interferes.

Most of the raw signal already exists (`Bus::is_active`, `WatcherDiagnostics`,
`store.projections`, `EventStore::list_sessions`); this is largely
**aggregation and exposure**, not new instrumentation.

## Two levels

### Node health — "is *this* device healthy?"  → `GET /api/health`

A single aggregate read. v1 fields (all reachable from `AppState` today):

| Field | Source | Surfaces |
|-------|--------|----------|
| `status` | overall roll-up | quick green/red |
| `store.backend` | `config.data_backend` | sqlite vs mongo |
| `store.sessions` | `event_store.list_sessions().len()` | durable session count |
| `bus.connected` | `bus.is_active()` | NATS up / NoopBus / disconnected |
| `projections.count` / `.sessions` / `.fresh` | `store.projections.len()` vs sessions | **projection freshness** — the token-0 class of divergence becomes visible (`fresh=false` ⇒ run `reproject`) |
| `watchers` | `watcher_diagnostics.snapshots().len()` | active watch actors (detail at `/api/watchers`) |
| `version` | `CARGO_PKG_VERSION` | build identity |

Future fields (designed, not v1): NATS stream bytes vs the 1 GB limit (eviction
risk), boot `reconcile`/`reproject` reports, ingest rate + persist latency,
schema-migration state, store-degraded (JSONL-fallback) flag.

`/health` stays the dumb liveness probe; `/api/health` is the detailed read.

### Network health — "does the *fleet* agree?"  → `GET /api/fleet` (designed)

- **Liveness** — which leaves are connected now (NATS `/leafz`: leaf count, last
  activity). Makes the leaf-supervisor `leafs: 0` disconnect a visible red light.
- **Convergence** — a per-node **event-id digest (Merkle root per
  session/project)**; compare roots across nodes to detect divergence *without
  shipping the data*.
- **Lag** — JetStream consumer sequence gap, framed git-style as **"ahead/behind
  N events vs hub."**

## The unifying insight

**`verify` (the state-management action) and convergence (the health metric) are
the same Merkle primitive.** One per-node event-id digest answers both "are we in
sync?" (health) and "which ranges differ?" (catch-up input). Design them
together: one digest, two consumers.

## Prior art

- **Membership / liveness** — SWIM / gossip (Serf, Consul, Cassandra);
  Kubernetes liveness/readiness probes; Prometheus `node_exporter` per node +
  federation for the fleet.
- **Convergence-as-health** — Cassandra `nodetool status` (up/down/load) + repair
  state; Riak ring status; Dynamo treats Merkle divergence as a health signal.
  "Are replicas in sync" *is* a health metric.
- **Sync engines** — **Syncthing**'s per-device "Up to Date / Syncing / Out of
  Sync %" is almost exactly the fleet view; **git** `ahead/behind N` is the
  cleanest per-node lag model.
- **Self-reporting triad** — `/health` + `/ready` + `/metrics`. OpenStory already
  has `/health` and optional Prometheus (`metrics_enabled`).

## Sequencing

1. **Node `/api/health`** — mostly aggregation; high value / low cost; surfaces
   this review's findings (projection freshness, bus connectivity, watcher
   activity). *(v1 implemented)*
2. **Network `/api/fleet`** — co-designed with `verify`; shares the Merkle digest.
3. Enrich node health with stream-bytes-vs-limit, boot reports, ingest latency.

## Related

- `[[state-management-interface]]` (the act side; `verify` shares the digest).
- BACKLOG: self-reporting `/api/health` item (this realizes its node level).
- `[[arch-review-perf-hardening-branch]]`, `[[perf-ingest-serial-ceiling]]`.
