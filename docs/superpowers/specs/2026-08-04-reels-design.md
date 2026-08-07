# Reels — saved, replayable story sequences (design)

**Date:** 2026-08-04 · **Status:** approved design, pre-plan
**Depends on:** the Attention-tree / `navigate_to` work on `fix/grok-ui-bugs`
(commit `c7d7af2`, PR pending) — the spotlight primitive and Intent/Attention
algebra this builds on.

## Problem

The Event Spotlight (full-screen, one-event presentation mode) is the best
storytelling surface in the product, but today it is:

- **Ephemeral** — pure presentation state in the Attention tree, not a place.
  A user cannot go there; only an agent can raise it.
- **Undiscoverable over MCP** — the server instructions never say "spotlight";
  the name exists only in tool-schema fine print and demo scripts.
- **Not a pattern** — narrated stories (demo-katie-90.sh, demo-launch-90.sh)
  are hand-rolled bash: macOS `say`, repo paths, curl against the seam. A
  foreign agent with only the MCP cannot tell a story.

## Decision summary (approved 2026-08-04)

| Decision | Choice |
|---|---|
| Tab contents | Saved stories (a theater), not a bare spotlight browser |
| Playback voice | Browser TTS (Web Speech API) + on-screen captions; reel format stays pure text |
| Naming | Artifact = **reel**, tab = **Reels**; the one-event primitive keeps the name **spotlight** |
| Architecture | Approach A: reels as store artifacts, UI-native player, 3 new MCP verbs |
| Reel editing UI | Out of scope v1 — agents re-save, users replay |
| Cross-fleet publish | v2 — backlog entry, design preserved there |

## Soul fit

A reel is *curation about* history — ordered pointers into the immutable
record plus narration text. It never mutates events (mirror, not leash; same
precedent as PlanStore). Reels are plain JSON files in `data_dir/reels/` —
greppable, portable, useful without the tool (sovereignty). Playback drives
only `ui.*` attention, never observed history.

## The reel format

One JSON file per reel, `data_dir/reels/<id>.json`:

```json
{
  "id": "reel-<uuid>",
  "title": "The Launch pitch, told by the record",
  "created": "2026-08-04T18:00:00Z",
  "author": "<principal_id or issuer string>",
  "closer": "Three weeks. Four sessions. One pitch.",
  "stops": [
    {
      "sessionId": "1300bfa5-…",
      "eventId": "0fceac2a-…",
      "line": "June twenty seventh. It starts with one typed line…",
      "clipAt": null
    }
  ]
}
```

- `line` is the narration/caption text — the ONLY narration representation.
  Voice is a play-time rendering (TTS), never stored audio.
- `clipAt` (optional) crops the shot before a marker — camera framing, record
  untouched (same semantic as the existing `focus_event` param).
- `closer` (optional) renders as the full-screen title card (TitleSpotlight)
  after the last stop.

**Validation at save:** every `(sessionId, eventId)` must resolve in the
EventStore. `save_reel` rejects unknown events — "do not invent events"
enforced at the seam, not by convention.

## Components

### 1. ReelStore (rs/store)

File-backed: JSON files in `data_dir/reels/`, no EventStore-trait extension
(no Mongo mirror, no conformance burden — the files ARE the artifact; the
store trait is only consulted to validate event references at save time).
CRUD: list / get / save / delete. Mirrors `plan_store.rs` in spirit.

### 2. REST API (rs/server)

- `GET /api/reels` — list (id, title, created, author, stop count)
- `GET /api/reels/{id}` — full reel
- `POST /api/reels` — save (validates every stop's event exists; 422 with the
  offending ids otherwise)
- `DELETE /api/reels/{id}`

### 3. Reels tab + player (ui/)

- New top-level tab **Reels** (own tab per UI convention), route
  `view: "reels"` in HashRoute — the tab and a selected reel are bookmarkable;
  the *playing* state remains ephemeral Attention (consistent with spotlight).
- Landing state: reel list (title, stops, created, author). Click → player.
- Player: walks stops in order; per stop raises the existing EventSpotlight
  with the stop's event, renders `line` as a caption card, speaks it via
  `speechSynthesis` (default voice, rate ~1.0); advances on utterance end
  (+ ~2s beat) — captions-paced fallback when TTS is unavailable/muted.
  Space/click = next stop, Esc = exit to list. Closer renders TitleSpotlight.
- Pure logic (stop sequencing, advance/exit transitions) lives in
  `ui/src/lib/reel-player.ts` as a reducer — BDD-testable without DOM/TTS.

### 4. MCP verbs (rs/mcp)

- `save_reel { title, stops, closer?, author? }` → `{ id, ok, invalid_stops? }`
- `list_reels {}` → trim rows
- `play_reel { id }` → drives the human's dashboard to the Reels tab and
  starts playback via the control seam (a `navigate_to {kind:"reel", id,
  autoplay:true}` under the hood); returns `{ ok, delivered }`.
- `navigate_to` gains `kind: "reel"`.

The authoring contract for a foreign agent is now MCP-complete:
find events (`search`/`agent_search`/`session_story`) → `save_reel` →
`play_reel`. Zero repo knowledge, zero bash, zero `say`.

### 5. Documentation clarity fixes (the "not clear" audit, 2026-08-04)

- Server instructions (rs/mcp): SHOW-HUMAN block names the **Event
  Spotlight** (`spotlight:true` = full-screen event; `present {spotlight}` =
  title card) and **Reels** (save/list/play).
- `rs/mcp/agent-docs/hands.md`: spotlight + reels become a proper beat in the
  show-human flow (today: one parenthetical at line 74).
- `ui_control` schema: add `clipAt` to the `focus_event` params doc (drift
  found vs docs/agent-in-ui.md).
- `rs/mcp/agent-docs/examples/show-human.md`: add the reel example.

### 6. Skill (openstory-skills repo, separate change)

`/openstory:reel` — three phases, MCP-only:
1. **Research** — existing tools; honesty rules from the demo pattern
   (verify every event id, quote display values verbatim, exclude the current
   session's own ask, flag what the record does NOT contain).
2. **Author** — `save_reel`, handle `invalid_stops` by re-searching.
3. **Show** — `play_reel`, then hand the wheel back and ask for a reaction.

## Testing (BDD, red → green)

- **Rust:** ReelStore CRUD; POST validation rejects a fabricated eventId
  (422 lists it); API round-trip.
- **UI (vitest, `scenario(given/when/then)`):** reel-player reducer — stop
  advance, TTS-end vs manual advance, exit, closer transition; hash-route
  round-trip for `view:"reels"` (+ selected reel); attention fold for
  `kind:"reel"` intent.
- **E2E (later, needs seed reel fixture):** click reel → spotlight visible →
  advances → title card.

## Out of scope (v1)

- Reel editing/reordering UI — agents re-save; users replay.
- Stored audio / video export — `record-tour-video.sh` still works by
  pointing at a playing dashboard.
- Cross-fleet publishing — see BACKLOG "Publish reels across the fleet"
  (Katie publishes → Max views; reels are already portable files; design
  sketch preserved there: `reel.published` CloudEvent over the existing
  federated stream, receiver lands the file in its own `data_dir/reels/`).
- Migrating demo-katie-90 / demo-launch-90 to reels (nice follow-up: a
  one-shot script that converts a demo script's stops into a saved reel).
