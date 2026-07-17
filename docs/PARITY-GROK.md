# Grok ↔ Claude parity — confidence gates

Last updated: 2026-07-17 (feat/grok-build-support)

## Objective

OpenStory treats Grok Build as a first-class observed agent at the same trust
surface as Claude Code: ingest → views → patterns → API/MCP → UI, with tests
that fail if that surface regresses.

## Gate commands (run from repo root)

```bash
# L1 — views (text, thinking, call_id join, turn token usage)
cd rs && cargo test -p open-story-views --lib grok_

# L1/L4 — MCP session_transcript subtype path for Grok
cd rs && cargo test -p open-story-mcp --lib transcript_entry

# L2 — single + multi-turn real fixtures → Grok sentences + pattern goldens
cd rs && cargo test -p open-story --test test_grok_storytelling

# L3 — Docker container: seed_tree → origin_agent=grok-build + non-empty assistant
# Requires: docker build -t open-story:test ./rs
cd rs && cargo test -p open-story --test test_grok_container

# L5 — UI unit (agent badge label/color)
cd ui && npm test -- --run tests/lib/origin-agent.test.ts

# L5 — Playwright Explore: Grok session + assistant prose
# Requires: open-story:test image + seed e2e/fixtures/seed-data/grok-session.jsonl
cd e2e && npx playwright test grok-parity.spec.ts
```

## Ladder status

| Level | Item | Gate |
|-------|------|------|
| L1 | tool join, text, thinking, tokens | `cargo test -p open-story-views --lib grok_` |
| L2 | multi-turn real fixture patterns | `test_grok_storytelling` |
| L3 | container seed_tree + FTS soft | `test_grok_container` |
| L4 | MCP transcript | `transcript_entry` |
| L5 | UI label + Playwright | `origin-agent` + `grok-parity.spec.ts` |
| L6 | Claude+Grok co-exist, no agent cross-talk | `cargo test -p open-story --test test_agent_coexistence` |
| L7 | live Grok Build runner in CI | optional |

## Data reflection (live dogfood shape)

Typical tool-heavy Grok session after views fix:

- `tool_call` / `tool_result` join on `call_id`
- `reasoning` + `assistant_message` non-empty
- `token_usage` on turns (camelCase ACP → views)
- `turn.sentence` summaries start with **"Grok"**
- Explore sidebar shows **Grok** agent badge; seed prose readable
