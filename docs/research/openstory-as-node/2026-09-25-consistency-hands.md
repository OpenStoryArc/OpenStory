# Consistency hands

**Date:** 2026-09-25 · **Status:** building · **Branch:** `feat/consistency-hands` (stacked on `feat/reel-chart-beats-kindle`) · **Worktree:** `~/projects/openstory-wt-consistency`

## The claim

The store is a grow-only set of immutable events keyed by id, deduplicated on insert. The merge of two nodes' stores is set union: commutative, associative, idempotent. Everything else is a fold over that set. So consistency has a precise, checkable meaning: two nodes agree when their sets are equal; a node is internally consistent when its derived state equals the fold of its set. The checks are pure functions over snapshots. The repairs are unions and re-folds, keyed so running them twice is the same as once. Nothing rewrites an observed event.

## What exists

- `rs/server/src/fleet.rs`: `SessionDigest { session_id, count, digest }`, `digest_event_ids` (hash of sorted ids, arrival-order independent), `diff_digests(local, remote) -> FleetDiff`.
- `GET /api/digests` (`api.rs::session_digests`) serves the node's digests; `catch_up_once` (`catch_up.rs`) diffs against a peer and pulls whole sessions it lacks; `POST /api/ops/catch_up` wraps it with a proposal and an idempotency key.
- `POST /api/ops/verify` compares store events to JSONL lines and reports unindexed FTS documents.
- Consumer supervision reports `lag` per actor (`consumers/supervision.rs`); the bus reports stream and source lag (`bus/src/nats_bus.rs`).
- Presence beats carry the whole health body every 15 s (`presence.rs`); the presence actor keeps the latest beat per node.
- `rs/tests/helpers/recording_bus.rs`: an in-memory bus for tests; the node-ops rows used it to prove publish paths.
- The tier rule and the MCP ops hands (`rs/mcp/src/tools/ops.rs`): tier 0 reads, tier 1 proposes then acts on derived state, tier 2 is a proposal only.

## Rows

Protocol as in `REQUIREMENTS.md`: red spec committed alone, then code, `just test` before push, loop log entries with times from `date -u`. Findings use the verdict vocabulary so a person, the dashboard, and a model read them the same way.

| id | requirement | red spec |
|---|---|---|
| C-01 | **Roll-ups.** `fleet::rollup(digests) -> DigestRollup { hosts: {host: {projects: {project: {sessions, events, digest}}, digest}}, digest }`, pure, a hash of child digests in sorted key order. `GET /api/digests?rollup=1` serves it. The presence beat carries `rollup.digest` and the per-host digests (not per-session rows) so any node can read the fleet's convergence state from its own presence store. | pure tests: same set in any order → same roll-up; one event added → the host, project, and root digests all change; two nodes' roll-ups from equal sets are equal. API test for the query flag. Beat test: the body carries `rollup`. |
| C-02 | **Watermarks.** Each node records, per origin host, the highest bus sequence it has persisted (from the persist consumer's acknowledged position, keyed by the subject's host segment). Health and the beat carry `watermarks: {host: seq}`. | test: after ingesting batches from two hosts through the recording bus, the watermark per host equals the last sequence acknowledged; a restart resumes from it (the existing durable-consumer semantics) |
| C-03 | **The report, pure.** `consistency::report(local: Snapshot, peers: [Snapshot]) -> Verdict` where a Snapshot is `{host, rollup, watermarks, verify: {agree, fts_unindexed}, consumers: {name: lag}}` taken from health and presence bodies. Findings: `diverged:<host>` (roll-up digests differ; text names the number of projects that differ), `behind:<host>` (a peer's watermark for us is below our own sequence, text gives the gap), `lag:<consumer>` (ack floor behind stream tail by more than one batch), `unverified` (verify does not agree), `stale_snapshot:<host>` (peer beat older than three intervals). Level: critical for `unverified` and `diverged` past a threshold, warn otherwise. Served at `GET /api/consistency` and as the MCP hand `consistency_report {}` (tier 0). | pure tests on hand-built snapshots for each finding and for the all-clear; the API test; the MCP schema test in the ops-hands style |
| C-04 | **Watch.** `subscribe_convergence { interval_secs? }`: the `subscribe_health` pump pointed at `/api/consistency`, speaking only on transitions (`from, to, added, cleared, seq`). | the transition function is the one health already uses; test that a diverged→agreed edge emits once and silence follows |
| C-05 | **Converge, tier 1.** `POST /api/ops/converge { peers?, max_rounds? (default 3) }` and the MCP hand `node_converge`: for each round, reproject stale, catch up against each peer, prune per retention, verify; stop when verify agrees and the roll-up matches every peer, or when a round changes nothing. Proposal `ops.proposal.converge` before, `ops.command.converge` after, one idempotency key for the whole run, a per-round record in the result. Refuses while not serving. Reads peers from the presence store when none are given (a peer is reachable only if its API URL is known; unknown peers are reported, not attempted). | tests with the recording bus and two in-memory nodes: a partition is injected, events land on both sides, the partition heals, `converge` on each node reaches equal roll-ups within two rounds, a third call changes nothing and reports `replayed: false, changed: false`; the ops-hands tests cover refusal while replaying and the proposal-first order |
| C-06 | **The property.** A test that replays a recorded JSONL sample through two in-memory nodes with a fake clock and an injected partition, then heals and converges: after convergence, roll-ups are equal, projections for every session are equal (fold determinism), and no `events.*` publish came from any hand (the source audit from G-01 stays green). | the test itself, plus `scripts/subject_publishers.py` remaining green |
| C-07 | **The map.** The Fleet tab in the dashboard shows each host's beat age, build, verdict, and the consistency findings against this node, from `/api/fleet/presence` and `/api/consistency`. Pure view functions in `ui/src/lib`, specs in `ui/tests`. | Vitest specs for the pure functions; a component spec that renders two hosts with one diverged |

## Order

C-01 and C-03 first: the roll-up and the report are the spine, and the report can be computed from what health and presence already carry, with `watermarks` empty until C-02. Then C-02, C-04, C-05, C-06, C-07.

## What this does not do

No hand writes to `events.*` or `local.*`. Converge is a union and a re-fold. There is no conflict resolution because there are no conflicts. Nothing here restarts anything.

## Loop log
