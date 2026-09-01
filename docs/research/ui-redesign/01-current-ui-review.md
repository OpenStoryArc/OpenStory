# Current UI review

A grounded critique of the live OpenStory dashboard (`ui/src/`). Every claim has a count or
a `file:line` behind it — this is a read of the actual code, not a vibe.

**Verdict:** production-grade data layer, scattered presentation layer. The engine is
better than the chrome. That's a fixable, well-bounded problem.

---

## 1. Inventory at a glance

48 components across 10 subdirectories + 9 root components. Four tabs (`Live`, `Explore`,
`Story`, `Users`) switched in `App.tsx:102-149`, routed through a hash router
(`use-hash-route.ts`) so views are bookmarkable and shareable.

The five biggest / most complex components — the natural refactor candidates:

| File | LOC | Why it's heavy |
|------|-----|----------------|
| `Sidebar.tsx` | 790 | Session list + subagent tree + resizable divider + keyboard nav + filter chips, all in one file. Session-derivation logic belongs in `lib/`. |
| `Timeline.tsx` | 733 | Turn-card rendering mixed with filter logic and a hand-rolled keyboard handler. |
| `story/TurnCard.tsx` | 667 | The single most complex component — sentence diagram, collapsed applies, domain facts, reasoning blocks, 5+ nested conditionals. |
| `story/StoryView.tsx` | 646 | Pagination + filtering + rendering interleaved. |
| `events/EventCard.tsx` | 486 | 8+ payload discriminators each with bespoke rendering — a textbook strategy-map candidate. |
| `RecordDetail.tsx` | 476 | Per-record-type rendering + markdown + syntax highlight + truncation. |

These aren't bad code — they're *grown* code. Each would shrink dramatically once primitives
(`Card`, `Button`, `Badge`) and a few extracted sub-components exist to compose from.

---

## 2. Styling — the biggest liability

A clean Tokyonight token system **exists** in `ui/src/index.css:4-17`:

```css
:root {
  --bg: #1a1b26;  --bg-surface: #24283b;  --bg-hover: #2f3348;
  --text: #c0caf5;  --text-muted: #565f89;  --accent: #7aa2f7;
  --green: #9ece6a; --red: #f7768e; --orange: #e0af68;
  --purple: #bb9af7; --cyan: #2ac3de;
}
```

…and then almost nothing uses it. The components inline the **raw hex values** into Tailwind
arbitrary-value classes instead:

- **643 hardcoded hex instances** embedded in `className` strings across the 48 components.
- The CSS variables are referenced in **~12 places total** — body background, scrollbar,
  prose/markdown overrides, one animation. That's it.
- Distribution of the worst offenders: `#565f89` (muted) ×200, `#7aa2f7` (accent) ×80,
  `#2f3348` (border) ×76, `#c0caf5` (text) ×69, `#24283b` (surface) ×51.

The pattern everywhere is `className="bg-[#1a1b26] text-[#c0caf5] border-[#2f3348]"`
(e.g. `App.tsx:88`, `TabBar.tsx:28-30`). Dynamic colors are the one bright spot — derived
through pure functions (`sessionColor()`, `personColor()`, `sessionChipStyle()`) and applied
via the `style` prop, which is correct and testable.

**Consequence:** the design system is decorative, not load-bearing. Changing a theme color,
shipping a light mode, or adjusting contrast means a 643-line sweep across 48 files. The
tokens promise themeability the components don't honor.

**This is the highest-leverage fix in the whole codebase**, and it's mostly mechanical.

---

## 3. Primitives — fragmented

Three primitives are genuinely well-built and should be *kept and composed on top of*, not
thrown away:

- `TabBar.tsx` — semantic `role="tablist"` / `role="tab"` / `aria-selected`. Correct.
- `PersonChip.tsx` — deterministic color, initials avatar, active-now pulse, `aria-label`.
- `sessionChipStyle()` — returns `{fg, bg, border}` so callers don't string-concat alpha.

Everything else is hand-rolled and inconsistent:

