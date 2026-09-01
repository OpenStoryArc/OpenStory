# shadcn/ui — full design-system migration assessment

The brief: evaluate adopting **shadcn/ui** as OpenStory's component foundation across *all*
tabs, honestly. This document gives the real cost, the real fit, the real tension, and a
phased order that de-risks it.

**Bottom line up front:** shadcn fits this project better than its reputation suggests,
because it vendors source you own rather than importing a framework. The first phase
(tokenize) is a no-regret win independent of the shadcn decision. The genuine cost is the
Radix dependency and a 48-file sweep — both manageable behind the existing test suite. The
honest caveat: only adopt the primitives the codebase actually uses (YAGNI), or the
"minimal code" principle is violated by catalog bloat rather than served.

---

## 1. Why shadcn doesn't conflict with "minimal, honest code"

The instinct on a project whose CLAUDE.md says *"No abstractions without justification.
Three clear lines beat a clever helper"* is to reject a component library. That instinct is
right about most libraries and wrong about this one, for a specific reason:

**shadcn is not a dependency you import — it's a CLI that copies component source into your
repo.** `npx shadcn@latest add button` writes `ui/src/components/ui/button.tsx` into the
tree as ordinary, editable code. There is no `import { Button } from "shadcn"`. You own it,
you read it, you change it. That is *the same ownership model as the existing `lib/`* —
which is exactly the project's stated value (open standards, user-owned data, code you can
audit).

So the real question isn't "framework vs hand-rolled." It's: **do we hand-roll 15 primitives
from scratch, or start from audited, accessible, owned-source primitives and edit them?**
Given that the hand-rolled set is currently 68 inconsistent buttons and zero dialogs, the
honest answer leans toward starting from a good baseline.

The reconciliation with principle #7: a `Button` primitive that replaces 68 divergent
buttons is *removing* complexity, not adding it. The justification is concrete and
measurable (643 hex → tokens, 68 buttons → 1 component, 0 → systematic a11y).

---

## 2. The genuine cost: dependencies

