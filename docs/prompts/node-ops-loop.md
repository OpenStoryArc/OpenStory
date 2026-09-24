# Prompt: node ops loop

**Use when:** building OpenStory's infrastructure tooling autonomously over a
long session: logging, supervision, health, presence, telemetry, the ops hands
on the MCP, the K3s shape, and DORA measurement.
**Duration:** up to 8 hours, or until every scoreboard row is GREEN or DEFERRED.
**Stack:** worktree `~/projects/openstory-wt-kindle` on branch
`feat/reel-chart-beats-kindle` (or a branch stacked on it), a1 over ssh for
k3s and docker-backed tests, PR #120.
**Law:** observe, never interfere. Nothing writes `events.*`. The MCP never
performs a tier 2 action. The owner's live `:3002` is never restarted.

Invoke with `/loop` and this file's path, or paste the loop prompt below.

---

## Loop prompt (paste verbatim)

You are running the node ops loop for OpenStory. Work only in the worktree at
`/Users/maxglassie/projects/openstory-wt-kindle`. Never edit
`/Users/maxglassie/projects/OpenStory`. Never restart the server on `:3002`,
NATS on `:4222`, or Vite on `:5173`; boot isolated instances with
`scripts/scratch_node.sh` (build it first if L-08 is not GREEN) or use a
testcontainer. Never merge. Push to the branch after every green commit.

Each iteration:

1. Read `docs/research/openstory-as-node/REQUIREMENTS.md`. Take the first row
   whose status is `TODO` or `RED`, in scoreboard order: L, E, H, P, O, M, K,
   D. Respect the dependencies stated in the loop protocol. Say which row you
   are taking and why in one line.
2. Read the design doc `docs/superpowers/specs/2026-09-23-node-ops-design.md`
   section for that group, and the audit
   `docs/research/openstory-as-node/2026-09-23-logging-and-ops-hands.md` for
   the file and line evidence. Read the code you will change before changing it.
3. Write the acceptance test named in the row, exactly, as behaviour
   (`mod when_x { fn it_y }` in Rust, `describe/it` in TypeScript,
   `test_when_x_it_y` in Python). Run it. It must fail for the right reason.
   Mark the row `RED` and commit the test alone
   (`test(<crate>): <id> red`).
4. Write the minimal code. Run the test, then the touched crate's full test
   suite, then `cargo clippy -p <crate> --all-targets -- -D warnings`
   (a pre-existing `large_enum_variant` on the CLI `Command` enum is known;
   fix it if you touch `main.rs`, otherwise leave it). For UI work run
   `npx vitest run` for the touched spec files and `npx tsc --noEmit`.
   Use `CARGO_TARGET_DIR=/Users/maxglassie/projects/OpenStory/rs/target` for
   Rust so builds stay warm.
5. Mark the row `GREEN`, update the scoreboard counts and the "Last updated"
   line, and commit code plus scoreboard together with the id in the first
   line and a Problem / Solution / Test coverage body. Push.
6. After every H, P, or M row: run
   `python3 scripts/node_health_probe.py --config
   /Users/maxglassie/projects/OpenStory/data/config.toml --data-dir
   /Users/maxglassie/projects/OpenStory/data` against the owner's live node
   (read only) and paste the verdict line into the commit body. If the probe
   itself needs a new field the row added, extend the probe in the same
   commit and keep its `--test` green.
7. For K rows: ssh to `a1` (already reachable, has k3s, kubectl, docker,
   32 cores). Use only namespaces prefixed `os-loop-`. Build the image on a1
   from a pushed commit (`git clone` or `git fetch` of the branch into
   `~/os-loop/` on a1), never by copying the worktree. Tear the namespace down
   at the end of the task even on failure. Never touch the hub
   (`debian-16gb-ash-1`) or a1's existing services.
8. If a row turns out wrong or impossible in this codebase, mark it
   `DEFERRED` with one sentence why, in the same commit, and move on. Do not
   redesign; the design doc is the spec. If the design doc is wrong, change
   the doc in the same commit and say what and why.
9. Every 90 minutes, or after each group completes, write a short progress
   note at the bottom of the REQUIREMENTS file under "## Loop log"
   (timestamp, rows flipped, anything the owner must decide), commit, push.
10. Stop when every row is GREEN or DEFERRED, when 8 hours have elapsed, or
    when you have failed the same row three times. On stopping, write the
    final loop log entry, push, and report: rows flipped, the live probe's
    last verdict, what the owner must decide, and the PR link.

Constraints that never bend: no new logging or metrics vendor (tracing,
tracing-subscriber, opentelemetry crates only); no real session data in
fixtures; no real names of people in any file; no `let _ =` on a fallible
persist, publish, append, or index; every test asserts values, not presence;
three clear lines beat a helper; do not build what no row asks for.

---

## Preconditions

```bash
cd /Users/maxglassie/projects/openstory-wt-kindle && git status --short | head
ls ui/node_modules e2e/node_modules            # symlinks to the main checkout
ssh -o BatchMode=yes -o ConnectTimeout=5 a1 'k3s --version; kubectl get ns'
python3 scripts/node_health_probe.py --test    # 32 assertions
```

If the worktree is missing: `git worktree add ~/projects/openstory-wt-kindle feat/reel-chart-beats-kindle` from the main checkout, then symlink `ui/node_modules` and `e2e/node_modules` from the main checkout.

## What the owner decides, not the loop

- Merging PR #120 or anything stacked on it.
- Bringing the hub back (Hetzner console). The loop reads leaf state; it never
  changes `nats_leaf_url`.
- Raising the events stream cap on the live node (three July PRs propose values).
- Enabling OTLP export to any external endpoint.
