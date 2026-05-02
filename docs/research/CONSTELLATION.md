# The Constellation

*Notes on what Open Story is once it has more than one home — and the principles and requirements that follow from leaning into that shape.*

---

## How to read this brief

This is a framing document, not a specification. If you're an agent or human reading it for context:

- **The principles section is the load-bearing part.** Each principle names a stance the architecture is taking (or could take). Verify by reading the code paths cited inline. Where a principle reads aspirational, treat it as direction-of-travel — not a current-state claim.
- **The requirements section is concrete primitives.** Each one is a small, recognizable architectural object. Some exist already. Some are sketched. None are committed.
- **Origin matters here.** This brief came from a specific debugging session in May 2026 where the system told us, in a small voice, that it had quietly become distributed and we hadn't named it yet. The observations are real. The framing is one reading of them — not the only reading.
- **Tone is interpretive throughout.** Match the house style of whatever you're writing into, not this brief. The voice here is reflective on purpose; that doesn't make it the voice for everything.

---

## Origin

In May 2026, while reviewing Katie's user-stamping branch, two things converged:

1. We confirmed that NATS leaf↔hub federation actually works end-to-end. Katie's events from `Katies-Mac-mini` arrived in our local JetStream, stamped with `host: Katies-Mac-mini, user: katie`. Federation is real, not aspirational.
2. We discovered that those events landed in our local SQLite (under one persist consumer process), while a later persist consumer process — same machine, different backend (Mongo) — never saw them. The UI, reading from Mongo, showed *nothing of Katie*. Mongo had the new live writes, SQLite had Katie's history, JSONL on disk had everything, the React UI had only what was in Mongo. Four homes, four different answers.

The natural reaction was: this is a bug, write a Reconciler. The deeper reading was: *this is the architecture revealing itself.* Open Story has been distributed for a while. We just haven't been writing invariants as if it were.

What follows is what we want to lean into — not a plan.

---

## The structural observation

A single coding event in this system has, today, **eight honest answers** to "where does it live?":

```
1. The agent's own native transcript JSONL    (~/.claude/projects/...)
2. The local NATS JetStream (leaf)            (process-local stream state)
3. The local "always-on" JSONL backup         (data/<sid>.jsonl)
4. The local EventStore                       (SQLite or Mongo, swappable)
5. The hub's NATS JetStream                   (debian-16gb-ash-1 / Tailscale)
6. The hub's local persist consumer's store   (Bobby on Hetzner)
7. Other peers' local stores via federation   (your laptop, Katie's mini, ...)
8. The UI's in-memory RxJS state              (the rendered view)
```

There is no canonical home. Each is *authoritative for its purpose* and *partial in scope*. The system's job is not to collapse these into one — it's to make their relationships legible.

This is a constellation, not a hierarchy.

---

## Principles

The following stance is what the architecture *implies*. Where the codebase already encodes it, we cite. Where it doesn't yet, we name what's missing.

### P1. Sovereignty is a property of the graph, not of any node

Each layer of the constellation independently preserves the user's data. JSONL on your disk is yours. The replicated copy on the hub is yours (the hub is custodian, not owner). Bobby's local store containing your federated events is yours. **No single layer is load-bearing for sovereignty.** If the hub vanishes, individual machines retain their own histories. If your machine vanishes, the hub still holds it. If both vanish, JSONL on disk survives; if your disk vanishes, federation peers have what they pulled.

Sovereignty is therefore *redundancy across mutually-non-trusting nodes*, not isolation.

A practical caveat: **JSONL canonicality is operator-configured.** The binary always writes JSONL — that's a code-level guarantee in `rs/store/src/persistence.rs`. But whether JSONL persists across container restarts depends on a volume mount. All shipped compose configs (`docker-compose.prod.yml`, `docker-compose.infra.yml`, `docker-compose.leaf.yml`, dev `docker-compose.yml`) mount `/data` to a persistent volume, so the architecture's promise holds for production deploys. Ad-hoc `docker run` deployments without a volume mount have ephemeral JSONL — in those, the EventStore is the canonical layer for that node and reconciliation is a no-op. The principle holds at the binary level; its operational truth is a deployment decision.

