# Research: The Eval-Apply Structure of Agent Sessions

*From a conversation that started with "tell me about it" and followed the thread.*

## What This Is

A research prototype demonstrating that AI coding agent sessions have layered computational structure — and that OpenStory can surface it. Built across three days in April 2026, TDD'd from scratch, validated against real OpenStory session data.

The core insight: the agent loop is SICP's metacircular evaluator. `eval` (model call) and `apply` (tool dispatch) call each other in a loop. The conversation is the environment. Context compaction is garbage collection. The Agent tool is a compound procedure. This isn't a metaphor — the types map one-to-one.

## How To Use This

### Run the prototype

```bash
cd docs/research/eval-apply-prototype
npm install
npx tsx test.ts              # 18 detector tests
npx tsx sentence-test.ts     # 20 sentence tests
npx tsx domain-test.ts       # 20 domain event tests

# Against a real session (requires OpenStory running on localhost:3002):
npx tsx eval-apply-detector.ts                          # list sessions
npx tsx eval-apply-detector.ts <session-id>             # terminal output
npx tsx eval-apply-detector.ts <session-id> --html      # HTML visualization
```

### Read the designs

| Document | What it describes |
|----------|-------------------|
| [`DESIGN_EVAL_APPLY_VIEW.md`](DESIGN_EVAL_APPLY_VIEW.md) | The eval-apply detector concept and UI mode |
| [`IMPLEMENTATION.md`](IMPLEMENTATION.md) | Step-by-step: Rust detector + React component |
| [`SPEC_DOMAIN_EVENTS.md`](SPEC_DOMAIN_EVENTS.md) | `ToolOutcome` enum for OpenStory's translate layer |
| [`../design-domain-events.md`](../design-domain-events.md) | Combined design: domain events (Rust) + Story tab (React) |
| [`LINEAGE.md`](LINEAGE.md) | History: Aristotle → Church → McCarthy → Sussman → the agent loop |
| [`OPEN_LETTER.md`](OPEN_LETTER.md) | The open letter to Sussman, Abelson, Julie, and Anthropic |

### Implementation path

**Start here:** `../design-domain-events.md` — this is the actionable design with Rust code for Part 1 (domain events) and React architecture for Part 2 (Story tab).

**Part 1 (Rust, backend):** Add `ToolOutcome` enum to `event_data.rs`, `derive_tool_outcome()` to `translate.rs`. ~7 files, follows existing `apply_*` pattern from the monadic EventData refactor.

**Part 2 (TypeScript/React, frontend):** Port prototype from `eval-apply-prototype/` into `ui/src/lib/story/` and `ui/src/components/story/`. The prototype code is the executable spec — 58 tests that define the behavior.

## The Five Layers

The prototype renders five layers of the same data per turn:

```
Layer 5: Sentence     "Claude wrote 8 Scheme files after reading 6 sources"
Layer 4: Domain       +3 created, ~1 modified, 2 cmd ok (deterministic facts)
Layer 3: Structure    eval [tool_use] → apply × 12 → CONTINUE (eval-apply phases)
Layer 2: Events       tool_call, tool_result, user_message (OpenStory today)
Layer 1: Raw          JSONL transcript bytes
```

Each layer is a different language for a different audience. Each is independently toggleable in the visualization.

## Origin

This research emerged from examining the architecture of a Rust reimplementation of Claude Code. The full conversation — 2235 OpenStory records across 62 turns — is recorded in the source repository. The Scheme implementation (45 tests) that proves the SICP correspondence lives there as well.

The prototype was validated against the session that produced it — the eval-apply detector observing the conversation where the eval-apply detector was built.