This is where to be honest. shadcn primitives are thin wrappers over **Radix UI**
primitives. Adopting them adds runtime dependencies that the UI currently does *not* have
(today's `package.json` has zero UI-component deps — just React, RxJS, Recharts, markdown).

Per [[feedback_audit_deps]], the deps a *need-based* adoption pulls in:

| Package | Used by | Notes |
|---------|---------|-------|
| `@radix-ui/react-dialog` | Dialog, Sheet | Report viewer, Command-center focus mode. Mature, widely audited. |
| `@radix-ui/react-dropdown-menu` | DropdownMenu | Filter menus. |
| `@radix-ui/react-tooltip` | Tooltip | Replaces native `title`. |
| `@radix-ui/react-scroll-area` | ScrollArea | Live tickers in the watch grid. |
| `cmdk` | Command (⌘K) | Keyboard-first palette. ~small, focused. |
| `class-variance-authority` + `clsx` + `tailwind-merge` | variant styling | Tiny utility deps shadcn generates. |
| `lucide-react` | icons | Optional — could keep using inline SVG/glyphs. |

`react-resizable-panels` (Resizable) is **already a dependency** — the Sidebar divider
should be re-expressed through it rather than hand-rolled.

These are all tree-shakeable, accessibility-grade, and individually small. But they *are*
new supply-chain surface. The mitigation is the YAGNI rule in §5: pull only what the tabs
need, audit each at add-time, and pin versions.

**Compatibility note:** shadcn supports **Tailwind v4** and **React 19** (the project's
exact stack) as of the current CLI. The `components.json` + CSS-variable theming path is the
v4-native one. Verify at adoption time, but there's no known blocker.

---

## 3. The leverage point: Tokyonight → shadcn tokens map 1:1

shadcn themes through CSS variables with semantic names. OpenStory's palette maps directly
onto them — which means **the tokenize sweep and the shadcn foundation are the same work**:

| shadcn token | OpenStory value | Current liability it absorbs |
|--------------|-----------------|------------------------------|
| `--background` | `#1a1b26` | `bg-[#1a1b26]` everywhere |
| `--card`, `--popover` | `#24283b` | `bg-[#24283b]` ×51 |
| `--muted` | `#2f3348` (`--bg-hover`) | `bg-[#2f3348]` / `border-[#2f3348]` ×76 |
| `--foreground` | `#c0caf5` | `text-[#c0caf5]` ×69 |
| `--muted-foreground` | `#565f89` | `text-[#565f89]` ×200 |
| `--primary` | `#7aa2f7` | `text-/bg-[#7aa2f7]` ×80 |
| `--border` | `#2f3348` | `border-[#2f3348]` |
| `--destructive` | `#f7768e` | error reds |
| `--success` (custom) | `#9ece6a` | greens |
| `--warning` (custom) | `#e0af68` | oranges |
| accent extras | `#bb9af7`, `#2ac3de` | purple / cyan |

Define these once in `index.css` under `:root` (and a future `.light`), point Tailwind's
theme at them, and every `bg-[#1a1b26]` becomes `bg-background`. The 643-instance sweep is
then a guided find-and-replace, not a redesign — and at the end, shadcn components dropped in
inherit the exact Tokyonight look automatically because they read the same tokens.

---

## 4. Bespoke → primitive map

What each current pattern becomes. "Keep + compose" means the existing good encapsulation
stays and is rebuilt *on top of* the primitive, not discarded.

| Today | shadcn primitive | Disposition |
|-------|------------------|-------------|
| 68 ad-hoc buttons | `Button` (variants: default/ghost/outline/destructive) | Replace |
| 12+ card patterns | `Card` (+ `CardHeader`/`CardContent`/`CardFooter`) | Replace |
| 15+ chip patterns | `Badge` | Replace base; **keep** `PersonChip`/`sessionChipStyle` composed on `Badge` |
| `TabBar` (already good) | `Tabs` | Optional — small win; could leave as-is |
| native `title` tooltips | `Tooltip` | Replace where styling/positioning matters |
| `SemanticSearch` input | `Input` | Replace |
| `FileImpactTable` div-list | `Table` | Replace (gains semantics + a11y) |
| Sidebar resizable divider | `Resizable` (dep already present) | Re-express |
| — (missing) | `Dialog` / `Sheet` | **New** — report viewer, focus mode |
| — (missing) | `Command` (⌘K) | **New** — palette: jump to session, open report, switch tab |
| — (missing) | `DropdownMenu` | **New** — filter menus |
| — (missing) | `ScrollArea` | **New** — watch-grid tickers |
| — (missing) | `Sonner` (toast) | **New** — transient error surface |
| — (missing) | `Skeleton` | **New** — loading states for REST fetches |

`Recharts` stays — shadcn's chart layer is also Recharts-based, so the existing analytics
charts are already compatible and just need token-driven colors.

---

## 5. What *not* to adopt (the YAGNI guard)

A full migration done wrong means dumping all ~50 shadcn components into the repo "for
completeness." That would violate the very principle this project holds. Don't. The
component set above is need-derived from the actual UI + the two new tabs. Skip (until a
real use appears): `Accordion`, `Carousel`, `Calendar`, `Form`+`react-hook-form` (no forms
here), `NavigationMenu`, `Menubar`, `Pagination` (Story's pager is fine), `Avatar` (PersonChip
already does this), `Toggle`, `Slider` (unless the Explore time-slider wants it).

"Full design-system migration" means *every tab is rebuilt on the shared primitives*, not
*every shadcn component is installed*. Coverage of the codebase, not coverage of the catalog.

---

## 6. Phased order (de-risked, each phase independently valuable)

**Phase 0 — Foundation (no visual change).** `shadcn init`, generate `components.json`,
wire the Tokyonight values into CSS-variable tokens, point Tailwind v4 at them. Baseline the
current UI with Playwright screenshots. *Outcome: shadcn installed, UI pixel-identical.*

**Phase 1 — Tokenize (the no-regret win).** Sweep 643 hex → semantic token classes
(`bg-background`, `text-muted-foreground`, `border-border`…). Mechanical, guarded by the
screenshot baseline + existing E2E. *Outcome: theming fixed, light-mode now possible, shadcn
drop-ins will match automatically.* **This phase is worth doing even if shadcn is rejected.**

**Phase 2 — Replace primitives.** Button → Card → Badge → Input → Tooltip → Table, one PR
each, behind tests. The giants (`Sidebar`, `Timeline`, `TurnCard`) shed styling as they
adopt primitives. *Outcome: consistent chrome, a11y baseline rises for free.*

**Phase 3 — New capabilities (these power the two new tabs).** `Dialog`/`Sheet`, `Command`
(⌘K), `DropdownMenu`, `ScrollArea`, `Sonner`. *Outcome: report viewer, focus mode, palette,
filter menus, error toasts all become available.*

**Phase 4 — Decompose the giants.** With primitives in hand, split `TurnCard` (667),
`Sidebar` (790), `Timeline` (733), `EventCard` (486) into composed sub-components; move
derivation logic into `lib/`. *Outcome: files small enough to hold in one context, easier to
test and edit.*

Phases 0–1 are the commitment-light entry; you can stop after Phase 1 and have captured most
of the value. Phases 2–4 are where shadcn earns its dependency.

---

## 7. Risks & honest caveats

- **Supply chain.** New Radix/cmdk deps vs today's zero-UI-dep posture. Audit each at add
  ([[feedback_audit_deps]]); pin versions; the need-based set keeps the surface small.
- **48-file sweep.** Real effort and real regression risk. The Playwright screenshot baseline
  (Phase 0) makes regressions mechanical to catch; don't start Phase 1 without it.
- **Don't endanger the engine.** RxJS streams, `lib/`, deterministic color, and the test
  suite are the crown jewels — the migration is presentation-only and must leave them
  untouched.
- **Catalog creep.** The failure mode is installing components you don't use. §5 is the
  guard; treat it as a rule, not a suggestion.
- **Light mode is a consequence, not a goal.** Tokenizing makes it *possible*; shipping it is
  a separate decision. Don't let it expand scope.

---

## 8. Recommendation

Do **Phase 0 + Phase 1 first as a self-contained branch** off a fresh master
([[feedback_fresh_branch_from_master]]) — it's the highest-value, lowest-risk change and
stands on its own. Make the full-shadcn call *after* seeing the tokenized UI: by then the
foundation is in, the bridge is proven, and adopting primitives is incremental rather than a
leap. The two new tabs (Command, Reports) are the forcing function for Phase 3 — design them
against shadcn primitives from the start so they don't add a *third* styling dialect.