→ Implies: every durable layer is rebuildable from any other durable layer; no layer is a single-point-of-truth-or-loss.

### P2. Multiple truths, named explicitly

Different layers answer different questions and are each authoritative within their own scope:

| Truth | Authoritative for | Today's location |
|---|---|---|
| Wire truth | "Did this event happen?" | NATS JetStream |
| Disk truth | "Is this preserved beyond process lifetime?" | JSONL append logs |
| Index truth | "Can the UI query this fast?" | EventStore (SQLite/Mongo) |
| Render truth | "What is the user looking at right now?" | UI in-memory state |
| Coordination truth | "What are agents doing right now?" | Wire (subset of NATS) |

These can disagree without being broken. The architecture's job is to expose the disagreements, not pretend they don't exist. **A health endpoint reporting "Wire: 132,000, Disk: 132,000, Index: 95,000" is more sovereign than one reporting `{status: "ok"}`** because it makes the actual state of the constellation legible.

→ Implies: each truth has a named accessor and a named lag/disagreement metric.

### P3. Intentional divergence is allowed

Different machines may want different scopes of the team's history:

- A laptop running in offline-friendly mode: last 30 days of own events only.
- A hub VPS: 7 years, everything, fully indexed for search.
- A privacy-shaped derivative store: events with prompts redacted, just tool-shapes.
- A release-only view: events whose sessions landed in master.

These divergences are **not drift** — they're *fitness to local context*. A reconciler that forces global convergence is the wrong abstraction. A *policy-aware* reconciler that converges within a *declared scope* per machine is the right one.

→ Implies: every store has a manifest declaring "what slice of the constellation should I contain?", and reconciliation operates against that manifest.

### P4. Compute moves to the data, not the other way around

Patterns, projections, summaries, embeddings, search indexes — every derived view is *another consumer over the same wire stream*. They need not run on the same machine, in the same process, against the same store, with the same retention.

- Patterns consumer per-developer with personalized rules.
- Team-wide patterns consumer on the hub with shared rules.
- Embeddings consumer on a GPU-equipped peer that never touches developer laptops.
- Ticketing actor on the hub watching for `system.error` patterns.

The bus is the substrate; the views are personal. Today this is enabled but underused — there is mostly one persist, one patterns, one projections. The architecture allows many.

→ Implies: a consumer is a *first-class object*, not a hardcoded actor — versioned, replayable, deployable.

### P5. Replay is the primitive, not migration

Events are immutable. Consumer position is a value. So:

- Deploy a new pattern detector → reset its consumer to t=0, replay history, backfill detections.
- Switch the EventStore backend → reset persist to t=0, rebuild index from wire.
- Change how a view interprets events → no migration, just re-derive.

This means **changing your mind about how to read history costs only the rerun.** Migrations become a special case of replay. There is no "schema is the source of truth" — the events are. The schema is just one current reading of them.

→ Implies: every consumer is replayable, durable consumers expose their position as a queryable/resettable value, and re-derivation is a routine operation.

### P6. Permissioning is routing, not row-level filtering

NATS subjects are hierarchical (`events.{project}.{session}.user.{user}.host.{host}.>`). Privacy can be enforced at the *forwarding* boundary, not after the fact:

- A leaf simply does not forward `events.private.{me}.>` to the hub.
- A team channel: `events.teamspace.>` is shared with everyone authorized.
- Cross-team coordination: `events.handoff.{from}.{to}.>` shared narrowly.

The user's data does not leave the local boundary because it was *never published past it.* This composes naturally with sovereignty (default local; explicit acts of sharing) and avoids the hard problem of post-hoc redaction in a database.

