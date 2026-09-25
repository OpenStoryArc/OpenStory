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

- **2026-09-25 14:59 UTC.** C-01 GREEN. Red `3f622aa` (test(server): C-01 red, digest
  roll-up specs), green: this commit. `fleet::rollup` folds `PlacedDigest`
  rows (host, project, session, count, digest) to project, host, and root,
  each level FNV-1a over `key␟digest` pairs in key order;
  `fleet::differing_projects` names the (host, project) pairs two roll-ups
  disagree on. `EventStore::session_event_ids` (default over
  `session_events`, SQLite reads the primary key only) feeds
  `catch_up::placed_digests`, which `/api/digests`, `?rollup=1`, and the
  health body share. The health body carries `rollup` recomputed only when
  the sessions table moves (rows, event sum, newest last_event, keyed by
  data dir), so a 15 s beat on a fleet store does not re-read the store.
  Measured: 5 specs; a reversed fleet folds to the same digest, one added
  event moves exactly the project, host, and root above it. Note:
  `test_health::when_leaf_is_configured_but_down` fails on this machine
  before and after the change because the live node's NATS monitor answers
  on :8222 (pid 8953) and reports a real leaf link; not touched.
- **2026-09-25 15:09 UTC.** C-03 GREEN. Red `66f02cd`, green: this commit.
  `consistency::Snapshot` (host, rollup, watermarks, verify, consumer
  lags, stale, age) reads from a health body or a fleet-presence node;
  `consistency::report(local, peers, interval_secs)` answers in the
  verdict's shape. Thresholds chosen where the plan left them open:
  `diverged` is critical when more than half of the union of (host,
  project) pairs differ, warn otherwise; `behind` when a peer's watermark
  for us trails ours by more than two beats; `lag` past one queued batch.
  `GET /api/consistency` compares the health body against every other
  node's latest beat (our own beat is never a peer); MCP
  `consistency_report {}` (tier 0, diagnose) reads it, and the OPS
  block names the five finding ids. Adjustment against the plan: the
  Snapshot's `watermarks` are RFC 3339 times per origin host, not bus
  sequences, because the persist consumer never sees a subject or a
  sequence (ephemeral push consumers, acked in the bus's pump task before
  persistence); C-02 fills them from the sessions table. Measured: 8 pure
  specs, one API spec, two MCP specs; a peer sharing our one project and
  holding one more reads warn on 1 of 2, a peer sharing nothing reads
  critical.
- C-01 gate note: `just test` on this machine fails only in
  `test_health::when_leaf_is_configured_but_down` (the live node's NATS
  monitor on :8222 reports a real leaf link; environmental, untouched); the
  remaining recipe steps (four audits, workspace clippy, 2082 UI specs)
  were run by hand and are green.
- **2026-09-25 15:16 UTC.** C-02 GREEN. Red `d9f83fa`, green: this commit.
  `fleet::watermarks(rows)` folds the sessions table to the newest
  `last_event` per origin host (MAX on upsert keeps it monotone); the
  health body and so the beat carry `watermarks: {host: time}`. What the
  plan got wrong: it asked for the persist consumer's acknowledged NATS
  sequence per host and said a restart resumes from it under durable
  consumer semantics; in the code the bus's pump task acks every message
  before the consumer sees it, consumers are ephemeral
  (`DeliverPolicy::All`), and `IngestBatch` carries neither subject nor
  sequence. The sessions row is the persist consumer's acknowledged
  position (written after the events are durable), so the watermark is
  read from the store and survives a restart by construction. Measured:
  two hosts' batches, the newer event arriving before the older, give
  `node-a = 10:00:05`, `node-b = 09:00:00`; a fresh AppState over the same
  data dir answers identically; the beat and the consistency Snapshot
  carry the same map.
- **2026-09-25 15:21 UTC.** C-04 GREEN. Red `6a3357a`, green: this commit.
  `stdio::handle_subscribe_pump` is the one pump behind `subscribe_health`
  and `subscribe_convergence`, parameterised by what it reads
  (`ops::read_verdict` / `ops::read_report`), the key the body rides
  under, and the notification method; `ops::transition(prev, next, key)`
  is the health transition the plan named, with `health_transition` kept
  as its `verdict` case. An unreachable node reads as
  `consistency_unreachable`, so an outage is itself a transition. Measured:
  a mock that answers diverged twice then agreed yields exactly one
  `notifications/openstory/convergence` (`from: warn, to: ok, cleared:
  [diverged:node-b], seq: 1`) and 400 ms of silence after, while polling
  continued (4+ reads).
- **2026-09-25 15:32 UTC.** C-05 GREEN. Red `6634fee`, green: this commit. `POST
  /api/ops/converge {peers?, max_rounds? (3), settle_ms? (250)}` and MCP
  `node_converge`: per round reproject stale (rows with no resident
  projection), `catch_up::catch_up_report` against each peer, prune per
  `retention_days`, settle, verify the sessions the run touched, compare
  root digests; stops when verify agrees and every peer's roll-up matches,
  or when a round changed nothing. Peers: given, else
  `OPEN_STORY_CATCH_UP_PEER`, else every other node's beat that carries
  `api_url` (new `advertise_url` config, `OPEN_STORY_ADVERTISE_URL`);
  beats without one are `unknown_peers`, reported not attempted; self is
  never a peer. What the plan got wrong, found by the two-node spec: the
  sets converged but the roll-ups did not, because catch-up batches carried
  no `project_id`, so a pulled session placed under `unknown` on the
  puller and `proj-1` at its origin. Placement is derived metadata the
  union must carry: `/api/digests` rows now serve host and project, and
  catch-up puts the origin's project on the envelope (the existing path,
  filled in). Catch-up also re-injects only the ids the puller lacks, so a
  round that finds nothing new heals nothing and the loop can see it.
  Measured: partition healed, converge on A heals 2 sessions (4 events)
  in round 1 and stops at round 2; converge on B reaches equal roll-ups
  in 1 round; a third call reports `changed: false, converged: true` in 1
  round; the same key replays; every `events.*` publish on A is its own
  history or a session B holds. `scripts/subject_publishers.py` green.
- Gate note: the UI bench `timeline-bench > toTimelineRows < 15ms`
  now fails on this machine even idle (load average 50, another worktree
  building); zero UI files changed on this branch.
