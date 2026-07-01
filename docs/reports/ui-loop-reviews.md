# UI Loop — Running UX & Design Reviews

A per-iteration review log for the `feat/ui-session-visibility` UI-improvement
loop. Each entry: a **UX critique** (framed for a senior developer who wants to
understand and *tell the story* of their agent work) and a **design review**
with inspiration from classic, simple brand leaders (**GitHub, Airbnb, Claude,
Apple**). Development is test-first — deterministic specs are used both to drive
the build and to *discover* the shape of each problem.

The reviews compound. Findings that aren't fixed in their iteration become the
backlog for later ones.

---

## Review #1 — Baseline after ribbon + Overview + untruncation + Story find

**Shipped so far:** D3 Session Activity Ribbon (Explore), Sessions Overview
dashboard (`#/overview`: calendar + facets + drill-in), harness-message
untruncation, Story sidebar find (search + facets).

### UX critique (senior-dev, "tell the story" lens)

Ranked by impact:

1. **No durations anywhere.** The single most story-shaping signal — "this Bash
   took 45s, that Read took 0.2s" — is invisible. We have `tool_call` and
   `tool_result` timestamps; we compute nothing. A senior dev narrating their
   work needs *where the time went*. → drives the **trace view** (task #7).
2. **Navigation is mouse-bound.** No `⌘K`, no fuzzy session jump, no keyboard
   switch between tabs. Finding one session among 1400 means scrolling. Every
   serious dev tool is keyboard-first. → drives the **command palette** (task #6).
3. **Filter state isn't in the URL.** Overview filters live in React state, so a
   view can't be shared or bookmarked — you can't send a teammate "here's the
   filtered story." Live already encodes `?user=`; Overview/Story should too.
4. **Loading states tell a thin story.** "Loading sessions…" text where a
   skeleton belongs; the prior UX review flagged the app "tells falsehoods" on
   boot (false Disconnected, "No events yet" over full history).
5. **No error-first affordance.** A session that errored looks like any other in
   the sidebar. A red dot / count would let a dev jump straight to the failure.
6. **The ribbon is a minimap but not yet a navigator.** It shows shape; it
   should also *scrub* — click-drag a time window to filter the event list
   (Chrome DevTools timeline brush).

### Design review (GitHub · Airbnb · Claude · Apple)

- **GitHub** — the calendar borrow is good; extend the "everything is
  permalinkable" ethos (copy-link on any event/session). Adopt GitHub's quiet
  density: our chips are close, but spacing is slightly inconsistent between the
  Overview sidebar and the Story sidebar — unify the facet-chip component.
- **Airbnb** — warmth through generous whitespace and a *single* accent per
  surface. We currently sprinkle blue/purple/green/orange/red fairly evenly;
  Airbnb would pick one primary (our blue `#7aa2f7`) and demote the rest to
  data-encoding only. Rounded, soft cards; friendly empty states with a next
  action, not a dead end.
- **Claude** — calm, legible typography and restraint. Reduce the number of
  simultaneous font sizes (we use 9/10/11/12/13/14/18px — tighten to a scale).
  Let content breathe; fewer borders, more spacing to separate regions.
- **Apple** — one obvious primary action per view; motion that explains (the
  drill-in should slide, the ribbon marks should fade in). Pixel-honest
  alignment: label gutters and baselines should line up across the calendar,
  ribbon, and lists.

**Cross-cutting design debt:** color tokens exist in `index.css` but components
use inline hex — a token pass would make the "one accent" discipline
enforceable. Filed, not yet fixed.

### Next iteration

Take **#2 (command palette)** — highest navigation leverage, keyboard-first, the
signature dev-tool pattern the brand leaders all share. Build the fuzzy matcher
test-first (it's a pure function — ideal for tests-as-discovery), then the UI.