→ Implies: subject hierarchy is a *first-class design surface*, not an incidental string format. It encodes the visibility graph.

### P7. The bus is the agents' shared workspace memory

Agents are not just observed; they can be productive citizens of the bus. Today, hooks publish to NATS via `POST /hooks`. The same channel can carry agent *intent*, not just transcripts:

- "I'm beginning a refactor on `rs/store/sqlite_store.rs`."
- "I'm blocked on a missing token."
- "I'm done; my output is at this artifact path."

Other agents subscribe to "what's hot in the team's code right now," see the claim, defer overlapping work, publish their own state. **The medium for collaboration is the same medium as the audit log.** The principle "observe, never interfere" still holds — Open Story does not write into the agent's own state; it just makes a richer substrate available for the agents themselves to publish into.

→ Implies: structured intent events, conventions for agent claims/releases, perhaps a small protocol layer over NATS subjects.

### P8. The hub is plural by configuration, not by code

Today there is one hub (Hetzner). The architecture does not require this. NATS supports:

- Many leaves, one hub (current shape).
- Multiple hubs federating with each other (cross-region, cross-org).
- Peer-to-peer mesh where every leaf is also a partial hub.
- A user-owned personal hub as a sovereign endpoint, optionally federating with a team hub.

The "hub" is a routing convenience, not an authority. Sovereignty over what crosses which boundary remains with the publishing leaf.

→ Implies: avoid baking "the hub" into the code as a singleton concept; design configuration so that the topology is data, not assumption.

### P9. The system observes itself

Open Story already observes coding agents. Nothing prevents it from observing itself:

- Persist consumer lag as a published event.
- JetStream message rate spikes.
- Cold-backend warnings.
- Reconciliation diffs as observable patterns.

The pattern detector that finds patterns in your code can find patterns in *its own operation*. Internal observability and external observability are the same shape. **Recursive sovereignty.**

→ Implies: operational events publish to the same bus on a dedicated subject (e.g. `events.openstory.internal.>`), and the same actors that analyze user work can analyze the system's work.

---

## Requirements

The principles above imply a set of architectural primitives. Some exist; some are sketched; some are missing. Each is a small, recognizable object — not a roadmap.

### R1. The Reconciler

A consumer-shaped actor whose job is to enforce P2 + P3:

> *Given a manifest declaring "this store should contain slice S of the constellation," ensure that every event matching S that exists in any rebuildable layer (JSONL on disk, JetStream wire, federated peer) is present in this store, idempotently.*

Properties:
- Idempotent by event-ID primary key (already guaranteed by all current EventStore impls).
- Bounded: scans within the declared slice, not globally.
- Reportable: emits structured diffs ("added 957 events spanning Katie's sessions") to the bus.
- Triggerable: on boot, on backend switch, on schedule, via admin endpoint.

This is the immediate gap that the May 2026 incident revealed. It also subsumes "backfill from JSONL" as a routine, not an emergency procedure.

**v1 implementation:** `rs/server/src/reconcile.rs` — a JSONL-source reconciler called unconditionally during boot, before any consumer subscribes to NATS. Idempotent via primary-key dedup; no-op when `data_dir` is empty (so fresh contributors see indistinguishable behavior). Also exposed as `open-story reconcile` for explicit operator invocation. Future versions add slice manifests (R4), scheduled rerun, wire-source replay (reset durable consumer + replay JetStream), federated-peer source, and an admin endpoint. The first version is intentionally narrow — disk-source only, all-or-nothing scope — but the function lives behind a stable interface that the principled R1 grows into.

### R2. Multi-truth health surface

A `/api/health` endpoint (and matching internal event subject) that reports each truth (P2) separately:

