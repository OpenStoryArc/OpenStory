---
name: openstory-tour
description: Use when asked to give a narrated/driven tour of the OpenStory dashboard, tell a "user story" from session data, demo features by driving the UI, tune the design live with voice narration, or record a tour video. Drives the UI through the control seam while narrating aloud.
---

# OpenStory narrated UI tours & story demos

You drive the OpenStory dashboard through its **control seam** while narrating
aloud with macOS `say`. The human watches their own dashboard being driven,
with every action visibly attributed ("driven by <issuer>") and seizable —
they take the wheel by simply navigating.

**Canonical reference:** `docs/agent-ui-tours.md` (control vocabulary, wired
toggle targets, recipes). Read it first if unsure. MCP path: `docs/agent-in-ui.md`.

## Prerequisites (check before starting)
1. Hub serving: `curl -s http://127.0.0.1:3002/api/sessions >/dev/null` —
   if down, start with `scripts/start-local-hub.sh` (in ~/projects/OpenStory).
2. Voice: prefer a Premium voice — `say -v '?' | grep -iE "premium|enhanced"`.
   Max's pick: **"Serena (Premium)"**. Fall back to Samantha. Narration is
   written for the EAR: contractions, no symbols, 2–4 sentences per beat.
3. The dashboard tab must be open (the seam broadcasts over its WebSocket).

## The three moves

**1. Run an existing tour/demo** (all in `scripts/`):
```sh
OS_TOUR_VOICE="Serena (Premium)" ./scripts/ui-tour.sh          # 12-stop product tour
OS_TOUR_VOICE="Serena (Premium)" ./scripts/demo-katie-loop.sh  # a user story from real data
./scripts/ui-tour.sh --fast --silent                            # test without voice/pauses
```
Run them in the background; they pace themselves on blocking `say`.

**2. Create a new story demo** — the two-phase pattern:
- Dispatch a researcher subagent: mine `GET /api/sessions` +
  `/api/sessions/{id}/conversation` for the REAL story (verbatim quotes,
  session ids, event ids for deep links). HONESTY RULES: never fabricate;
  flag unverifiable claims; exclude the current session's own ask as
  evidence (circularity trap — it happened once).
- Have it write `scripts/demo-<name>.sh` mirroring `demo-katie-loop.sh`
  exactly (post/talk/pause helpers, VOICE/SID env vars, --fast/--silent,
  OS_TOUR_AUDIO_DIR video mode, eventId deep links), `bash -n` it, commit
  it (specific files only). Then YOU run it for the human.

**3. Live design tuning** — the interactive loop Max loves:
narrate what you're about to change (`say` in background) → edit tokens in
`ui/src/index.css` (HMR applies instantly on the Vite preview) → flip/point
the UI via the seam → `present` a banner asking for a verdict → the human
reacts by voice → iterate. Commit each accepted round as its own commit.

## Driving the seam (raw HTTP; MCP tools wrap the same thing)
```sh
B=http://127.0.0.1:3002/api/control
curl -s -X POST "$B" -H 'Content-Type: application/json' -d '{
  "action":"present","params":{"message":"👋"},"issuer":"my-tour"}'
```
Actions: `open_view {view, sessionId?, eventId?}` · `present {message, route?}`
· `toggle {target, value}` · `query {…}`. Wired toggles: `theme`,
`session.lens`, `ribbon.compact|collapsed`, `tokens.collapsed`, `story.sort`,
`ask.question`, `canvas.mode`. Via MCP: `ui_control`, plus `where_is_user` /
`subscribe_ui_state` to meet the human where they are.

## Recording a video
```sh
./scripts/record-tour-video.sh [out.mov]   # or point it at any demo script
```
Records the main display + muxes a clean rendered narration track (no mic).
Needs Screen Recording permission for the terminal (grant → quit & reopen
terminal) and ffmpeg. WARN the human first: the whole display is recorded —
have them stage the browser and say go.

## Voice + narration etiquette
- Speak SHORT updates while working (background `say ... &`); block on `say`
  only when pacing a tour.
- Banners ≤160 chars; the voice carries the detail.
- Always end asking for a reaction — this is a dialogue, not a broadcast.
- If the human speaks mid-tour, stop talking and respond.
