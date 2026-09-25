# The boot pass, bounded

**Date:** 2026-09-25 · **Status:** plan, ready to work · **Branch:** `feat/reel-chart-beats-kindle` (or a branch stacked on it) · **Owner gate:** nothing rolls to a node until B-00 is green.

## Why now

The node loads every session's full event list at boot, one session at a time, to read two small fields: the parent session of a subagent (`data.session_id` on any of its events) and the working directory (the first event that carries a `cwd`). On a fleet store of 3050 sessions that pass alone drives anonymous memory to about 3.4 GB before the async replay starts; replay and the live consumer actors add the rest. Tonight the hub died at 4 GB, then at 5 GB, and the owner's mini died at 2 GB and again at 3 GB on the same image. Every earlier round bounded what the node keeps or raised what the box gives, and none of them changed how the node reads.

The record (via `agent_search` on the local node, 2026-09-25):

| when | where | what was said | what was done |
|---|---|---|---|
| 2026-04 | co-founder's workspace | "boot replay loading 241 sessions / 44669 events into memory at once" · backlog entry *Streaming Boot Replay (bounded-memory)*: "the server loads its entire event history into memory at boot" | entry never reached master; async replay landed instead |
| 2026-04-16 | branch triage | "cache could balloon mid-replay. There's no eviction… if that ever exceeds RAM, OOM" | noted |
| 2026-07-08 | openstory-deploy `4c7381a` | node "sat pinned at 99% of 2G" | node 2G → 4G, NATS 512M → 4G |
| 2026-07-10..13 | `cecba5c` … `12fecae` | bounded read-through projection: ProjectionCache, PayloadCache, pins, gauges | bounds what is kept after boot, not what is loaded during boot |
| 2026-07-12 | owner session | "it's back at 93% memory, the recurrence risk is real" | memory file updated |
| 2026-09-25 | this loop | cache bound 1.5G, limit 5G, PR for 8G, resize discussed | the boot pass read, this plan |

## Facts the plan stands on

- `boot_from_sqlite` (`rs/server/src/state.rs`) calls `event_store.session_events(&row.id)` for every session row and feeds the whole list to `detect_subagent_relationship` (reads `data.session_id`) and `extract_cwd_from_events` (`find_map` over events for a `cwd`). Both answers come from the first few events in practice.
- `replay_boot_sessions` (`rs/server/src/ingest.rs`) walks every session again with full events to fill projections and session rows.
- The sessions table already carries `project_id`, `project_name`, `label`, `custom_label`, `branch`, `event_count`, `first_event`, `last_event`, `host`, `user`, `origin_agent`; columns are added with idempotent `ALTER TABLE` (`rs/store/src/sqlite_store.rs`). Mongo mirrors the row in `upsert_session` (`rs/store/src/mongo_store.rs`) and already extracts `parent_uuid` per event.
- The boot phase lives in `rs/server/src/boot.rs` (`Phase`, `set_serving()`); `/api/health` is 503 with a body until serving.
- Consumer actors are supervised and start at boot; on the hub they drained a 13k-message backlog with 1000 events in flight each while replay ran.
- cgroup `memory.stat` on the hub during replay: anon 3.98 GB, file 1.0 GB at 703 of 3050 sessions. The bytes are heap, not page cache, and not inside the bounded caches.
- The mini's store: 2.97 GB SQLite, 1329 JSONL files, 1259 sessions. Old image steady state 1.36 GB; this image reaches serving at 2.99 GB and is killed.

## Rows

Same protocol as `REQUIREMENTS.md`: red spec committed alone, then the code; the gate runs `just test`; the loop log takes its times from `date`.

