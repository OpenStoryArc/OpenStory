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

---

## Review #6 — After the Skeleton polish pass

**Shipped this iteration:** first shadcn `ui/` primitive (`Skeleton`, calm
shimmer + reduced-motion), layout-matched skeletons across Overview / drill-in /
Explore, and a warm empty state with an inline "Reset filters" recovery. Loading
no longer jars; the shadcn base is finally live.

### UX critique

The surfaces look finished; now make the *numbers do something*:

1. **Stats are still passive.** The SessionSummary header shows "3 errors" and a
   top file but you can't act on them. Make each stat a verb: click errors →
   jump to the first failure; click the top file → filter events to it. GitHub
   makes every count a link; this is the cheapest way to turn *visibility* into
   *navigation*. → next iteration (task #11).
2. **Story still lacks the summary spine.** Once the header can take a session
   id (not just records), Story should carry it too (Review #4 #3, still open).
3. **No frecency / recents** — carried; ⌘K and lists still order by event
   recency, not attention recency.

### Design review (GitHub · Airbnb · Claude · Apple)

- **GitHub** — clickable counts are the pattern to copy: a hover underline + a
  cursor change tells the eye a number is a link. Apply to the summary stats.
- **Apple** — with skeletons in, the last jank is the *transition* skeleton →
  content (a hard swap). One shared fade token would finish the perceived-
  performance story. Still worth a small motion iteration.
- **Claude / Airbnb** — restraint holding; the empty state's new warmth is the
  right direction — extend it to the ⌘K "No matches" state next time we touch it.
- **Design debt — 5th mention, now escalated to a scheduled iteration.** The
  color-token pass (CSS vars exist; components hardcode inline hex) blocks the
  "one accent" discipline and taxes every new component. Filing task #12 to do
  it deliberately: migrate inline hex to the `--bg/--accent/...` tokens, then
  enforce one primary accent with the rest demoted to data-encoding.

### Next iteration

Take **task #11 — actionable summary stats**. Extract a pure helper
(`firstErrorEventId(records)` / file-jump target — test-first, a fold over
records) and wire the SessionSummary header stats to jump/filter in Explore
(errors → first failure card, top file → file facet). Turns the one-product
spine from a readout into a control surface. Then schedule the token-pass
(task #12).

---

## Review #7 — After actionable summary stats

**Shipped this iteration:** `firstErrorEventId` helper + clickable header stats
(errors → jump to first failure, top file → filter events). The spine is now a
control surface (Review #6 #1 → done).

### UX critique

1. **Attention has no memory.** Five reviews have flagged it: ⌘K and every list
   order by *event* recency, never by what *you* just looked at. A senior dev
   revisiting work wants "the three sessions I was just in" at the top. Frecency
   is pure, testable, and low-risk. → next iteration (task #13).
2. **Story still has no summary spine.** Needs a records fetch for the selected
   session (the SessionVizLoader pattern) — safe, deferred again only for
   sequencing. Task #14.
3. **⌘K is still nav-only, empty state is bare.** Once recents exist, the empty
   palette should show them; and palette should eventually run actions.

### Design review (GitHub · Airbnb · Claude · Apple)

- **Apple / Linear** — "recently viewed" at rest is a hallmark of a tool that
  respects your flow; pair it with the ⌘K empty state so the palette is useful
  before you type a character.
- **GitHub** — the clickable-count pattern from this iteration should propagate:
  make the Overview stat-bar numbers (sessions/events) clickable too (e.g.
  events → sort by events).
- **Claude** — hold the line on restraint; recents should be a short, quiet list
  (max 5), not a second feed.
- **Design debt — color-token pass (task #12).** Deliberately NOT auto-run in
  this loop: it's a wide inline-hex→CSS-var refactor whose failure mode is
  *visual* regression, which the (logic-only) test suite can't catch and this
  environment can't screenshot. It's documented and ready for a supervised pass
  rather than an autonomous one. This is the honest call, not avoidance.

### (later iterations continue below)

Take **task #13 — frecency / recently-viewed sessions**. A pure ranking module
(`lib/recents.ts`: record a visit, rank by recency×frequency — test-first, the
decay/ordering is exactly what specs should pin down), persisted at the edge
(localStorage), surfaced in the ⌘K empty state and optionally an Overview
"Recent" strip. Addresses the most-repeated open critique with a low-risk,
well-tested change.

---

## Review #8 — After frecency / recently-viewed sessions

**Shipped this iteration:** `lib/recents.ts` frecency ranking + `useRecents`;
the ⌘K palette now leads with your 5 most-recent sessions on an empty query.
The app finally has attention memory.

### UX critique

The session-visibility feature set is now mature (ribbon, calendar, trace,
actionable summary, deep links, palette + recents, skeletons). Remaining gaps
are about *reach and consistency*, not new capability:

1. **Story is the odd one out.** Explore and Overview share the SessionSummary
   spine; Story still opens to sentences with no at-a-glance header. Closing
   this makes the three views feel like one product. → next iteration (task #14).
2. **Recents live only in ⌘K.** A quiet "Recent" strip atop the Overview list
   (or its empty state) would surface them where the eye already is. Small.
3. **Overview stat-bar numbers still aren't clickable** (Review #7 GitHub note).
   Cheap follow-on: events → sort by events, etc.

### Design review (GitHub · Airbnb · Claude · Apple)

- **Apple / Airbnb** — consistency is the theme now: the *same* summary spine on
  every session surface is exactly the kind of coherence that makes a tool feel
  designed rather than assembled. Prioritize reach over new widgets.
- **Claude** — recents in ⌘K are appropriately quiet (max 5, labeled). Keep any
  new "Recent" strip equally restrained.
- **Design debt — token pass (task #12)** still queued for a supervised pass;
  unchanged. Every new surface added on inline hex raises its eventual cost, but
  the honest tradeoff (visual regression risk vs. this env's no-screenshot
  limit) still says: not autonomously.

### Next iteration

Take **task #14 — the Story summary spine**. A small `SessionSummaryLoader`
(fetch records for the selected session → render the shared
`SessionSummaryHeader`) mounted atop the Story main pane, so all three session
views share one header. Safe, reuses existing tested components; verify via the
build + a focused loader test (mocked fetch → header renders).

---

## Review #9 — After the Story summary spine

**Shipped this iteration:** `SessionSummaryLoader` → the shared spine now sits
atop Explore, the Overview drill-in, AND Story. The three session views cohere.

### State of the round

Twelve iterations in, the session-visibility mandate is substantially met:

- **See a session's shape** — activity ribbon (D3 swimlanes + token burn).
- **See where time went** — tool trace waterfall with durations.
- **See it at a glance** — the shared, actionable SessionSummary spine (errors
  and files are clickable).
- **See the whole corpus** — Overview dashboard (calendar heatmap + facets +
  sortable list), shareable via URL.
- **Find/return to sessions** — ⌘K palette with frecency recents, Story find.
- **Read it honestly** — harness-message untruncation, skeletons, warm empties.

### UX critique — remaining work is polish and reach, not capability

1. **Recents live only in ⌘K.** Surface a quiet "Recent" strip atop the Overview
   list so frecency shows where the eye already is. Small, safe. → next (task #15).
2. **Overview stat-bar numbers still inert** (Review #7). Cheap: events → sort by
   events, sessions → clear filters.
3. **Motion** — the one unshipped design-review ask (Apple). A single shared
   transition token remains the highest-leverage pure-polish item.

### Design review (GitHub · Airbnb · Claude · Apple)

- **Apple / Airbnb** — coherence achieved (same spine everywhere); the product
  now reads as designed. The next tier of "considered" is *motion* — but it's
  hard to verify without a screenshot here, so it's a supervised-friendly item.
- **Design debt — token pass (task #12)** — still the biggest deliberate
  omission, still supervised-only for the stated reason (visual-regression risk
  vs. no-screenshot env). It is documented and ready.

### Loop status

The big gaps are closed. Remaining items are small polish (recents strip,
clickable stats) plus two supervised-only iterations (color tokens, motion). A
PR off this branch is ready whenever wanted. The loop continues on low-risk
polish until redirected or stopped.

### Next iteration

Take **task #15 — an Overview "Recent" strip**. Reuse `useRecents` + the session
list to render up to 5 frecency-ranked recent sessions as a quiet row above the
list, click-to-drill-in. Verify via a focused test (given recent ids + sessions,
the strip renders those sessions in order).

---

## Review #10 — Round retrospective (after the Overview Recent strip)

**Shipped this iteration:** Overview "Recent" strip (frecency where the eye is) +
clickable stat-bar numbers. That closes the last of the small, autonomously-
verifiable polish items.

### The round, end to end (13 feature iterations, ~1458 UI tests, all TDD-first)

The mandate — *give visibility into sessions* — is met across every axis:

| Question a senior dev asks | Answer shipped |
|---|---|
| What did this session *do*, and when? | D3 activity ribbon (swimlanes + token burn) |
| Where did the *time* go? | Tool-trace duration waterfall |
| Can I see it *at a glance*? | Shared, clickable SessionSummary spine (all 3 views) |
| What's the *whole picture*? | Overview dashboard: calendar heatmap + facets + sorts |
| Can I *share* a view? | URL-encoded Overview filters + Copy link |
| How do I *find / return* to a session? | ⌘K palette + frecency recents; Story find |
| Is what I'm reading *honest*? | Harness-message untruncation; skeletons; warm empties |

Process held throughout: every feature = pure model + failing spec first, then
component; a UX + brand-design review each iteration; nothing merged red.

### What's deliberately NOT done, and why

Three items are in `docs/BACKLOG.md` under "UI — follow-ups", not skipped by
oversight: the **color-token pass**, a **motion primitive**, and **⌘K actions**
/ **subagent lanes**. Each shares one property — its failure mode is *visual*
regression, which the logic-only suite can't catch and this loop's environment
can't screenshot. Running them blind would trade the round's most valuable
asset (a green, trustworthy suite) for unverifiable churn. They're specced and
ready for a supervised pass. This is the honest engineering call.

### Design review — closing note (GitHub · Airbnb · Claude · Apple)

The app now reads as *designed, not assembled*: one spine on every session
surface (Apple/Airbnb coherence), keyboard-first navigation with memory
(Linear), shape-matched skeletons (GitHub), calm restraint in type and motion
(Claude). The single largest remaining lift to "brand-leader" polish is motion —
and it's a supervised item.

### Status & recommendation

The round has reached a natural completion point. Branch `feat/ui-session-
visibility` is green (tsc + 1458 tests + build) and PR-ready. Recommended next
action is **human review / open a PR**, then tackle the BACKLOG UI items with a
visual pass. Absent direction, the loop slows its cadence rather than manufacture
marginal features.

---

## Review #11 — Deep review: Explore, Story, architecture friction, subagents, wowser viz

Prompted by Max for a full design/UX + architect pass, with special attention to
the Explore page, Story usability, whether session reports include subagents, and
"wowser" interactive-canvas ideas. Grounded in the live data (dogfooded via the
REST API) — 1428 sessions, of which **623 (44%) are orphaned `agent-*` subagent
sessions**.

### Subagents — the headline finding (partially shipped this iteration)
A session report did **not** include its subagents. Subagents run as separate
`agent-<id>` sessions with **no parent link materialized** in the data model, so
they were invisible from their parent AND flooded every list (44% noise).
- **Shipped:** `lib/subagents.ts` reconstructs the edge (parent `Agent`
  tool_result echoes `agentId:<hex>` → child `agent-<hex>`); a Subagents section
  in the drill-in; Overview hides `agent-*` by default with a toggle.
- **Still open (backend, architect hat):** the link should be *materialized at
  ingest* (`parent_session_id` on the child session), not reconstructed in the
  client each time. Same for surfacing subagents in the **Explore** view (only
  the Overview drill-in has them so far).

### Architecture / backend friction (architect hat)
1. **Subagent orphaning** (above) — no parent edge in the store.
2. **Token totals exclude the prompt cache at the SOURCE.** The session-list
   `total_input_tokens` omits `cache_read`/`cache_creation`; on a real session
   that under-reported ~224× (4.2M shown vs 949M true). Fixed the per-session
   records-derived view (`TokenReport`), but the *list aggregate* is still wrong
   until the projection includes cache — a `projection.rs` change.
3. **Unpaginated `/records`.** Ribbon, trace, summary, token report, and
   subagents all fetch the *full* record set per session; a 12k-event session
   ships everything each open. Fine now, a wall at scale (matches the known
   read-path ceiling). Wants keyset pagination or a lightweight summary endpoint.
4. **Lossy label at source** (`projection.rs:302`, 50-char first-prompt) — the UI
   cleans harness noise at render, but search/exports still see the raw label.

### Explore page — movement & clickability
Layout: left nav (turn outline + file/tool/plan facets) → right event cards, now
topped by summary spine + ribbon + trace. What works: ribbon mark → scrolls to
event; trace span → scrolls to event; summary "errors →" / top-file → jump/filter;
arrow-key sidebar↔cards. Friction:
- **Inconsistent session-finding.** Overview and Story have search + facets;
  Explore's session picker (`ExploreSidebar`) does not. Finding a session differs
  per tab — should be unified (ideally the ⌘K palette is the one true finder).
- **Active-filter state is invisible + not deep-linkable.** Multiple facets read
  only as "N of M events"; the active facets aren't shown as removable chips, and
  (unlike Overview) they aren't encoded in the URL.
- **No subagents in Explore** (only the Overview drill-in) — inconsistent.
- **No "next error / next turn" intra-session jumps** despite having the data.

### Story page — usability
Flat virtualized turn list (`estimateSize 140`). Good ideas from Max, all viable:
- **Compress turns into groups of ~10** — collapsible "Turns 1–10" decade headers
  (pure grouping over the sentence list; a natural test-first lib fn), so long
  sessions are scannable and you can collapse the boring stretches.
- **Sticky turn/decade header** while scrolling; a **jump-to (top / next
  terminal turn / next error)** control; a thin **minimap** of turn categories.

### Pain points in the test-driven process (asked directly)
- **jsdom lacks `scrollIntoView`/`scrollTo`** → scroll-aware components threw
  unhandled errors mid-test. **Fixed this iteration** with a global stub in
  `tests/setup.ts` (removes a recurring footgun).
- **No full `<App>` mount test** → App-level runtime crashes (e.g. the blank-page
  regression) aren't caught by the suite; a mocked-connection smoke test would.
- **Split value/label spans** make `getByText('N tool')` fail (must use
  `toHaveTextContent`) — a documented testing convention/helper would save churn.
- **Per-file fetch re-mocking** is brittle; a shared API-mock helper would reduce
  friction and the "wrong shape" surprises (hit once with `/synopsis`).

### Wowser visual — the standout idea
Now that parent→subagent edges are recoverable, the natural "Figma-like"
interactive screen is a **session constellation / agent graph**: a pan-zoom
canvas where a session is a node and its subagents fan out as linked child nodes
(recursively), sized by event/token volume, colored by status, edges = spawn
relationships. Click a node → its report; drag to explore; the recent multi-agent
*audit* (dozens of `agent-*` children) would render as a striking tree. This is
genuinely novel visibility (no tool shows your agent *delegation graph*) and
builds directly on this iteration's linkage. Runner-up: a session **flame graph**
(turns × depth × time) and a **live fleet canvas** of active sessions.

### Next iterations (proposed)
1. **Add the Subagents section + summary spine to Explore** (consistency; small).
2. **Story decade-grouping** (collapsible groups of 10 + jump control) — test-first.
3. **The agent constellation graph** (the wowser canvas) — the flagship next build.
4. Backlog (backend/supervised): materialize `parent_session_id`; cache tokens in
   the projection aggregate; `/records` pagination; full-`<App>` smoke test.

---

## Review #12 — Hands-on UX audit (subagent) + Ask panel + Canvas polish

**Date:** 2026-07-01 · **Branch:** `feat/ui-session-visibility`

This iteration paired a build round with a **hands-on UX review subagent** that
drove every surface with stock Chromium, screenshotted each state, and returned a
prioritized P0→P2 findings report. Highlights below; the fixes shipped this
iteration are marked ✓.

### New this iteration (built)
- **Ask panel** (`#/ask`) — the first agent-in-UI step, done sovereignty-safe
  (Pattern 3): a read-only "ask your fleet" surface answering 7 curated questions
  purely from the sessions list (latest / today / running / token burners /
  longest / active projects / **agents & efficiency**). No writes, no LLM,
  nothing leaves the machine. Honest "no token telemetry" for uninstrumented
  agents. Verified: claude-code 220 vs pi-mono 36 out-tok/event.
- **Scatter marquee-select** — drag a box over the efficiency cloud → linked list
  of those sessions ranked by output tokens, click-through to Explore. Pure,
  tested `pointsInBrush` (data-space filter). Gated behind a "Select" toggle so
  the overlay doesn't steal point clicks.
- **Gantt empty-band collapse** — `visibleGantt(model, window)` drops bands with
  no bar in-window and re-bases lanes, so narrowing no longer leaves labeled
  empty strips. Tested.

### UX-review findings — addressed this iteration ✓
- **P0 #1 Flow perpetual "loading…" on the default agent** ✓ — `Promise.all`
  blocked on the live 2833-event session's records fetch. Fixed with per-fetch
  8s AbortController timeout + graceful drop ("N skipped (too large)").
- **P1 #3/#4/#15 Session titled 4 different ways per tab** ✓ — promoted Overview's
  private title logic to shared `lib/session-title.ts::sessionTitle()`; Overview,
  Explore (parent+subagent), Story, and ⌘K now agree. Explore went from raw
  `<command-message>` XML / bare hex → readable prompts.
- **Truncation artifact** ✓ — `cleanHarnessPreview` now strips dangling harness
  tag fragments (`…</command-message>`) from plain labels.
- **P1 #6 ⌘K couldn't reach Canvas/Ask** ✓ — added both to TAB_ITEMS (all 8 nav
  routes reachable).

### UX-review findings — queued (next iterations)
- **P1 #2** Session totals disagree across surfaces (807 / 1028 / 1436 / 1000) —
  need one canonical count or a consistent "main · subagents" qualifier.
- **P1 #5** Story lands on an empty subagent — default-select the top session
  that actually has sentences.
- **P1 #7** Palette match highlighting; **#8** Board auto-fit on entry;
  **#9** Conversation renders raw markdown/command wrappers;
  **#10** Scatter 1-event pile-up on the y-axis (jitter/annotate);
  **#11** Gantt/Scatter agent-color legend.
- **P2** Board/Gantt/Ask vertical dead space; empty-state copy; facet counts vs
  active search; console `net::ERR_ABORTED`/404 noise.

### What the review said is genuinely good (keep)
Overview drill-in ribbon+token panel, Story's eval-apply narrative (single best
feature), Admin's endpoint-honesty dots, Sunburst/Treemap, Board radial bloom,
Explore Graph constellation, the Live feed with type filters, Users cards.

---

## Review #13 — Canvas polish sprint (post-audit, self-paced loop)

**Date:** 2026-07-01 · **Branch:** `feat/ui-session-visibility`

Worked the queued findings from Review #12's hands-on audit, one tested +
screenshot-verified increment per loop firing. Each is a pure model + BDD specs,
then the component. Commits `b303c61`…`b4c53ed`.

**Shipped (in order):**
1. **Board auto-fit on first load** (P1 #8) — data arrives async after the
   viewport is measured, so the fit effect fired on an empty model; added a
   fit-once-when-ready effect. Extracted pure `lib/canvas-fit.ts::fitTransform`
   (4 specs). Verified 0/4 group nodes clipped on load.
2. **Flow ribbon/node hover-highlight** — pure `linkActive(link, hover)` (4
   specs); hovering a ribbon/from-node/to-node lights its path, dims the rest.
   Verified 47/52 ribbons dimmed on hover.
3. **Scatter overplotting jitter** (P1 #10) — pure `pointJitter(id, radius)`
   deterministic disk offset (4 specs); non-zero points ±5px, zero-token gutter
   spread across its column. Verified gutter 1→222 distinct x.
4. **Sunburst inline radial labels** — pure `lib/sunburst-label.ts` fit/
   orientation (5 specs); non-leaf wedges labeled + upright-flipped on the left.
   Verified 28 labels render.
5. **Mode-tab icons + per-mode group-by note** — `lib/canvas-modes.ts` (MODE_META
   + modeUsesGroupBy, 3 specs); tabs get icon+label+tooltip, Scatter/Flow show a
   note explaining the group-by row's absence.
6. **Gantt taller concurrency-histogram overview** — pure `overviewDensity(bars,
   domain, buckets)` (3 specs); 46→68px strip showing active-sessions-over-time
   instead of folded-lane dot-soup.

**Loop-prompt checklist now:** empty-band collapse ✓, taller overview ✓, sunburst
labels ✓, Flow faster-load ✓ + hover-highlight ✓, mode icons/group-by clarity ✓,
scatter brush→list ✓ + jitter ✓, board auto-fit ✓. **Remaining:** sunburst/
treemap click-zoom *animation* (transitions) — the last unaddressed item.

**Still queued (non-Canvas, from #12):** P1 #2 session-count reconciliation,
#5 Story empty-subagent landing, #7 palette match-highlight, #9 Conversation
markdown rendering. UI test count ~1561.
