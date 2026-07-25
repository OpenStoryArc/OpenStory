# Session citizenship — plan

**Status:** Slice 0–1 implemented on `feat/session-citizenship-from-loop` (`7974058` Slice 1 + `6e2c3ff` bus reconnect); this doc is the durable plan for the product surface and remaining work.  
**Origin:** Self-reflective Grok loop (2026-07-17) that discovered live sessions with empty Explore atoms.  
**Soul:** Observe, never interfere. Live and Explore must not be silently merged. Citizenship is whether the **mirror holds** a session, not whether a file is changing.

Related: `docs/research/node-and-network-health.md` (node health aggregation), `docs/prompts/citizenship-loop.md` (Slice 1 checklist, on citizenship branch), `scripts/session_citizenship.py`.

---

## Problem

OpenStory promises a mirror: you and the agent can look back at what happened. That requires **memory**, not only motion.

| Layer | Meaning |
|-------|---------|
| **Live** | Transcript on disk + watcher → NATS publish |
| **Citizen (Explore)** | SQLite / REST / MCP / Story — durable, queryable |

We observed **ghosts**: disk growing, watcher diagnostics green (`publish success=true`), store/MCP empty for that UUID. Self-reflection tools fail while the human believes OpenStory is watching.

### Root cause (class)

1. Grok (or any chatty agent) backfills a large burst into JetStream.
2. Persist consumer falls behind → NATS **slow consumer** / max pending.
3. Push subscription ends; without reconnect the consumer task **exits forever**.
4. Watcher keeps publishing (acks look fine).
5. Nothing drains into SQLite → **ghost**.

---

## Vocabulary (product)

| Verdict | Disk | Store | Meaning |
|---------|------|-------|---------|
| **citizen** | yes | yes (events > 0) | Live and Explore agree — sovereignty path intact |
| **ghost** | yes | no | Live without memory — sovereignty failure |
| **orphan-store** | no | yes | Durable rows, no local transcript (federated / deleted disk) |
| **absent** | no | no | Unknown id |

Node-level **ghost_risk** (coarse): watchers have `cloud_events_emitted > 0` and store session count is `0`. Does **not** replace per-session ghosts when the store is non-empty.

---

## Implementation slices

### Slice 0 — Stop the silent death *(done on citizenship branch)*

| Item | Notes |
|------|--------|
| NATS consumer reconnect | Exponential backoff when delivery ends; `DeliverPolicy::All` + event-id PK dedup |
| Larger buffers | `mpsc` 2048, `max_ack_pending` 4096 |
| Script | `scripts/session_citizenship.py` (verdicts, `--test`, fleet scan) |

**Commit:** `6e2c3ff` — `fix(bus): reconnect NATS consumers after slow-consumer drop`

### Slice 1 — Make ghosts visible *(done on citizenship branch)*

Product surface so operators/agents do not need NATS logs.

| Item | Surface |
|------|---------|
| Pure classify + Grok disk probe | `rs/server/src/citizenship.rs` |
| Per-session API | `GET /api/sessions/{id}/citizenship` → always 200 + verdict + disk/store/watcher |
| Node health | `GET /api/health` → `citizenship: { ghost_risk, watcher_cloud_events_emitted, store_sessions }` |
| MCP | `session_citizenship` tool (GET REST via `OPENSTORY_API_URL`) |
| Tests | unit (`citizenship` lib), integration `test_citizenship`, MCP URL/arg tests |
| Dogfood | script `--test`; live absent/ghost; scan found real Grok ghosts |

**Commit:** `7974058` — `feat(citizenship): Slice 1 — ghosts visible via API, health, and MCP`

**Ops notes learned while dogfooding:**

1. Rebuild the **CLI** binary: `cargo build -p open-story-cli` (not `-p open-story` alone).
2. Large production DB (~2.5GB) can hang boot in `reconcile_local` **before** `:3002` binds — dogfood with a light `--data-dir` when validating routes.
3. Coarse `ghost_risk` can be false while per-session ghosts still exist (store non-empty).

### Slice 2 — Make ghosts rarer *(planned — primary backlog)*

Goal: Grok-sized bursts cannot recreate the failure mode.

