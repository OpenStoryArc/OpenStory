---
name: check-docs
description: Validate the project's docs against the live codebase. Catches drift between claims (in markdown prose) and reality (in Cargo.toml workspace members, source files, the filesystem). Use before committing docs changes, or whenever you're about to write something architectural and want to be sure you're not perpetuating a stale claim. The validator surfaces things internal consistency cannot catch — when every doc agrees with every other doc but none agree with the build.
---

# check-docs

A TDD-style validator for the OpenStory documentation. The script collects facts from the live codebase and compares them to assertions extracted from the docs. Failing assertions are the spec for what to fix.

## Usage

```bash
python3 scripts/check_docs.py            # validate the repo, exit non-zero on failure
python3 scripts/check_docs.py --quiet    # only print failures + summary
python3 scripts/check_docs.py --test     # self-tests on synthetic fixtures
```

The script reads the workspace structure (`rs/Cargo.toml` members, `rs/patterns/src/*.rs` files, `rs/server/src/consumers/*.rs` files, the filesystem) and validates that the markdown files in `docs/`, `CLAUDE.md`, and `README.md` are telling the truth about it.

## What it checks (current assertions)

| Check | What it asserts |
|---|---|
| `no_merge_markers` | No tracked markdown file contains `<<<<<<<`, `=======`, or `>>>>>>>` lines |
| `pattern_detector_count` | Docs claiming N detectors must match the actual count of files in `rs/patterns/src/` (excluding `lib.rs`) |
| `crate_count` | Docs claiming N crates must match the workspace `members` array in `rs/Cargo.toml` |
| `consumer_count` | Docs claiming N consumers must match the file count in `rs/server/src/consumers/` (excluding `mod.rs`) |
| `tour_mentions_nats` | `architecture-tour.md` must reference NATS (the actual event bus) |
| `tour_mentions_consumers` | `architecture-tour.md` must reference the `consumers/` directory |
| `soul_arch_mentions_nats` | `soul/architecture.md` pipeline must mention NATS |
| `no_phantom_crates` | If a directory under `rs/` has a `Cargo.toml` but isn't a workspace member, no doc may list it as a real crate without an explanation (vestigial / orphan / not a workspace member) |
| `use_case_4_no_ingest_events_fanout` | `soul/use-cases.md` Use Case 4 must not still claim `ingest_events()` is the fan-out point |
| `referenced_scripts_exist` | Every `scripts/foo.py` referenced in docs must actually exist (skips `BACKLOG.md` because it tracks future scripts; skips placeholder names like `foo.py`, `bar.py`) |
| `readme_mentions_sessionstory` | `README.md` must reference `sessionstory.py` |
| `claude_md_mentions_sessionstory` | `CLAUDE.md` must reference `sessionstory.py` |
| `tour_references_sessionstory` | `architecture-tour.md` must reference `sessionstory.py` |

## How to use this skill

**Before editing docs** that touch the architecture, crate layout, detector list, consumer list, or any code reference:

1. Run `python3 scripts/check_docs.py` to see the current state
2. Make your edits
3. Re-run the validator
4. Address any new failures before committing

**When you find a stale claim that the validator doesn't catch:**

1. Add a new check function to `scripts/check_docs.py` (each is ~10 lines)
2. Add it to the `CHECKS` list
3. Extend the synthetic fixture in `selftest()` to verify it fires correctly
4. Run `--test` to confirm the self-tests still pass
5. Run the validator for real to see if any docs now fail

**When the codebase changes** (a new crate, a renamed detector, a new consumer):

1. The validator will fail on the next run, telling you which docs lie about the new state
2. Update the docs
3. Re-run until green

## Why this skill exists

OpenStory's soul is **giving humans visibility into what their agents are actually doing**, not what the agent claims. Applied inward, the same principle says: **give the project visibility into what its codebase actually is, not what its docs claim.** Internal consistency is not truth — when four files all say "9 crates" and the build only has 8, every file is internally consistent and every file is wrong. Mechanical comparison against the source of truth (`rs/Cargo.toml`, the filesystem) is the only way to catch that.

This is the **claim-vs-reality pattern**: when a fact is stated in two places — once as a claim in prose, once as a reality in code — they can drift, and the drift is invisible because each side is internally coherent. Mechanical checks comparing the two sides surface the drift before it metastasizes. See `docs/soul/patterns.md` for the longer rationale.

## When NOT to use this skill

- For validating that *new* code works — run `cargo test` and `cd ui && npm test` instead
- For validating that scripts work — they have their own `--test` flags
- For validating runtime behavior — use the REST API, the WebSocket, or `scripts/sessionstory.py`
- For checking code style — `cargo clippy`, `cargo fmt`, ESLint

The validator is *only* for the doc-vs-code consistency surface. Don't bolt unrelated checks onto it.

## Adjacent scripts

- `scripts/sessionstory.py` — the same shape applied to *session* data (fact collector + skill)
- `scripts/analyze_*.py` — narrower structure analyses, also consumed by the model

If you find yourself wanting another mechanical "did the docs/code agree?" check, the right move is a new sibling script (`scripts/check_api.py`, `scripts/check_config.py`, …) — not a unified `check.py`. Keep them small, independent, and `--test`-able.
