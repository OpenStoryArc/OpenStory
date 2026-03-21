# Contributing to Open Story

## What this project believes

Open Story gives humans full visibility into AI coding agent behavior, in real time. It watches but never interferes — a mirror, not a leash. The data is yours, in open formats, portable and unencumbered.

Read [docs/soul/philosophy.md](docs/soul/philosophy.md) before contributing. Then read [docs/soul/use-cases.md](docs/soul/use-cases.md) to see each principle demonstrated in real code. These are more important than the code.

## Non-negotiables

These are hard boundaries, not suggestions. PRs that violate them will be declined.

1. **Observe, never interfere** — No features that write back to the agent, modify transcripts, or block execution
2. **BDD** — No production code without a failing spec first
3. **Functional purity in core** — Side effects at the edges only. Core crates (core, views, patterns) have no I/O dependencies.
4. **Open formats** — CloudEvents 1.0, JSONL, Markdown. No proprietary formats.
5. **Atomic documentation** — Code and docs change together in the same commit

## Mistakes we've already made

- Built a tree UI for data that's a linked list. Deleted it.
- Set a truncation threshold without measuring payloads. It made payloads larger.
- Merged live and stored data in one view. Got "partially both, fully neither."
- Wrote lazy-loading for 2000 records that render in milliseconds. Unnecessary.
- Assumed `parent_uuid` meant tree structure. It doesn't — it's a sequential chain.

Full list with context: [docs/soul/patterns.md](docs/soul/patterns.md)

## Documentation is a snapshot, not an artifact

When you change code, the documentation that describes it changes in the same commit. Not in a follow-up PR. Not "later." In the same commit.

This applies to:
- **Use cases** (`docs/soul/use-cases.md`): If your change touches code referenced by a use case, update the file path, line numbers, and description to match the new state.
- **CLAUDE.md**: If you add a config field, add it to the config table. If you add a crate, add it to the project structure. If you add an API endpoint, add it to the architecture section.
- **Stories and plans**: If you complete work described by a story or plan, update its status to Completed with the date.

The principle: docs and code are one atom. You swap them together or not at all. A PR that changes code without updating the docs it invalidates is incomplete.

## Before your first PR

1. Read [docs/soul/philosophy.md](docs/soul/philosophy.md) — understand what this project believes
2. Read [docs/soul/use-cases.md](docs/soul/use-cases.md) — see each principle in real code
3. Read [docs/soul/patterns.md](docs/soul/patterns.md) — learn the mistakes we've already made
4. Walk through [docs/architecture-tour.md](docs/architecture-tour.md) for large changes

## How we build

### Artifact flow

Every feature follows: **Story → Plan → Implementation**.

- **Stories** (`docs/stories/`) describe *what* and *why* in user terms
- **Plans** (`docs/backlog/`) describe *how* in technical terms, with phasing and file references
- **Implementation** is code + BDD specs

### Development workflow (BDD)

1. **Write a failing test** that describes the expected behavior
2. **Run the test** to confirm it fails
3. **Implement the minimum code** to make it pass
4. **Refactor** if needed, keeping tests green

No production code without a failing spec first. Tests verify *correctness*, not just *presence* — assert on actual values.

### Prototype first

For new features involving data model decisions or UI design, write a script in `scripts/` first. Validate on real data. The prototype catches wrong assumptions before you invest in production code.

## Dev environment

- **Rust** (stable) — backend server, event pipeline
- **Node.js 20+** — React dashboard
- **Docker** — E2E tests only

## Building and testing

```bash
# Rust — all crates + integration tests
cd rs && cargo test

# Rust — build the CLI binary
cd rs && cargo build -p open-story-cli

# React dashboard
cd ui && npm install && npm run dev    # dev server (port 5173)
cd ui && npm test                      # run tests

# E2E tests (requires Docker)
cd rs && docker build -t open-story:test .
cd e2e && npx playwright test
```

Or use `just`:
```bash
just test        # Run all tests (Rust + UI)
just test-rs     # Rust only
just test-ui     # UI only
just e2e         # E2E tests
```

## Branch strategy

- `master` is the stable trunk
- Use feature branches for contributions
- PRs against master — maintainers review and merge
- Run tests before pushing

## Commit messages

Write detailed commit messages. First line: concise summary (imperative mood, under 72 chars). Body: explain *why* and *how*. Structure as **Problem → Solution → Test coverage** for non-trivial changes.

```
Add rate limiting to WebSocket broadcast

Problem: Rapid event bursts could overwhelm slow clients, causing
backpressure and memory growth.

Solution: Added a bounded channel (1000 events) per WS connection.
When full, oldest events are dropped with a warning logged.

Tests: 3 new integration tests covering burst, slow client, and
reconnect scenarios.
```

Detailed commits are how the next agent picks up context without reading every diff.

## Code style

- **Functional-first**: pure functions for core logic, side effects at the edges
- **Actor systems**: independent actors communicating through events, no shared mutable state
- **Minimal code**: three clear lines beat a clever helper. Don't build for hypothetical futures.
- **Open standards**: CloudEvents 1.0, JSONL for persistence

See [CLAUDE.md](CLAUDE.md) for the full set of architectural principles and [docs/soul/](docs/soul/) for the philosophy behind them.

## Test locations

| Layer | Location | Runner |
|-------|----------|--------|
| Rust unit tests | inline `#[cfg(test)]` modules | `cd rs && cargo test` |
| Rust integration | `rs/tests/` | `cd rs && cargo test` |
| UI unit tests | `ui/tests/` | `npm test` |
| E2E tests | `e2e/tests/` | `npx playwright test` |

## Generating test fixtures

Test fixtures are synthetic data generated by `scripts/synth_transcripts.py`:

```bash
# Run the generator's own tests
python scripts/synth_transcripts.py --test

# Regenerate all Rust test fixtures
python scripts/synth_transcripts.py generate --fixtures

# Generate E2E seed data
cd rs && cargo test --test gen_seed_data -- --ignored
```

## PR checklist

- [ ] Tests pass (`just test`)
- [ ] New code has failing specs first (BDD)
- [ ] No side effects in core crates (core, views, patterns)
- [ ] Documentation updated in the same commit as code changes
- [ ] Use cases still point to valid files/lines (if you touched referenced code)
- [ ] Story/plan status updated if applicable
