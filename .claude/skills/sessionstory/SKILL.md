---
name: sessionstory
description: Tell the story of an OpenStory session by collecting deterministic facts (records, tools, patterns, prompts) via the REST API, then narrating them. Use when the user asks "what happened in session X", "summarize this session", "what was that session about", or wants to understand a past or current session through OpenStory's own data. Dogfoods OpenStory — never grep transcript files when you can ask the API.
---

# sessionstory

A two-phase skill: a script collects the facts, you write the story.

## Phase 1 — Collect facts (deterministic)

Run the script. It hits three OpenStory API endpoints, aggregates them, and prints a structured fact sheet. It does **not** narrate.

```bash
python3 scripts/sessionstory.py SESSION_ID                # markdown fact sheet
python3 scripts/sessionstory.py latest                    # most recent session
python3 scripts/sessionstory.py SESSION_ID --json         # machine-readable
python3 scripts/sessionstory.py SESSION_ID --brief        # shape + prompts only
python3 scripts/sessionstory.py SESSION_ID --unfinished   # include trailing assistant messages
python3 scripts/sessionstory.py --list                    # list recent sessions
```

The script requires the OpenStory server running on `http://localhost:3002` (override with `--url`).

**What's in the fact sheet:**
- **Shape** — total records, turns, duration, sidechain count
- **Record types** — full histogram (`tool_call`, `assistant_message`, `user_message`, `turn_end`, …)
- **Tool calls** — histogram by tool name
- **Patterns** — counts of every `pattern_type` from the patterns endpoint (`eval_apply.*`, `turn.phase`, `turn.sentence`, `error.recovery`, `test.cycle`)
- **Turn phases** — distribution of `conversation`, `implementation`, `execution`, `delegation`, `testing`, etc., from the `turn.phase` detector
- **Sample sentences** — up to 8 verbatim outputs from the `turn.sentence` detector (these are *gold* — quote them, don't paraphrase)
- **Prompt timeline** — every non-noise top-level user prompt in time order, with `HH:MM` timestamps. Filters `<task-notification>`, `<command-name>`, `[Image: …]`, and `[Request interrupted]` automatically.
- **Trailing assistant messages** (with `--unfinished`) — last 6 assistant messages, useful for figuring out what was in flight at session end (e.g., recovering from a `/compact` failure)

## Phase 2 — Narrate (model)

Read the fact sheet and write the story. Lead with shape, then tools, then patterns, then arc inferred from the prompt timeline. Be specific and ground every claim in the data:

- **Quote sentences verbatim from the detector** — they are the ground-truth narrative atoms. Do not paraphrase.
- **Group prompts into time blocks** — use natural gaps in the timeline (≥ 30 min) and topic shifts visible in the prompts
- **Cross-reference patterns to prompts** — e.g., if `delegation: 2` shows up in `turn.phase`, find the two prompts that triggered subagent work and call them out
- **Look for inconsistencies in the patterns** — e.g., `eval_apply.scope_open` >> `scope_close` often signals subagent flushes worth flagging
- **Use `--unfinished` if the user asks "what was in flight"** or wants to pick up where a previous session left off — the trailing assistant messages reveal what the model was doing right before the session ended

## Output style

Structured markdown with sections: Shape → Tool usage → Patterns → Narrative arc. Use tables for histograms. Quote real strings from the data. Avoid filler — the user reads the fact sheet too.

End with a question or a "want me to dig into X" offer if there are obvious follow-ups (e.g., "the two delegation turns are X and Y — want me to pull their full sentences?").

## When NOT to use this skill

- The user asks about *current* code state — use `git log`, `Read`, `Grep` instead
- The user wants live/streaming events — use the WebSocket or `curl /api/sessions/{id}/records` directly
- The user wants day-scoped narration across multiple sessions — use `docs/research/scheme/daystory.sh`

## Adjacent scripts

The `scripts/analyze_*.py` family produces complementary structured output and is fair game when the fact sheet isn't enough:

- `analyze_eval_apply_shape.py --session SID` — eval-apply cycle structure (cycles, terminal vs with-tools, tools per cycle)
- `analyze_turn_shapes.py SID` — distribution of distinct turn shapes and their probability classes
- `analyze_event_groups.py --session SID` — per-turn event counts and tool sequences
- `token_usage.py --session-id SID` — input/output/cache tokens and estimated cost

If you find yourself needing data the fact sheet doesn't expose, prefer extending `sessionstory.py` over writing inline Python — scripts endure, one-liners vanish.
