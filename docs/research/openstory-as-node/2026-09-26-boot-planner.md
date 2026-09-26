# The boot planner

**Date:** 2026-09-26 · **Status:** plan, loop-ready · **Branch:** `feat/reel-chart-beats-kindle` · **Depends on:** B-11 (durable, bounded consumer start) from `2026-09-25-boot-pass-memory.md` · **Loop prompt:** `docs/prompts/boot-planner-loop.md`

Boot becomes three steps: **inventory** (cheap facts), **plan** (a pure function of inventory and policy), **execute** (idempotent phases with checkpoints, and a debt ledger for what is deferred past serving). An agent reads the inventory, dry-runs plans, chooses one for the next boot, and watches the debt drain. The node stays a mirror: transcripts are only read, boot never deletes, and deferred work can wait but never disappear.

## How efficient boot is today

Measured on the fleet image `sha-368efde`, 2026-09-25/26, all clean restarts where nothing on disk had changed:

| node | reconcile JSONL | boot facts + working set | async replay | before serving | useful work |
|---|---|---|---|---|---|
| laptop | 52 s · 1,141,376 lines from 6.2 GB · 0 added | 150 sessions, 70,102 events | 78 s · 924,568 events from 8.5 GB SQLite | ≈ 2.2 min | 0 events added |
| hub | 55 s · 1,395,934 lines · 0 added | 136 sessions, 65,475 events | 73 s · 750,857 events | ≈ 2.2 min | 0 events added |

Then every consumer actor re-reads its whole stream from the start (today about 1 GB per node, up to 2 GB at the new caps), which on the mini is the step that exceeds its memory.

Every restart reads the whole history from disk twice, once as JSONL and once as SQLite, and then the whole bus window once more, to arrive where it started. Cost is O(total history) three times over; useful work on a clean restart is zero. Before serving, the node needs only the recent window: about 150 sessions and 70 k events, roughly 8 % of the replay. So a clean restart can serve in seconds instead of minutes, and the rest can be proven unchanged instead of re-read.

Where the time goes, and what removes it:

| cost today | removed by |
|---|---|
| reconcile reads every JSONL line | a manifest of each file's size, modified time, byte offset and tail hash: unchanged files are skipped, appended files are read from their offset (BP-01, BP-02) |
| replay folds every session before serving | window-first replay; the rest is built on first access or drained as debt after serving (BP-06) |
| boot facts are queried per session | facts stored on the session row at ingest (BP-07, the B-03 row) |
| reconcile and replay both walk every session | one walk (BP-07, the B-04 row) |
| every consumer re-reads the stream | durable consumers resume from their last acknowledgement, bounded in flight (B-11) |

## Invariants: never optional, under any policy

- **I-1 Read-only transcripts.** Boot opens transcript and JSONL files for reading only. Tests mount them read-only.
- **I-2 No loss.** After boot and a full debt drain, the store's event set equals the union of the pre-boot store and every JSONL file. Boot never deletes a row or a file.
- **I-3 Debt is durable and visible.** Deferred work is appended to `data_dir/boot-debt.jsonl` before serving and survives restarts. While debt exists, health carries it and the verdict names it.
- **I-4 Window guarantee.** Every session whose own last event is inside the serving window is complete in the store and its read model at the moment the phase says serving.
- **I-5 Policy equivalence.** For any starting state, every policy converges, after its debt drains, to the same store set, the same per-session folds, and the same digests as `thorough`.
- **I-6 Idempotence.** Booting twice equals booting once. A boot interrupted at any checkpoint and restarted reaches the same final state as one that was never interrupted, and drains no debt twice.
- **I-7 Determinism of the plan.** `plan(inventory, policy)` is pure: the same inputs give the same steps in the same order.
- **I-8 Bounded memory.** Under a cgroup limit L, peak resident memory stays below 0.75 L for every policy, including during debt drain.
- **I-9 No silent gaps.** If the bus rolled past a consumer's cursor, or a transcript the manifest knows is missing, boot says so as a finding and queues the repair as debt.

## The state space the tests must cover

| dimension | states |
|---|---|
| store | empty · consistent with JSONL · missing sessions JSONL has · holds sessions JSONL lacks (from bus or peers) |
| JSONL files | none · unchanged since manifest · appended tail · new files · rewritten (tail hash differs) · file missing |
| manifest | absent (first boot after upgrade) · consistent · stale · unreadable |
| debt ledger | empty · pending from a previous boot · partly drained |
| durable consumers | absent · behind · at head · deleted mid-boot |
| stream | empty · backlog under budget · backlog many times the budget · rolled past a durable's cursor |
| memory | no cgroup limit · small (512 MB) · large (5 GB) |
| clock | sessions all inside the window · all outside · mixed |
| policy | fast · standard · thorough · repair · a deadline · a custom window |
| interruption | none · killed during reconcile · during replay · during debt drain |

