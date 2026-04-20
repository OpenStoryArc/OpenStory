---
name: session-report
description: Query OpenStory for a structural report on an agent session — at-a-glance metrics, turn arc, file impact, errors, and optional event detail. Use when you have a session ID and need to know what happened in it without re-reading raw transcripts.
version: 1.0.0
metadata:
  openstory:
    tags: [observability, recall, session, introspection, hermes-integration]
    related_skills: []
    related_tools: [recall_tool_sketch.py]
    requires_server: "OpenStory at http://localhost:3002 (or via OPENSTORY_API_URL)"
---

# Session Report

Produces a markdown-formatted structural report on a single OpenStory session, optionally surfacing one specific event in detail. Five sections, in order: at-a-glance metrics, errors, turn arc, file impact, and (optional) event detail.

## When to use this

- The user gives you a session ID and asks "what happened?" / "what did I do in that session?" / "tell me about session X."
- You need to understand the shape of a session before deciding how to act on it — picking up where a previous session left off, debugging a failed run, summarizing for a report, deciding whether work is worth resuming.
- You need to compare sessions structurally (run this skill on multiple session IDs and compare the outputs).
- You're acting *as* a Hermes agent that has a `recall` tool and you want a worked example of what calling it should produce. This skill is the human-callable equivalent of the same five-endpoint composition.

Do **not** use this for:
- Reading the raw transcript of a session — use `/api/sessions/{id}/conversation` or `/api/sessions/{id}/records` directly. This skill *summarizes*, not *reproduces*.
- Real-time monitoring — this is a snapshot, not a stream. For live observation, subscribe to the WebSocket `live.events` / `live.story` channels.

## Inputs

| Input | Required | Notes |
|---|---|---|
| `session_id` | yes | The OpenStory session UUID |
| `event_id` | no | If provided, the report includes a detail section for that specific event |
| `base_url` | no | Defaults to `http://localhost:3002` or `$OPENSTORY_API_URL` |

## Procedure

The procedural layer is captured in [`session_report.py`](../../session_report.py) one directory up. To run it:

```bash
# Basic report
python3 session_report.py <session_id>

# Report with one event surfaced in detail
python3 session_report.py <session_id> --event <event_id>

# Against a non-default OpenStory
python3 session_report.py <session_id> --base-url http://remote:3002
```

The script is stdlib-only — no `pip install` needed. It composes five OpenStory endpoints in a specific order, in a specific way. The order matters; the *why* of each is below.

### The five endpoints, in the right order

1. **`GET /health`** — reachability ping. Fail fast if OpenStory isn't running. ~5ms cost. If this fails, abort with a clear message; do not proceed to the next steps.

2. **`GET /api/sessions/{id}/synopsis` AND `GET /api/sessions/{id}/summary`** — these overlap by ~70% but each has unique fields. Synopsis has `label`, `top_tools`, `duration_secs`, `event_count`, `error_count`. Summary has `status`, `model`, `prompt_count`, `response_count`, `exit_code`. Call both in parallel; merge for the at-a-glance section. (A future, more confident version of this skill may drop one of the two — but until you know which fields you need, calling both is the right explore-mode trade.)

3. **`GET /api/sessions/{id}/errors`** — cheap call that surfaces a single high-signal number. Zero errors across hundreds of tool calls is itself a fact worth reporting. If non-zero, the error messages tell you where to look next.

4. **`GET /api/sessions/{id}/patterns?type=turn.sentence&limit=50`** — the narrative arc. **Note: the response is wrapped as `{"patterns": [...]}`, not a bare list.** Slicing the response directly will raise `TypeError: unhashable type: 'slice'`. Bracket the result by showing the *first 5* and *last 15* turn sentences — first 5 to see the original intent, last 15 to see what's happening now. The middle is usually noise from a bird's-eye view. Each pattern carries a `metadata.turn` integer (the turn number) and a `summary` string (the sentence diagram).

5. **`GET /api/sessions/{id}/file-impact`** — top files by read+write count. Show the top 15. Display the last 3 path components per file to disambiguate common basenames (e.g., `server/src/state.rs` vs `store/src/state.rs`).

6. *(only if `event_id` was passed)* **`GET /api/sessions/{id}/events/{event_id}/content`** — the targeted lookup. **This may return empty.** If so, fall back to `GET /api/sessions/{id}/events?limit=10000` and grep client-side. The fallback is expensive (~4MB for a 4000-event session) but reliable.

## Output structure

