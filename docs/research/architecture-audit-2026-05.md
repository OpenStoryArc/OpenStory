# Architecture Audit — May 2026 (branch `test/arch-review-perf-hardening`)

An architecture review of OpenStory run as an **empirical "test until it breaks"
loop**, not a paper read. Every seam was reviewed by making its failure
reproducible in a container, then closing it. Hardware: a 32-core / 123 GB
workstation with an RTX 5090 (used to drive a real pi-mono session via LM
Studio). 15 commits, `3525a07`→`b2d891e`.

## Method

1. **Test until it breaks, then read the break.** Ramp past tuned thresholds;
   the failure mode localizes the bottleneck.
2. **Measure → diagnose → fix → re-measure.** Every fix is confirmed by the wall
   *moving*, never by assertion alone.
3. **Validate the instrument before the system.** Several early "findings" were
   broken *tools* (the image, a wrong-shape parse). A review that trusts its own
   instruments measures fiction.
4. **Invariance as a probe.** "Scale a resource 32× and nothing changes" *is* the
   diagnosis (serial I/O, not compute).
5. **Dogfood the real thing.** Synthetic fixtures can't tell you `pi --no-session`
   writes nothing, or that real pi-mono output still translates. Running it did.
6. **Change behind the seam.** Fixes land behind `EventStore` / `Bus`; the
   conformance suite keeps both backends honest.

## Findings & fixes

Severity = blast radius × likelihood × how-hidden.

| # | Finding | Sev | Status | Commit |
|---|---------|-----|--------|--------|
| 1 | `open-story:test` image unbuildable — Dockerfile never copied `mcp/benches/` (blocked *every* container test + the lab) | Blocker | ✅ fixed | `3525a07` |
| 2 | Perf break-finder parsed the wrong `/api/sessions` shape → phantom 100% loss, yet tests passed green | High | ✅ fixed + honest asserts + host-scale tier | `5493ca9` |
| 3 | Ingest pinned at ~44 ev/s, **CPU-invariant 0.5→16 cores** → serial fsync wall; persist wrote per-event | High | ✅ batched → ~250 ev/s, ceiling 13.5k→121k events | `08f3ed9` |
| 4 | Boot is O(total history) with redundant scans; reconcile per-event; two full payload scans for one field each | Med | ✅ batched reconcile + single-pass boot | `779c8e8` |
| 5 | Codex (3rd translator) emits `system.turn.context`/`system.token_count` — absent from the `Subtype` enum | Med | ✅ registered + dogfood | `9a28c93` |
| 6 | Codex absent from the translator integrity tests (raw-passthrough, host-stamping, subtype) | Med | ✅ added | `36c5d14` |
| 7 | After restart, projections aren't rehydrated → `/api/sessions` shows 0 tokens for source-less sessions | Med | ✅ `reproject` at boot | `0e8d731` |
| 8 | **Federation node→hub loss**: at ~10 concurrent nodes the hub silently drops a session — `subscribe(New)` delivers no backlog, after the inline replay was deleted on a false premise | High | ✅ catch-up subscription (`All`); 10→100 nodes converge | `cffbf30` |
| 9 | **Federation hub→nodes mirror gap**: faithful lab — hub 10/10 but slowest node 8–9/10. Confirmed a timing-dependent cross-leaf race (cold 10-node fails 4/4; warm converges) — not structural, not random | High | ✅ **fixed** — app-level catch-up anti-entropy (peer-generic); cold 10-node → fully mirrored in 12.8s. NATS-level sources/mirrors remain an optional transport-layer alternative (#23) | `024fcc2` |

### Empirical before/after (the two that moved a number)
- **Ingest (persist batching):** Medium tier ~44 → ~250 ev/s; broke at 13,480 events → now clears 40,400, breaks at 121,120 (~9× ceiling).
- **Federation (catch-up subscription):** 10 nodes 9/10 plateau (241s, never converged) → 10/10 in 1.7s. Ramp: 10/25/50/**100** all converge (100/100 in 16s).

## The operability layer (designed + partly built)

The audit kept rediscovering that state management is *implicit and boot-coupled*.
The response is to make it explicit — an **observe/act pair** for a fleet:

|  | Observe | Act |
|--|---------|-----|
| **Node** | `GET /api/health` ✅ (`4514ef4`) | `reproject` ✅ (`0e8d731`) |
| **Fleet** | `GET /api/digests` ✅ + `/api/fleet` (designed) | `verify` ✅ pure fn + `catch-up`/`prune` (designed) |

The elegant invariant held: **`verify` and convergence-health are the same Merkle
digest** — `diff_digests` *is* verify, and equal digests *is* the health signal.
Leaf↔hub digest convergence is proven end-to-end (`37072a7`). Design docs:
`state-management-interface.md`, `node-and-network-health.md`.

## Capture / storage model (dogfooded with real pi-mono)

- pi-mono **persists by default** to `~/.pi/agent/sessions/{cwd}/…`; OpenStory's
  watcher is recursive, so it captures it. Real current pi output translates
  cleanly (promoted to a fixture).
- `pi --no-session` writes **nothing** → OpenStory's watcher-only path is blind.
  A **capture gap**, not durability — the fix is a push integration (task #18).
- **OpenClaw** embeds pi via the SDK; persists by default but to
  `~/.openclaw/agents/<id>/sessions/`, *not* the pi default — so OpenStory's
  documented `pi_watch_dir` example would capture nothing from OpenClaw
  (doc fix, task #17).

## Coverage map

**Reviewed (with fixes):** build/CI image, test instruments, translate
(codex/pi/hermes), ingest perf, boot/reconcile, storage model, capture paths,
node + fleet operability, federation (both directions).

**Still thin / unreviewed:**
- **Bidirectional JetStream replication** — the open finding #9; the lab's core
  premise needs confirming (mirrors/sources/domain vs core propagation).
- **CI is slack** — container/compose/perf tests are `#[ignore]` and the image
  isn't built in CI; Mongo isn't run (`--features mongo` absent). The whole
  integration + parity net is on the honor system. *Highest-leverage cheap fix.*
- **Mongo backend** — fully implemented (32 methods, 0 real `todo!()`; the
  "Phase 2 stub" header is stale) but **never run in CI**.
- **Consumer memory** — `full_payloads`, pattern pipelines, detected patterns
  grow unbounded (no eviction).
- **eval_apply** — 3 documented correctness bugs with characterization tests
  (call_id pairing F-1 is silent corruption on parallel tools).

## Deferred TODOs (open)

- #8/#11 boot perf quantification + Lever 3 (boot O(new work))
- #14 frontier vs recency-fix tension (pre-existing red test — real design conflict)
- #15 `pi_mono_decomposed` raw test (pre-existing red)
- #16 harden SessionStore JSONL backup (swallowed append errors; load-bearing for pi)
- #17 CLAUDE.md `pi_watch_dir` OpenClaw path
- #18 pi push-integration (ephemeral capture)
- #19 leaf tests not parallel-safe (`#[serial]` / `--test-threads=1`)
- #20/#21 build out `verify`/`catch-up`/`prune` + `/api/fleet`
- #22 **confirm the bidirectional-mirror gap** (config vs model)
- CI: build the image, add `--features mongo`, un-`#[ignore]` what fits a runner

## The honest headline

The architecture is sound and the team clearly knows its risks — the
characterization tests and conformance suite prove it. What's missing is
**enforcement**: most "findings" are known truths the automation doesn't defend
yet. The two genuinely new discoveries are the **CPU-invariant ingest wall**
(fixed) and the **federation convergence gaps** (one fixed, one open) — both
found only by testing at scale on real hardware, neither visible from the code.