The full product is tens of thousands of cases. The suite covers it in three layers:

- **Pure, exhaustive where cheap.** `manifest::classify` and `boot::plan` are pure, so they are tested over the full product of their own inputs (file-state × manifest-state for classify; inventory summaries × policies for plan) with table-driven BDD specs, and with `proptest` generating inventories for I-5 at plan level and I-7.
- **Integration, pairwise.** A seeded builder (`rs/tests/helpers/boot_state.rs`) constructs any combination of the dimensions in a temp data dir against a scratch `nats-server`. A pairwise generator picks the cases that cover every pair of dimension values at least once (about 60 to 80 cases), and each case asserts I-2, I-4, I-5, I-6 and I-9.
- **Containers, the risky corners.** The production image under a cgroup limit for the states that have failed in the field: clean restart with a full stream, first boot after upgrade, backlog many times the budget, rolled stream, and `docker kill` during each phase. These assert I-1 (read-only mount), I-6 and I-8, and record serve time.

## Rows

Protocol as in `REQUIREMENTS.md`: the acceptance test named in the row is written first as behaviour and committed alone while red, then the code, then the row flips. Loop log below with times from `date -u`.

| id | requirement | acceptance test | status |
|---|---|---|---|
| BP-00 | **State builder.** `rs/tests/helpers/boot_state.rs`: from a seed and a `BootState` value (one choice per dimension above), build a data dir, JSONL files, store, manifest, ledger, stream contents and durable consumers deterministically; plus `pairwise(dimensions) -> Vec<BootState>`. | `mod when_a_boot_state_is_built`: the same seed and state give byte-identical data dirs; every dimension value is observable in the built dir; the pairwise set covers every pair of values (checked by counting pairs) | TODO |
| BP-01 | **Manifest.** `data_dir/manifest.jsonl`, one entry per session file `{path, size, mtime, lines, byte_offset, tail_hash}`, written after a file is reconciled. Pure `manifest::classify(stat, entry) -> Unchanged · Appended{from} · Rewritten · New · Missing`. | `mod when_a_transcript_is_classified`: a table over every stat-by-entry combination, asserting the class and the offset | TODO |
| BP-02 | **Reconcile by manifest.** Only New, Appended and Rewritten files are read; Appended from the stored offset; Missing never deletes anything and raises `transcript_missing`. A missing or unreadable manifest means a full reconcile, then the manifest is written. | `mod when_the_node_restarts_clean`: a counting reader proves 0 JSONL bytes read; `when_a_file_grew`: only the tail's bytes read and its events land; `when_a_file_was_rewritten`: full read, union, nothing removed; `when_a_file_vanished`: store unchanged, finding raised | TODO |
| BP-03 | **Inventory.** `boot::inventory()` gathers the manifest summary, store digest summary, durable consumer positions, stream heads and first sequences, per-host watermarks, pending debt, cgroup limit and free disk. `GET /api/boot/inventory` answers in every phase. | `mod when_inventory_is_taken`: on BP-00 states, each field equals the value the builder planted; the endpoint answers during replay | TODO |
| BP-04 | **Plan.** Pure `boot::plan(inventory, policy) -> Plan { before_serving, debt, estimate { bytes_read, peak_bytes, seconds } }`. Policies: `fast` (1 day), `standard` (7 days), `thorough` (everything before serving), `repair` (full re-read and verify), plus `window_days`, `serve_by` deadline, `debt_bytes_per_sec`. | proptest: I-7 determinism; completeness (`before_serving ∪ debt` covers every required step for every generated inventory); window steps are always `before_serving`; `thorough` has empty debt; estimates are monotone in window size | TODO |
| BP-05 | **Debt ledger.** `data_dir/boot-debt.jsonl`, append-only step records with state; a worker drains after serving at the policy's rate with a checkpoint per step; survives restarts. Health `boot.debt { steps, bytes, oldest }`, finding `boot_debt:<kind>`. | `mod when_debt_is_left_at_serving`: the ledger lists exactly the plan's debt; after a restart it resumes where it stopped; when drained, the finding clears and the ledger records completion | TODO |
| BP-06 | **Window-first replay.** Sessions in the window are replayed before serving; the rest become debt or are built on first access. Health `boot.serving_window { from, sessions }`. | `mod when_the_window_is_one_day`: at the serving flip every in-window session's fold equals its full fold (I-4); an out-of-window session read on demand equals its full fold | TODO |
| BP-07 | **Facts on the row, one walk.** The B-03 and B-04 rows, done here if not already: `parent_session` and `cwd` columns filled at ingest and backfilled by the ledger; replay fills any missing facts as it walks. | the B-03 and B-04 specs from the boot-pass plan, plus: a boot with facts present makes zero boot-fact queries | TODO |
| BP-08 | **Plan for the next boot.** `data_dir/boot-plan.json { policy, window_days, serve_by, debt_bytes_per_sec, author, evidence }` is read at boot, logged, applied once and archived; a malformed file means the default plan and a `boot_plan_invalid` finding. CLI `--boot-policy`. | `mod when_a_plan_file_is_present`: the boot follows it and archives it; `when_it_is_malformed`: default plan, finding, file kept for inspection | TODO |
| BP-09 | **Policy equivalence, the core property.** For each pairwise BP-00 state and every policy: boot, drain debt, then compare with `thorough` on the same state. | integration, real scratch `nats-server`: I-2, I-5 on store sets, per-session folds and roll-up digests; I-1 by hashing every transcript before and after | TODO |
| BP-10 | **Interruption.** For each checkpoint kind, cancel the boot there, restart, drain. | integration: I-6, the final state equals the uninterrupted run and no ledger step ran twice (counted) | TODO |
| BP-11 | **Stream gaps.** If a durable's cursor is older than the stream's first sequence, or its consumer vanished, boot raises `stream_gap:<actor>` and queues catch-up and verify as debt. | integration with a scratch stream rolled past a cursor: finding present, debt queued, after drain the store equals the union (I-9) | TODO |
| BP-12 | **Containers.** The production image under cgroup limits for: clean restart with a full stream, first boot after upgrade (no manifest, no durables), backlog many times the budget, rolled stream, and `docker kill` during reconcile, replay and debt; `fast` and `thorough` each. Transcripts mounted read-only. | `rs/tests/test_boot_planner_containers.rs` under `just test-container`: I-8 (peak under 0.75 L from `memory.stat`), I-6 after each kill, `fast` serves within a bound recorded in the loop log, I-1 by the read-only mount plus hashes | TODO |
| BP-13 | **Hands.** Tier 0: `boot_inventory`, `boot_plan { policy }` (dry run with estimates), `boot_status`, `subscribe_boot` (transition only). Tier 1: `set_boot_plan`, `drain_debt { window · project · session }`, `rebuild_manifest`. Tier 2 stays a proposal: restart with a plan, carrying the dry run as evidence. | `rs/mcp/tests/ops_hands.rs` style specs for each hand; the OPS instructions block names them; tier-1 refuses while not serving except `set_boot_plan`, which only writes the plan file | TODO |
| BP-14 | **Verdict and view.** Findings `boot_debt`, `serving_window`, `transcript_missing`, `stream_gap`, `boot_plan_invalid` in the verdict, the probe script, and `verdictFor`; a Boot panel in the Fleet tab from a pure lib. | Vitest `scenario(given, when, then)` specs in `ui/tests/lib/boot-status.test.ts`; probe `--test`; verdict unit tests | TODO |

