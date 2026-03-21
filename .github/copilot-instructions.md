# Open Story — Copilot Instructions

## Soul

Open Story gives humans visibility into AI coding agent behavior, in real time. It observes but never interferes — a mirror, not a leash. Your data stays local, in open formats, fully portable.

## Non-negotiables

1. **Observe, never interfere** — No features that write back to the agent, modify transcripts, or block execution. The data flow is unidirectional.
2. **BDD** — No production code without a failing spec first. Tests verify correctness, not just presence.
3. **Functional purity in core** — Core crates (core, views, patterns) are pure functions with no I/O dependencies. Side effects live in server/.
4. **Open standards** — CloudEvents 1.0, JSONL, Markdown. No proprietary formats.
5. **Atomic documentation** — Code and docs change together in the same commit.

## Architecture

Unidirectional pipeline: Source → Translate → Ingest → Persist → Broadcast → Render. 9 Rust crates with enforced dependency boundaries (the compiler prevents side effects in core). RxJS observables on the frontend. Actor model with message-passing, no shared mutable state.

## Before you build

- Read `docs/soul/use-cases.md` — concrete code examples of each principle
- Read `docs/soul/patterns.md` — mistakes already made, don't repeat them
- For new features: prototype in `scripts/` first, validate on real data
- For large changes: walk through `docs/architecture-tour.md` (14-stop guided tour)

## Full instructions

See `CLAUDE.md` for complete principles, build commands, and conventions.
See `docs/soul/` for philosophy, architecture, patterns, and use cases.
