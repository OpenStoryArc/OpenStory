# Prompt: boot planner loop

**Use when:** building the boot planner for OpenStory nodes: the manifest, the pure plan function, the debt ledger, window-first replay, the equivalence and interruption properties, the container gate, and the boot hands on the MCP.
**Duration:** up to 8 hours, or until every row in the plan is GREEN or DEFERRED.
**Stack:** worktree `~/projects/openstory-wt-kindle` on branch `feat/reel-chart-beats-kindle`, Docker for testcontainers, a scratch `nats-server` for integration tests.
**Law:** observe, never interfere. Transcripts are only read. Boot never deletes. Deferred work is never dropped. The owner's live `:3002` and `:4222` are never restarted.

Invoke with `/loop` and this file's path, or paste the loop prompt below.

---

## Loop prompt (paste verbatim)

You are running the boot planner loop for OpenStory. Work only in the worktree at `/Users/maxglassie/projects/openstory-wt-kindle` on branch `feat/reel-chart-beats-kindle`; check `git branch --show-current` before every commit. Never edit `/Users/maxglassie/projects/OpenStory`. Never restart the server on `:3002`, NATS on `:4222`, or Vite on `:5173`. Never ssh to any node. Never merge. Push after every green commit.

Each iteration:

1. Read `docs/research/openstory-as-node/2026-09-26-boot-planner.md` in full: the invariants, the state space, the rows, the order. Read the B-11 row in `docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md`. If B-11 is not GREEN, do not start BP-09 or later; work the earlier rows. Take the first row whose status is `TODO` or `RED` in the plan's order. Say which row and why in one line.
2. Read the code the row touches before changing it: `rs/server/src/state.rs` (boot), `rs/server/src/reconcile.rs`, `rs/server/src/ingest.rs` (`replay_boot_sessions`), `rs/server/src/boot.rs` (phase), `rs/store/src/*`, `rs/bus/src/nats_bus.rs`, `rs/mcp/src/tools/ops.rs`, and the tests the row names.
3. Write the acceptance test exactly as named, as behaviour (`mod when_x { fn it_y }` in Rust; `scenario(given, when, then)` from `ui/tests/bdd.ts` in TypeScript). Assert values, never presence. Run it; it must fail for the right reason. Mark the row `RED`, commit the test alone (`test(<crate>): <id> red`).
4. Write the minimal code. Run the row's test, the touched crates' suites, and `cargo clippy --workspace -- -D warnings` with `CARGO_TARGET_DIR=/Users/maxglassie/projects/OpenStory/rs/target`. For UI: `npx vitest run <spec>` and `npx tsc --noEmit`. For container rows: `just docker-build` then `just test-container`.
5. Mark the row `GREEN`, update the scoreboard, commit code and plan together with the id in the first line and a Problem / Solution / Test coverage body. Push.
6. After BP-02, BP-06 and BP-12, measure: run `just boot-memory --docker` on the standard fixture and record reconcile bytes read, time to serving, and peak memory in the loop log next to the "How efficient boot is today" table.
7. Integration tests use a scratch `/opt/homebrew/bin/nats-server` on ports above 4800 (client) and 8800 (monitor), checked free with `lsof -ti:PORT`, data under `/private/tmp/claude-501/-Users-maxglassie-projects-OpenStory/8a02ac03-3bda-485d-8a11-d943cb20cf81/scratchpad/boot-planner/`. Containers are named `bp-*` and removed after. Never start or stop Docker Desktop.
8. If a row is wrong or impossible against the code, mark it `DEFERRED` with one sentence why, or change the plan and say what and why, in the same commit. Do not weaken an invariant to make a row pass; if an invariant cannot hold, stop and say so in the loop log.
9. After every three rows, or every 90 minutes, write a loop log entry (time from `date -u`, rows flipped, measurements, anything the owner must decide), commit, push.
10. Stop when every row is GREEN or DEFERRED, after 8 hours, or after failing the same row three times. Then write the final loop log entry, push, and report: rows flipped, the before-and-after boot numbers, and what the owner must decide.

Known environmental failures on this machine, not yours to fix: `test_health::when_leaf_is_configured_but_down::it_reports_not_connected` (the live NATS answers `:8222`), UI timing benches under load, `pi_mono_session_metadata_persisted` under load.

Constraints that never bend: no real session data in fixtures; no real names of people in any file; no `let _ =` on a fallible write; three clear lines beat a helper; do not build what no row asks for; commits end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

---

## Preconditions

```bash
cd /Users/maxglassie/projects/openstory-wt-kindle && git branch --show-current && git status --short | head
docker info >/dev/null && echo docker ok
/opt/homebrew/bin/nats-server --version
python3 scripts/boot_memory.py --test
```

## What the owner decides, not the loop

- Rolling any new image to a node, and which boot policy each node runs by default.
- Merging the branch.
- The serving window and debt pace for the hub, which serves the whole fleet.