## Order

BP-00 first (every other row tests with it). Then BP-01, BP-02, BP-03, BP-04 (the pure core). Then BP-05, BP-06, BP-08 (execution). Then BP-09, BP-10, BP-11 (the properties). Then BP-07, BP-12. Then BP-13, BP-14. B-11 must be green before BP-09.

## Scoreboard

0 of 15 green · Last updated 2026-09-26

## Loop log

- **2026-09-26 · prototype, before any row** — `scripts/boot_assess.py` (18 self-checks), read-only against the laptop's live data (3,233 transcript files, 6.3 GB; store 3,235 sessions). Stat of every file 0.014 s; store session summary 0.03 s; cold manifest build (line count and tail hash of every file) 5.9 s, once; warm classify as at a restart 0.003 s → 3,232 unchanged, 1 appended (the session being written). One-day window, `fast`: 17 KB to reconcile and 68,937 events to replay before serving; 1,097,215 events of replay as debt. Full id verification 21 s (12.7 s to read the store's ids, 8.1 s to extract ids from the JSONL). Findings: 3,383 ids appear in both a parent transcript and its compaction or side-question subagent transcript and are stored once under whichever came first, so BP-09's verification must compare across sessions, not per session; 25 ids in 7 sessions are absent from the store, to be explained by BP-02 or BP-09 before BP-12. Python numbers bound the disk-bound passes and overstate the CPU-bound ones; the row measurements in Rust are the ones that count. BP-01 and BP-04 should reproduce this script's classes and plan on the same inputs.
