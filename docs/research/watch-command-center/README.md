# Watch Command Center — wireframe exploration

Exploratory wireframes for a 2×2 live-watch grid: a senior engineer watching four AI
coding agents at once (NOC / mission-control). Read-only mirror — observe, never interfere.
Wireframes only; nothing here touches the production UI.

## Files
- **`WIREFRAMES.md`** — ASCII wireframes + rationale. Card-anatomy diagram and all four
  states: all-active, mixed (active + idle + errored — the money shot), empty slots, focus mode.
- **`command-center.html`** — self-contained static mock (inline CSS, no build/deps/network).
  Open directly in a browser. Shows the mixed state in OpenStory's Tokyonight dev-tool palette.

## Top 3 UX decisions
1. **Equal-weight when healthy, loud when not** — status is carried by color + glyph + a left
   edge rail; a calm fleet looks calm, an errored agent jumps out red, idle recedes gray.
2. **Stable quadrant placement over auto-sort by severity** — cards never reflow; position is
   muscle memory, severity is color. You learn the board once.
3. **Keyboard-first, tail -f ticker** — newest event at the bottom with a live caret, `1-4`
   focus a quadrant, `esc` back to grid; motion only ever means a real event arrived.

## Open question
Should empty quadrants stay as dashed drop-targets (capacity always visible, placement
stable) or should the grid auto-collapse to fit only watched sessions (more pixels per agent,
but the layout shifts under you)? Picked the former here — worth a human's taste.