| # | Work | Why | Acceptance |
|---|------|-----|------------|
| 2.1 | **Durable named JetStream consumers** (resume from last ack) | Ephemeral + `DeliverPolicy::All` redelivers everything after reconnect; durable is correct long-term | Consumer survives disconnect; no full-stream replay storm; tests for reconnect + ack |
| 2.2 | **Watcher backfill rate-limit** | Belt-and-suspenders so publish rate cannot outrun persist | Configurable batch/delay; no ghost under synthetic Grok backfill fixture |
| 2.3 | **Persist lag / reconnect metrics** | Prove recovery in ops | Counters or `/api/health` fields: `consumer_reconnects`, optional `persist_lag` |
| 2.4 | **Ghost remediation path (observe-only)** | After fix, how do humans heal existing ghosts? | Documented: restart serve with reconnect + optional re-ingest/backfill from disk without mutating agent transcripts |

Constraints: observe never interfere; no harness injection; BDD red→green.

### Slice 3 — Productize the dual ontology *(planned — secondary)*

| # | Work | Why |
|---|------|-----|
| 3.1 | **Finer ghost_risk** | Flag when `cloud_events_emitted` climbs while store session count is flat (not only store==0) |
| 3.2 | **List API** | `GET /api/citizenship?agent=grok` or filter on session list — fleet scan without the Python script |
| 3.3 | **UI honesty** | Live vs Citizen labels; ghost badge on session cards; never merge WS live into Explore REST into one lie |
| 3.4 | **Agent self view** | `openstory://me` / default “am I a citizen?” for *this* session id without hunting UUID |
| 3.5 | **Multi-agent disk probes** | Claude JSONL / Codex / pi-mono paths in addition to Grok `updates.jsonl` layout |

### Slice 4 — Fleet (optional, ties to SICP distributed work)

| # | Work | Why |
|---|------|-----|
| 4.1 | **Golden-set distance** | Declarative: every device holds a superset of named golden sessions (event-id set) |
| 4.2 | **Cross-node citizenship** | Ghost on laptop vs citizen on hub (federation lag vs true loss) |

Out of scope for citizenship core: mutating agent processes, merging live WS into store views.

---

## Architecture sketch

```
  Agent transcript (disk)          NATS JetStream
         │                              │
         ▼                              ▼
    Watcher ──publish──► events.* ──► Persist consumer ──► SQLite
         │                    │              │
         │                    │              └─ (if dead → ghost)
         │                    └─ diagnostics (can look green)
         │
         ▼
  citizenship classify(on_disk, in_store, counts)
         │
         ├─► GET /api/sessions/{id}/citizenship
         ├─► GET /api/health  citizenship.ghost_risk
         └─► MCP session_citizenship
```

Classification is pure; I/O (disk walk, store, watcher snapshots) stays at the API edge.

---

## Test plan (by slice)

| Slice | Gates |
|-------|--------|
| 0 | Bus reconnect unit tests; dogfood under backfill stress |
| 1 | `cargo test -p open-story-server --lib citizenship`; `cargo test -p open-story --test test_citizenship`; MCP citizenship tests; `python3 scripts/session_citizenship.py --test` |
| 2 | Integration: kill persist mid-backfill → reconnect → session becomes citizen; rate-limit unit tests |
| 3 | UI / API list tests; agent multi-root disk fixtures |

---

## Dogfood checklist (any machine)

```bash
# Correct binary
cd rs && cargo build -p open-story-cli

# Health (needs serve with Slice 1 binary)
curl -s http://127.0.0.1:3002/api/health | jq .citizenship

# Per session
curl -s http://127.0.0.1:3002/api/sessions/<id>/citizenship | jq .

# Script
python3 scripts/session_citizenship.py --test
python3 scripts/session_citizenship.py --agent grok --api http://127.0.0.1:3002
```

---

## Branch / merge notes

| Branch | Contents |
|--------|----------|
| `feat/session-citizenship-from-loop` | Slice 0 bus reconnect + Slice 1 product surface |
| master / other feature branches | May lack citizenship until merge |

When opening a PR: require CI green on citizenship unit + integration tests; note rebuild of `open-story-cli` in the PR body.

---

## Success criteria (overall)

1. A human never needs to grep NATS logs to learn the mirror is blank for a session.
2. An agent can ask `session_citizenship` and act on the verdict (pause trust in recall if ghost).
3. Under Grok-sized backfill, persist either keeps up or reconnects until the session is a **citizen**.
4. UI (when Slice 3 lands) never claims Explore completeness for a ghost.

---

## One-line north star

**Every co-creator session should be either a citizen of the durable mirror or an explicit ghost — never a silent blank.**
