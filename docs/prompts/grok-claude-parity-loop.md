# Loop prompt: Grok ↔ Claude E2E parity (OpenStory)

Copy everything under **PROMPT** into a new agent session (Grok Build or Claude Code) working in the OpenStory repo. Re-run / continue until exit criteria pass.

---

## PROMPT

```
You are implementing OpenStory so Grok Build is on full parity with Claude Code
as a first-class observed agent — with automated confidence, not vibes.

## Soul constraints (non-negotiable)
- Observe, never interfere. Never mutate Grok/Claude transcripts or inject into
  the harness execution path.
- BDD / TDD: describe behavior first (`describe("when X") / it("should Y")` or
  equivalent). Red → green → refactor. No production code without a failing
  test first.
- Fast cycles on this Mac: prefer `cargo test -p <crate> --lib <filter>` and
  targeted integration tests over full workspace builds.
- Functional-first; side effects at edges. Translators leave `raw` untouched.
- Data is user-owned: CloudEvents + JSONL. Agent field distinguishes platforms.
- Dogfood OpenStory when possible: REST `http://localhost:3002/api/` and MCP
  before grepping transcript files. This session may be observed — use it.

## Objective (one line)
A fresh OpenStory boot with Claude + Grok watch dirs can ingest Grok (seed or
live), show non-empty Story/Explore text and thinking, serve MCP
session_story + session_transcript, and keep container + one Playwright gate
green — the same practical confidence bar Claude has.

## Definition of parity (trust surface, not file format)
Same pipeline as Claude:
  transcript → watch → translate → CloudEvents → NATS
  → persist / patterns / projections / broadcast
  → REST + WS + MCP + UI

Equal fidelity on: ingest acts, views joinability, eval-apply + turn.sentence
("Grok …"), API/MCP tools, UI label/filter/cards, dual-watch ops, CI gates.

NOT required: Claude hooks, same tool names, harness mutation, live Grok API
in every PR (use real session extracts + seed_tree).

## Confidence ladder (do in order; each level is a closed TDD loop)

### L1 — Unit truth (cheap)
- [x] Views: tool_use ↔ tool_result call_id join on Grok fixtures
- [x] Views: assistant text + thinking non-empty (typed GrokPayload)
- [ ] Token usage fields from turn_completed → views/analytics usable

### L2 — Pipeline story (cheap)
- [x] Fixture → eval-apply → turn.sentence starts with "Grok"
- [ ] Multi-turn real_turn fixtures → stable pattern/golden assertions

### L3 — Server REST (medium)
- [ ] Un-ignore test_grok_container: seed_tree → origin_agent=grok-build +
      non-empty assistant records via /api/sessions/.../records
- [ ] /api/search hits assistant prose for Grok sessions

### L4 — MCP dogfood (critical for "extension of Grok")
- [x] session_transcript returns entries for Grok (do NOT rely on raw.role;
      reconstruct from typed payload / views — ACP has no Hermes-style role)
- [ ] session_story + subscribe_session smoke on fixture or live session
- [ ] Optional: grok-maxxxing outbound journal ⊆ inbound tools for this session

### L5 — UI E2E (medium)
- [x] origin-agent label/color for "grok-build" → "Grok" (ui/src/lib/origin-agent.ts)
- [ ] e2e seed with Grok session (factory or real_turn extract)
- [ ] Playwright: sidebar shows Grok; Story has visible assistant text

### L6 — Production co-existence
- [ ] One process: Claude watch_dir + grok_watch_dir, one DB
- [ ] Smoke: both agents visible; agent field never cross-contaminated

### L7 — Optional live Grok runner
- Only if headless Grok Build exists; otherwise L3–L5 on real extracts = ship bar.

## Done when
L1–L5 are green in CI (or documented local just recipes that mirror CI), and a
short PARITY.md (or PR checklist) maps each box to a test command + last green
timestamp. L6 verified once on this machine. L7 optional.

## How to loop (every iteration)

1. **Pick the next unchecked box** at the lowest incomplete L-level.
2. **Write the failing test** (behavior name, not implementation).
3. **Run only that test** — confirm red.
4. **Minimal code** to green. No drive-by refactors.
5. **Re-run the test** + nearest related suite (`-p open-story-views`, `-p open-story-core`, etc.).
6. **Dogfood** if server is up: curl or MCP against this/live Grok session.
7. **Atomic commit** on feat/grok-build-support (or stacked PR): one logical change.
8. **Update the checklist** in this prompt's progress (comment in PR or
   docs/prompts/grok-claude-parity-loop.md).
9. **Stop** only when L1–L5 are checked, or you are blocked (state the blocker
   and the smallest unblock experiment).

## Branch / PR
- Branch: feat/grok-build-support (PR #103 may already exist — extend it or stack).
- master is protected; no force-push; commit messages for agents (Problem →
  Solution → Test coverage).
- Do not commit drafts/, generated factory out/, or secrets.

## Known facts (do not re-discover blindly)
- Grok sessions: ~/.grok/sessions/{urlencoded-cwd}/{session-id}/updates.jsonl
- Translator: rs/core/src/translate_grok.rs; agent "grok-build"
- Views gap was: typed GrokPayload.text dropped for assistant/thinking —
  fixed for text/thinking; transcript MCP still Hermes-role based → empty
- Live dogfood DB may be /tmp/openstory-grok-test with Grok-only watch
- Real seeds: rs/tests/fixtures/grok/; scripts/extract_grok_session_seed.py,
  scripts/gen_grok_fixtures.py, scripts/grok_session_factory/
- Companion: /Users/maxglassie/projects/grok-maxxxing (outbound↔inbound reconcile)

## Anti-patterns
- Building UI before L4 transcript/API works
- Full `cargo test` every cycle (too slow) — target filters first
- Normalizing raw in the translator
- Claiming parity without a named test command that fails if broken
- Merging with #[ignore] container tests and no Playwright seed

## First action when you start
1. git status + read PR #103 / this checklist
2. Confirm OpenStory API or note if boot needed
3. Start L4 session_transcript TDD (highest leverage remaining) OR the lowest
   unchecked L1 item if L4 is blocked
4. Report: box chosen, red test path, then implement

Begin. Stay in the loop until L1–L5 are green or you hit a hard blocker.
```

---

## Short variant (sticky header for multi-hour sessions)

```
LOOP: Grok↔Claude OpenStory parity. TDD only. Observe never interfere.
Next unchecked box on ladder L1→L5 in docs/prompts/grok-claude-parity-loop.md.
Red test → green → atomic commit on feat/grok-build-support. Dogfood :3002/MCP.
Stop when L1–L5 green or blocked. No vibes claims — only named test commands.
```