| id | requirement | red spec |
|---|---|---|
| B-00 | **A boot-memory harness.** `scripts/boot_memory.py` builds a synthetic store (N sessions; a tail of large sessions with 8k events and 80 KB tool outputs, shaped like the fleet's agent sessions), boots `open-story serve` against it on a free port, samples RSS every second until `/api/health` says serving, and prints peak RSS per phase (reconcile, boot pass, replay, serving). `--test` covers the parser and the phase split. Gate for the branch: **1259-session fixture, peak under 2 GB**. Real-data validation on the mini's store copy is an owner-run step, not CI. | script `--test`; a `just boot-memory` recipe; the number in the loop log |
| B-01 | **Ask the store for the two facts.** `EventStore::session_boot_facts(session_id) -> BootFacts { parent_session: Option<String>, cwd: Option<String> }`. SQLite: one query with `json_extract` over the first 32 events by time. Mongo: same with a projection and a limit. No event bodies cross the trait. | conformance helper on both backends: a session whose parent link and cwd sit in events 1 and 3 answers correctly; a session with neither answers `None`; a 5000-event session answers in under 5 ms |
| B-02 | **The boot pass uses B-01 and nothing else.** `boot_from_sqlite` never calls `session_events`. | a recording `EventStore` wrapper asserts zero `session_events` calls during `AppState` boot; the existing subagent-parent and project-resolution tests stay green |
| B-03 | **Stop recomputing.** Sessions rows gain `parent_session` and `cwd`, filled by the persist consumer from the first batch that carries them; the boot pass reads rows only; `open-story reconcile` backfills old rows once via B-01. Idempotent `ALTER TABLE` on SQLite; field on the Mongo document. | conformance: upsert then read back on both backends; reconcile on a store with empty columns fills them; boot with filled columns makes zero B-01 calls |
| B-04 | **One walk, not two.** `replay_boot_sessions` fills any missing boot facts as it walks, so a store with empty columns costs one read, not two. | test: a store with no columns filled boots with exactly one `session_events` per session |
| B-05 | **Consumers start at serving.** Config `consumers_start = "serving"` (default) or `"boot"`. The supervisors hold until `set_serving()`; health shows `consumers.<name>.state = pending_start` until then; the verdict does not raise `consumer_dead` for a pending actor. | test: with the default, no consumer subscribes before the phase flips; with `"boot"`, behavior is today's; health body and verdict cover `pending_start` |
| B-06 | **Give memory back.** `tikv-jemallocator` behind a default-on feature in `open-story-cli`, `background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000`; the plain-glibc build stays available. | B-00 peak and settle-after-serving numbers with and without the feature, in the loop log; the settle number must fall |
| B-07 | **Say it before the kernel does.** Health gains `process.memory_limit_bytes` (cgroup v2 `memory.max`, else null) and the verdict raises `memory_pressure` (warn at 75 %, critical at 90 % of the limit). The probe script and the dashboard's verdict match. | pure `verdict` test on a body with rss 4.6 G and limit 5 G → critical `memory_pressure`; probe script test; UI `verdictFor` test |
| B-08 | **Roll only behind the gate.** Order: mini (compose, owner's box), hub (openstory-deploy workflow), a1. Each roll: B-00 number on the branch is green; the node reaches serving under its limit; `fleet_presence` shows it beating; one `node_verify` agrees. PR 4 (8G) closes unmerged. PR 5 (node compose cache lines) merges only if B-03 still wants the knobs. | loop log entries with `date` times and the health verdict at serving for each node |

## Order and size

1. **B-00** first, red: the number that fails today (the fixture will pass 2 GB on this branch). Half a day.
2. **B-01 + B-02**: the morning fix. The hub's 3.4 GB pre-replay plateau goes away.
3. **B-05 + B-06 + B-07**: an afternoon. Deterministic boot, memory returned, and the verdict names pressure before the kernel does.
4. **B-03 + B-04**: the next day. The right shape: derived, on disk, rebuildable, one walk.
5. **B-08**: only when B-00 reads green on the branch.

## Tonight, before any of it

The owner rolls the mini back to `sha-f0a22d1` and either rolls the hub back the same way or lets it loop until B-08. Neither is a code change and neither is the agent's to run.

## What this does not do

It does not change what is observed, persisted, or replayed. Every row reads less or reads later; nothing reads differently. `events.*` and `local.*` are untouched. The caches from July stay as they are.