```
{
  "wire":          { "stream": "events", "messages": 132127, "bytes": 1073494604 },
  "disk":          { "jsonl_files": 425, "total_events": 132129 },
  "index":         { "backend": "mongo", "events": 95012, "lag_vs_disk": 37117 },
  "render":        { "ws_clients": 3, "last_broadcast": "2026-05-02T16:09Z" },
  "consumers":     [{ "name": "persist", "position": 132127, "lagging": false },
                    { "name": "patterns", "position": 131005, "lagging": true }]
}
```

This makes the failure modes legible. The May 2026 incident would have surfaced as `index.lag_vs_disk: 37117` — a number you'd notice.

**Tactical seed parked:** a `scripts/constellation_status.py` data-spike — a single-machine introspection script that walks each accessible layer (JSONL on disk, SQLite, Mongo, NATS JetStream via `/jsz`, the API itself) and reports per-layer state. Useful for diff-across-machines (each operator runs the same script, results are compared to surface drift between peers). Sketched during the May 2026 incident debugging but deferred — the boot-time R1 implementation removes the *acute* need by self-healing on restart. The spike still belongs as the prototype of R2 once an operator surface (HTTP endpoint, dashboard) is wanted.

### R3. Consumer as first-class object

A consumer is not a hardcoded `tokio::spawn` in `state.rs` but a registered, named, versioned entity:

- Has a stable name (e.g. `persist-mongo-v3`).
- Has an associated durable JetStream consumer, position queryable.
- Has a manifest of what slice of the bus it processes.
- Has a `replay-from` admin operation that resets position and re-runs.
- Reports its own lag and error rate to the internal observability subject.

Once consumers are objects, swapping backends / deploying new patterns / re-deriving everything become routine.

### R4. Slice manifests

A short declarative description of what a node wants from the constellation. A laptop manifest might say:

```toml
[slice]
include  = ["events.team.>", "events.private.maxglassie.>"]
retention_days = 30
```

A hub manifest might say:

```toml
[slice]
include = ["events.>"]
retention_days = 2555  # 7 years
```

The reconciler (R1) honors the manifest. Federation routing honors the manifest. Permissioning (P6) is implicitly the union of manifests across the network.

### R5. Subject hierarchy as design surface

Today's subject scheme:

```
events.{project}.{session}.main
events.{project}.{session}.agent.{agent_id}
```

A more expressive scheme that encodes principles P3 + P6 + P7:

```
events.user.{user}.host.{host}.project.{project}.session.{session}.{phase}
events.intent.user.{user}.host.{host}.{kind}              # agent intent (P7)
events.openstory.internal.{component}.{kind}              # self-observation (P9)
events.private.{user}.>                                   # never forwarded (P6)
events.share.team.>                                       # forwarded to hub (P6)
```

Subjects are the routing-and-permissioning DSL. They deserve a design pass.

### R6. Replay as a routine operation

Given R3 (consumers as objects), expose:

- `POST /admin/consumer/{name}/replay-from?seq=0` → reset position, re-run.
- `POST /admin/consumer/{name}/replay-against?store={alt}` → re-run into an alternate store (for shadow-testing new derivations).
- A pattern of "derived stores are disposable; sources are precious" baked into ops conventions.

Migration scripts become a smell. The path is "stand up a new derived store, replay into it, switch reads, drop the old one."

### R7. Agent intent protocol

A small set of event types over a dedicated subject hierarchy enabling P7:

```
intent.claim       — { user, host, agent_id, scope: "rs/store/sqlite_store.rs", expires_at }
intent.release     — { claim_id, reason }
intent.progress    — { claim_id, summary }
intent.handoff     — { from: claim_id, to: { user, host }, reason }
intent.observe     — { claim_id, observation, severity }
```

These are not enforcements (Open Story still does not interfere). They are *advisory broadcasts* that other agents and humans may consult. The principle is: make coordination *possible*, leave coordination policy to the agents and the humans.

### R8. Internal observability subject

A dedicated subject prefix for the system observing itself:

```
events.openstory.internal.{component}.{event}
```

Any actor that already publishes user-work events also publishes its own operational events on this subject. Patterns consumer can detect "lag growing, restart loop suspected" the same way it detects "user is in flow state." Recursive sovereignty (P9).

### R9. Topology as data

The hub URL is config. The federation graph is config. The retention per layer is config. Nothing in the binary should assume "the hub" or "one EventStore" as load-bearing concepts. The same binary should run as a leaf, a hub, a peer, a derived-view-only node — selected by a single TOML file plus env.

This is mostly true today; the principle is: *don't backslide into singleton assumptions when adding features.*

---

## Open questions

These are the questions worth chewing on as we build toward the constellation. Naming them so they don't get lost:

1. **Granularity of subject hierarchy.** Is `events.user.{user}.host.{host}.project.{project}.session.{session}.{phase}` the right level of detail, or do we collapse some axes? Subjects affect routing performance, retention math, and the cognitive load of permissioning.

2. **Retention-per-truth vs retention-per-slice.** Is retention a property of *which truth* (wire/disk/index) or *which slice* (team/private/public)? Or both? The interaction between R4 (slice manifests) and per-layer retention is non-obvious.

3. **Authoritative-time question.** When peer A and peer B both have an event but A says it arrived at T1 and B says T2, which is "true"? The original `time` field on the CloudEvent should be invariant across peers — but ingest time per peer is per-peer. Both might matter. Naming both makes the constellation honest about time.

4. **Conflict resolution for derived views.** Two agents stamp the same session with different derived metadata (e.g. one runs a v3 pattern detector, another runs v4). Both publish to `events.derived.>`. Are derivations append-only (new event each time)? Versioned? Replaceable? This is the "derived state" half of the architecture, less developed than the raw-event half.

5. **What counts as "the team"?** P3 says intentional divergence is allowed. Federation requires shared subjects. When does a private-by-default leaf become a team participant — and what's the act of joining? Is it an admin act, a user act, an agent act? The boundary deserves design.

6. **Replay cost at scale.** R5/R6 lean on "replay is cheap because of PK dedup." This is true for SQLite at hundreds-of-thousands of events. Does it hold at tens of millions? Hundreds of millions? At some scale, replay becomes a *budget* not a *primitive*, and the architecture changes shape.

7. **What is the consumer's identity persistence story?** R3 implies durable consumer names. If a consumer's code changes meaningfully, is it the same named consumer (replay against new code) or a new named consumer (run alongside the old one)? Both make sense in different scenarios. We need a convention.

8. **Bidirectional observability and hooks.** Today's hooks are `agent → openstory`. P7's intent protocol is `openstory bus → other agents`. What's the right shape of subscriber-from-bus on the agent side — a poller, a websocket, an MCP tool that surfaces "team intent" as context? The interface design here will shape adoption.

9. **What does "personal hub" look like in practice?** P8 says hubs are plural. A user-owned personal hub as a sovereign endpoint is appealing. What does it cost to operate one? Does it federate easily with team hubs? Is there a path from "everyone runs a leaf" to "everyone runs a personal hub" that's progressive rather than disruptive?

10. **Failure modes of the constellation framing itself.** Are there problems this shape solves *less well* than a single-source-of-truth shape? The honest answer affects which problems we should *not* try to solve this way (e.g. transactional financial operations across the constellation are probably out of scope; that's not Open Story's job, but the boundary is worth naming).

---

## What this brief is for

To remind us, in future arguments about features, that the architecture is already a constellation and the question is whether each new feature *leans into that shape* or *pushes against it*. Either may be the right call for any given feature. But the choice should be conscious.

When the answer feels obvious, it usually is. When the answer feels difficult, this brief is a place to come back to.

---

*Written 2026-05-02 from a debugging session that turned into a conversation. The bug was real; the framing is one reading. Other readings welcome — that's also in the spirit of the thing.*
