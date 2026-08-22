# Prompt: Reels interaction matrix loop

**Use when:** exploring slide × annotate × agent-pen parity, or dogfooding reels UX.  
**Stack:** OpenStory API `:3002`, UI `:5173`, branch with beat ink + `draw target=slide`.  
**Law:** ui.* only — never invent history events for spotlight stops unless they exist.

---

## Goal

Drive a full **create → play → annotate (agent) → clear/re-annotate → review** loop so every combination in the matrix is exercised and **eyes** (ui-state journey) can see the result.

---

## Preconditions

```bash
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3002/api/sessions   # 200
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:5173/                 # 200
```

If API is down, start hub from this tree (`open-story serve --port 3002 --data-dir ./data`).  
If UI is down, `cd ui && npm run dev -- --port 5173`.

---

## Loop (automated preferred)

```bash
# Offline contract (TDD)
cd ui && npx vitest run tests/lib/reel-annotate.test.ts tests/lib/ui-control.test.ts tests/lib/reel-slide.test.ts

# Live matrix loop (creates reel, agent inks slides, prints journey)
python3 scripts/reels_interaction_matrix.py
```

Then open the printed URL (or control `open_view` reels + reelId).

---

## Loop (manual agent steps)

1. **Create reel** — `POST /api/reels` with opener + title + diagram + title + closer (no fake event ids).
2. **Open** — `open_view` `{ view: "reels", reelId, reelAutoplay: true }`.
3. **Agent pen on each body slide** (unified index: opener=0 → body starts at 1 if opener exists):

```json
{
  "action": "draw",
  "params": {
    "target": "slide",
    "reelId": "<id>",
    "beatIndex": 1,
    "mode": "append",
    "strokes": [
      { "type": "circle", "cx": 0.3, "cy": 0.35, "r": 0.07, "stroke": "#0f172a", "strokeWidth": 8, "fill": "none" },
      { "type": "circle", "cx": 0.3, "cy": 0.35, "r": 0.07, "stroke": "#facc15", "strokeWidth": 4, "fill": "none" }
    ]
  }
}
```

Repeat for beatIndex 2, 3 with different (cx,cy).

4. **Clear one slide** — `target:slide`, `clear:true`, `strokes:[]` for one beatIndex; re-append a mark.
5. **Review** — `GET /api/ui-state/journey?n=40` and assert:
   - `reelId` present  
   - `beatInk.beatIndex` differs per slide  
   - `beatInk.stroke_count` > 0 after agent draw  
   - clear reduced one slide only (when UI reports)

6. **Usability spot-check (human):** Pause, click segments, Annotate slide, Done, ink stays on that slide only.

---

## Done when

- [ ] Vitest matrix tests pass  
- [ ] Live script exits 0 and prints reel URL  
- [ ] Journey shows multi-beat `beatInk` activity  
- [ ] Human can open URL and see marks / tools  

**Docs:** `docs/research/reels-interaction-matrix.md`, `docs/research/reel-slide-standard.md`