Render as markdown, in this order:

```
# Session report — `<session_id>`

## At a glance
[Label, then a 2-column metadata table: Project, Status, Model, First/Last event,
 Duration, Events, Tool calls, Errors, Human prompts, Assistant responses, Top tools]

## Errors
[Either "Zero errors. Clean session." or a bulleted list of error messages]

## Turn arc — N turn-sentences surfaced
### First 5
[Bulleted list: - **T<n>** — <summary>]
### Last 15
[Same format]

## Files most worked
[Markdown table: File | Reads | Writes (right-aligned)]

## Event `<event_id>`              ← only if --event was passed
[Time, Source, Type, Subtype, then a JSON code block of the agent_payload truncated to 2000 chars]
```

The order is deliberate: **identity first, health second, narrative third, substance fourth, specifics last.** A reader who only wants the headline gets it from "At a glance" and "Errors." A reader who wants the story keeps reading. A reader who wants ground truth on a specific event finds it at the bottom.

## What to actually look for

The procedural and aesthetic layers above are reproducible. This section is the *contextual* layer — judgment calls that take a report from "data dump" to "useful." When reading the rendered report:

- **Status: ongoing vs. completed.** An ongoing session means the last event is recent and the agent may still be working. A completed session is a closed unit you can summarize whole.
- **Errors: zero vs. non-zero.** Zero errors across hundreds of tool calls is a *strong* health signal — flag it. Non-zero errors deserve a closer look at the error messages and the surrounding turn sentences.
- **Duration vs. event count.** A long duration with few events suggests waiting / paused work. A short duration with many events suggests dense, focused activity. The ratio matters more than either number alone.
- **The first turn vs. the last turn.** Often the first prompt is a small ask and the last turn is doing something much larger. The label (which is usually the first prompt) can be misleading about scope. The arc tells the truth.
- **Files with reads ≫ writes.** This is exploration / understanding work. Files with writes ≫ reads is creation work. Files with balanced read/write counts (like `ingest.rs` at 26/26) are *iteration* work — the kind of edit-test-edit-test cycle that means the agent is actively wrestling with that file.
- **A "honest answer:" or "you're right to push on it" turn.** These phrases (visible in the sentence summaries) usually mark moments where the agent had to *correct* something it had previously asserted. They're high-signal turns and worth flagging in the report's prose.
- **Sudden shift in file focus.** If the first 10 turns touch backend files and the last 10 turn touch UI files, that's a phase change worth naming. The skill doesn't auto-detect this, but the file-impact + turn-arc together let a reader spot it.

## Common pitfalls

1. **Don't slice the patterns response directly.** It's `{"patterns": [...]}`, not a bare list. Use `data["patterns"][...]`.
2. **Don't trust the targeted event endpoint to always return.** Always have the listing fallback ready. (See `find_event` in `session_report.py`.)
3. **Don't over-truncate file paths.** Two trailing components collide (`src/state.rs`); three usually disambiguate.
4. **Don't pull all events unless you have to.** The fallback exists for a reason — only invoke it when you need a specific event, not as a default.
5. **Don't conflate `synopsis.event_count` with `summary.event_count`.** They sometimes differ slightly because one counts narrative events and the other counts raw events. The script uses whichever is non-null first; document the discrepancy if you spot it.
6. **The session may move while you're querying it.** If the session is `ongoing`, two queries seconds apart can return different counts. This is not a bug; it's the price of observing live state.

## Origin

Extracted from a manual session report on 2026-04-08, where the same procedure was run by hand against OpenStory's REST API for session `f2679c73-79b1-4514-a9d3-c9a43e055822`. The five-section format and the endpoint sequence both came from that run; the script and this skill file capture them so the procedure outlives the conversation that produced it.

The corresponding agent-facing tool is [`recall_tool_sketch.py`](../../recall_tool_sketch.py) two directories up — it wraps the same endpoints with the same intent, just in a tool-call invocation mode. This skill is the *worked example* of what composing those calls into a single useful artifact looks like.

Companion design docs:
- [`../../README.md`](../../README.md) — overview of the hermes-integration prototype directory
- [`../../../HERMES_INTEGRATION.md`](../../../HERMES_INTEGRATION.md) — the architectural framing brief
- [`../../../LISTENER_AS_ALGEBRA.md`](../../../LISTENER_AS_ALGEBRA.md) — why "an agent reading the algebra of its own coalgebra" is the structural justification for tools like `recall` and skills like this one
