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

---

## Review #2 — After the ⌘K command palette

**Shipped this iteration:** global command palette (⌘K) — fuzzy jump to any
session or tab, keyboard-first, with a visible "Jump to… ⌘K" header hint.
Built matcher-first (10 discovery specs shaped the scoring before any UI).

### UX critique

What the palette *closes:* finding 1 session among 1400, and mouse-bound tab
switching (critique #2 → done). What it *opens next:*

1. **Palette is nav-only; it should also act.** GitHub's palette runs commands
   ("copy permalink", "clear filters", "toggle theme"), not just navigation.
   Once we have per-session actions, surface them here.
2. **Still no durations / trace.** Unchanged from Review #1 and still the #1
   story gap. Promote to this-iteration work. → trace view (task #7).
3. **Recents/frecency.** An empty-query palette should show *recently viewed*
   sessions, not the first N by list order — Linear/VS Code rank by frecency.
4. **The header now has two nav affordances** (TabBar + palette hint) — good,
   but the tab bar could show a subtle count badge (e.g. Overview → 1.4k) so
   the numbers live where you navigate (Apple: information where the eye lands).

### Design review (GitHub · Airbnb · Claude · Apple)

- **GitHub** — palette overlay matches the quiet, dense idiom; good. Add
  result-group headers ("Navigate" / "Sessions") as sticky mini-labels rather
  than the per-row right-aligned group tag (less repetition, more GitHub-like).
- **Apple** — the overlay appears instantly; it should *animate in* (a 120ms
  scale/opacity) so it feels physical. One motion primitive, reused, would lift
  the whole app (drill-in slide, palette pop, ribbon fade).
- **Claude** — the palette's type scale is calm; extend that restraint by
  retiring one or two of the six body font-sizes app-wide.
- **Airbnb** — friendly empty state: "No matches" is terse; offer "Try a project
  or branch name" — guide, don't dead-end.

### Next iteration

Take **the trace view (task #7)** — the persistent #1 story gap. A turn rendered
as an observability waterfall: tool calls as spans with **durations** computed
from `tool_call`→`tool_result` timestamps, subagents nested. Build the pure
`tool_call`/`tool_result` pairing + duration model **test-first** (discovery:
the timestamp-pairing edge cases — missing results, interleaved calls,
out-of-order seq — are exactly what tests should surface), then the D3 view.

---

## Review #3 — After the turn trace / duration waterfall

**Shipped this iteration:** `TurnTraceView` — tool calls as a duration
waterfall (slowest ringed, failures flagged, unresolved calls hatched), mounted
in Explore (click-linked to event cards) and the Overview drill-in. Built
model-first; the pairing edge cases were discovered by the specs, not guessed.

### UX critique

Durations are finally visible (Reviews #1/#2 #1 → done). What's now exposed:

1. **The three session views don't share a spine.** Explore has ribbon+trace,
   Overview drill-in has ribbon+trace+stats, Story has sentences. A senior dev
   mentally re-orients each time. A single **SessionSummary** header (duration,
   tokens, tool count, error count, top files) reused across all three would
   make the app feel like one product.
2. **Nothing is deep-linkable except the session id.** Overview filters and the
   trace's selected span vanish on reload; you can't send a teammate "the failed
   Bash at 10:04." → shareable, URL-encoded state (Review #1 #3, still open).
3. **Errors still aren't first-class.** The trace flags them, but you must open
   a session to see it. Sidebar/list rows should carry a red error dot so you
   can triage failures without drilling in.
4. **No frecency.** ⌘K and the lists order by recency-of-event, not
   recency-of-*your-attention*. Track recently-viewed sessions client-side.

### Design review (GitHub · Airbnb · Claude · Apple)

- **GitHub Actions** — our waterfall is close to their job-timing view; adopt
  their "N% of total" per-span hint so a bar's share of the turn is legible at a
  glance, not just its absolute ms.
- **Apple** — still no motion. The trace bars and ribbon marks should ease in;
  the drill-in should slide. One shared 120–160ms transition token, applied
  consistently, is the highest-leverage polish left.
- **Claude / Airbnb** — the trace's left label column (tool + detail) competes
  with the bar for attention; Airbnb would quiet the detail to a hover and let
  the bars carry the eye. Tighten the type scale (still 6 body sizes app-wide).
- **Cross-cutting** — the color-token pass (vars exist, components use inline
  hex) is now blocking the "one accent" discipline three reviews running. Worth
  a dedicated iteration.

### Next iteration

Take **shareable session views + a shared SessionSummary header** (critique #1 &
#2). Encode Overview/Story filters and the selected session/span into the hash
(extend `lib/hash-route.ts`) so any view is a link, and extract a
`SessionSummary` model (pure, test-first — it's a fold over records) rendered as
one header across Explore / Overview / Story. Highest "feels like one product"
leverage, and directly serves telling-the-story-to-a-teammate.

---

## Review #4 — After the shared SessionSummary header

**Shipped this iteration:** `SessionSummary` fold + `SessionSummaryHeader` — one
consistent stat strip (model · duration · turns · tools · tokens · errors · top
file) atop both Explore and the Overview drill-in. Errors are now a first-class
red stat (Review #3 #3 → done). Built model-first (8 specs).

### UX critique

The spine exists; now make it *carry weight*:

1. **Half of task #8 remains: nothing is deep-linkable.** The header proves the
   views share data; the URL should too. Encode Overview/Story filters so a
   filtered view is a link. This is the single biggest "tell a teammate" lever
   still open. → next iteration (task #9).
2. **The header is present but passive.** Each stat should be a filter/jump:
   click "3 errors" → filter the trace to failures; click the top file → filter
   events to that file. GitHub makes every count a link.
3. **Story still lacks the header.** It shows sentences, not the summary spine.
   Add the header there too once it reads from a session id, not just records.
4. **Tokens need context.** "1.75k tokens" means little alone; a subtle
   in-vs-out split (we compute both) or a cost estimate would ground it.

### Design review (GitHub · Airbnb · Claude · Apple)

- **Apple** — the header is dense; a hair more vertical breathing room and a
  single hairline separator (not the current heavier border) would feel more
  considered. Consistency: the header uses `bg-[#24283b]` while the ribbon uses
  `bg-[#1a1b26]` right below — pick one surface color for the stacked block.
- **GitHub** — the model chip is the right idea; give the whole header the same
  "sub-nav" treatment GitHub uses under a repo title (quiet, monospace numbers,
  clickable counts).
- **Claude** — restraint is holding; the stat strip resists the urge to chart
  everything. Keep it. Retire one more font size (the header introduced 10/11px
  side by side — pick one).
- **Airbnb** — warmth: when a session has zero errors, a tiny green "clean" tick
  would reward the eye rather than simply omitting the stat.

### Next iteration

Take **task #9 — shareable URL-encoded Overview/Story filters**. Extend
`lib/hash-route.ts` test-first (parse⇄build round-trip is a pure function —
tests-as-discovery for the encoding edge cases: empty filters, special
characters in search, unknown params), then wire OverviewView to read initial
state from the route and push filter changes back to the hash.

---

## Review #5 — After shareable URL-encoded Overview filters

**Shipped this iteration:** Overview filters/sort/drill-in now encode into the
hash (`#/overview?project=…&sort=events&sid=…`), hydrate from a pasted link, and
mirror back via `replaceState`; a "Copy link" button in the stats bar. Built
round-trip-first (12 discovery specs).

### UX critique

Feature coverage for session visibility is now broad (ribbon, calendar, trace,
summary, palette, deep links). The compounding backlog is shifting from
*features* to *fit-and-finish* — which is where a senior dev's trust is won or
lost:

1. **Loading states still lie / jar.** "Loading sessions…" / "Loading activity…"
   are bare text; the drill-in pops in. The prior senior-dev UX review
   (`ux-review-senior-dev.md`) already flagged the app "tells falsehoods" on
   boot. Content-shaped **skeletons** are the fix — perceived performance +
   polish. → next iteration (task #10).
2. **The shadcn foundation is unused.** We added `cn` + clsx + tailwind-merge +
   cva three iterations ago and never built a single `ui/` primitive. A
   `Skeleton` (and `Badge`) primitive pays that down and gives a reusable base.
3. **Empty states dead-end.** "No sessions match these filters." offers no way
   out; Airbnb would pair it with a "Clear filters" action inline.
4. **Still no clickable stats / motion** — carried forward.

### Design review (GitHub · Airbnb · Claude · Apple)

- **Apple / Airbnb** — the highest-leverage polish now is *perceived
  performance*: skeletons that match the final layout so nothing shifts (Apple's
  "no jank" bar), and empty states that feel considered, not broken (Airbnb's
  warmth). This is a whole iteration and worth it.
- **Claude** — a `Skeleton` with a calm, slow shimmer (not a frenetic pulse)
  matches the product's restraint.
- **GitHub** — their skeletons mirror the exact row shape they replace; ours
  should too (a session-row skeleton, a stat-bar skeleton), so the transition is
  invisible.
- **Design debt, 4th mention** — the color-token pass (vars vs inline hex) still
  blocks "one accent." Escalating: schedule it as its own iteration soon; it now
  costs more each feature we add on inline hex.

### Next iteration

Take **task #10 — the polish pass**. Build a shadcn-style `components/ui/
skeleton.tsx` (uses the dormant `cn` foundation), then replace the bare loading
text across Overview / drill-in / ribbon / trace / Story with layout-matched
skeletons, and give empty states an inline recovery action. Design-led, visible,
and it finally exercises the shadcn base the round asked for.