| Primitive | State today |
|-----------|-------------|
| **Button** | ~68 buttons, 10+ distinct styling patterns. No `Button` component. Theme/branding changes are impossible to make consistently. |
| **Card** | 12+ card patterns (`rounded-xl border border-[#2f3348]` ± `ring-1`), each re-rolling spacing and border. No `Card` wrapper. |
| **Badge/Chip** | 15+ patterns; 4 distinct chip implementations, inconsistently sized (`text-[10px]` vs `text-[11px]`). |
| **Input** | Exactly one (`SemanticSearch.tsx`). Anything form-shaped would start from scratch. |
| **Table** | None semantic. `FileImpactTable` is a flex `div` list, not a `<table>`. |
| **Dialog / Sheet** | None. All interaction is in-place or via navigation. |
| **Command palette** | None — a real gap for a keyboard-first senior-eng audience. |
| **DropdownMenu / Select** | None. |
| **Tooltip** | Native `title` attributes only (lightweight + accessible, but unstyleable). |
| **Toast** | None — errors have no transient surface. |

A primitive audit would extract roughly 10–15 reusable components and collapse a large
fraction of the per-component styling.

---

## 4. Accessibility — selective, not systematic

Coverage is concentrated in three components and thin everywhere else (~9/48 have any a11y
attributes).

**Good:** `TabBar` (full tab semantics), `Sidebar` + `Timeline` (working arrow/Enter/Escape
keyboard nav, a `data-focus-zone` concept for focus management), correct `aria-hidden` on
decorative dots.

**Gaps:**
- **377 `div`s used as interactive elements** with `onClick` but no `role`, no keyboard
  handler, no focus styling. Sidebar session rows, Timeline rows, and Timeline filters are
  all `div`-with-onClick.
- Icon-only buttons (close, expand/collapse, clear-filter) have no `aria-label`.
- No `aria-expanded` on expandable sections; no `aria-live` on the timeline despite events
  streaming in; no landmark elements (`<main>`, `<nav>`, `<section>`).
- Explore and Story views have no keyboard navigation at all.

Roughly WCAG Level A with gaps. The codebase optimizes for *sighted keyboard power users* in
a few components and leaves the rest mouse-only. A systematic pass — semantic elements +
`aria-*` + focus rings — is squarely in scope for a design-system migration, because the
primitives that fix it (Radix-backed Button, Dialog, DropdownMenu) ship a11y by default.

---

## 5. What's genuinely good (keep all of this)

The migration must not endanger the parts that are excellent:

1. **RxJS stream architecture** (`streams/`) — `connection.ts` does
   `timer + switchMap + tap + catchError + retry` for resilient WebSocket reconnect with
   status tracking. Decoupled from React lifecycle, type-safe, testable.
2. **Pure-function `lib/`** — ~70 functions for transforms, color derivation, filtering,
   time. Testable in isolation, no side effects. This is the SICP-grade core.
3. **Deterministic color** — `sessionColor()` / `personColor()` via djb2 hash → stable,
   predictable colors across the whole UI even when lists reorder.
4. **BDD coverage** — unit specs for stream reducers and pipelines, E2E for filters, deep
   links, timeline, sidebar. This is the safety net that makes a 48-file sweep *safe*.
5. **`useObservable()`** — clean RxJS↔React bridge with initial-state fallback.
6. **Tokyonight commitment** — one coherent palette; the appearance is polished and modern.
   (The problem is *how* it's applied, not the palette itself.)
7. **Markdown + Prism** with a custom Tokyonight code theme and language detection.

**The asymmetry is the whole story.** A 7/10-architecture codebase with a 9/10 data layer
and a 4/10 presentation layer. A design-system pass is the single change that closes the gap
without touching the engine — which is exactly why it's worth doing carefully.

---

## 6. The fix, in one breath

1. **Tokenize** the 643 hex values → CSS-variable-backed semantic classes. Mechanical,
   test-guarded, theming fixed.
2. **Extract** ~10 primitives; let the giants compose from them and shrink.
3. **Adopt semantic + Radix-backed components** for the a11y win for free.
4. **Decompose** `Sidebar`, `Timeline`, `TurnCard` once primitives exist to build with.

[`02-shadcn-migration.md`](02-shadcn-migration.md) turns this into a concrete, phased,
honestly-costed plan.
