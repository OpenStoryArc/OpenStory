# Agent-driven UI tours (and how to make more of them)

*Written 2026-07-13, the night Max and Claude tuned the design live and the
dashboard gave its own narrated tour.*

## What happened

An agent drove the OpenStory dashboard through its **control seam** — the
same write path any MCP/operator agent uses — while narrating aloud through
macOS text-to-speech. Nothing touched the DOM; the UI is a pure sink that
reacts to `control` messages broadcast over its WebSocket, and every drive is
visibly attributed ("driven by ui-tour") so the human can always seize the
wheel by simply navigating.

## The moving parts

1. **The control seam** — `POST /api/control` with
   `{"action": ..., "params": {...}, "issuer": "your-name"}`.
   The server broadcasts it to every connected dashboard over `/ws`;
   `ui/src/lib/ui-control.ts` parses it; components subscribe via
   `controlActions$`.

   | action | params | what it does |
   |---|---|---|
   | `open_view` | `view` (live/explore/story/canvas/ask/users/admin), `sessionId?`, `eventId?` | navigate |
   | `present` | `message`, `route?`, `sessionIds?` | banner to the human |
   | `toggle` | `target`, `value` | flip a wired view control |
   | `query` | overview filter fields | narrow the Overview |

   **Wired toggle targets** (grep `controlActions$` for the current list):
   `theme` (light\|dark) · `session.lens` (conversation\|trace\|subagents\|details) ·
   `ribbon.compact` / `ribbon.collapsed` / `tokens.collapsed` (on\|off) ·
   `story.sort` (latest\|active\|tokens) · `ask.question` · `canvas.mode`.

   Not yet drivable (candidates to wire next): the ⌘K command palette,
   the text-size control (S/M/L/XL).

2. **Narration** — `/usr/bin/say -v "Serena (Premium)" -r 178 "<text>"`.
   Premium/Enhanced voices are far better than the defaults; download them in
   System Settings → Accessibility → Spoken Content → Manage Voices.
   Write narration for the EAR: contractions, no symbols, 2–4 sentences.
   `say` BLOCKS until finished — that's what paces a tour.

3. **The tour script** — `scripts/ui-tour.sh`. Twelve stops with a narrative
   arc (flight-recorder opening → session deep-dive with real lens-flipping →
   theme flip → every tab → a meta closing). Flags: `--fast` (no sleeps),
   `--silent` (no voice). Env: `OS_TOUR_VOICE`, `OS_TOUR_SID`, `OS_TOUR_API`.

   ```sh
   OS_TOUR_VOICE="Serena (Premium)" ./scripts/ui-tour.sh
   ```

4. **Video recording** — `scripts/record-tour-video.sh [out.mov]`.
   Records the main display (`screencapture -v`) while the tour runs; each
   narration line is rendered to its own audio file with a start offset, and
   ffmpeg muxes a **clean narration track** onto the recording afterwards —
   no microphone involved. Needs Screen Recording permission for the
   terminal + `brew install ffmpeg`. Put the browser front & center first:
   everything on the main display is recorded.

## Recipes

**One-off drive from any shell** (no script):
```sh
curl -s -X POST http://127.0.0.1:3002/api/control -H 'Content-Type: application/json' \
  -d '{"action":"present","params":{"message":"👋 from the seam"},"issuer":"me"}'
```

**Add a stop to the tour**: in `ui-tour.sh`, a stop is just
`post <action-json>` → `post <present-json>` → `pause 1` → `talk "<narration>"`.
Keep banners ≤160 chars; let `talk` carry the detail.

**Wire a new toggle target**: subscribe in the component —
```tsx
useEffect(() => {
  const sub = controlActions$().subscribe((a) => {
    if (a.type === "toggle" && a.target === "my.thing") apply(a.value);
  });
  return () => sub.unsubscribe();
}, []);
```
— then document it in the table above.

## Why this matters

The tour is the product thesis demonstrating itself: OpenStory renders the
observed/observer seam between humans and agents, and a tour is an agent
using that seam — visibly, interruptibly, with the human able to take the
wheel at any moment. Interaction and command are inverses; a captured
journey can replay as a tour, and a tour is a journey somebody else can
follow.
