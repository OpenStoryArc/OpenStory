# UI Redesign — research spike

A grounded review of the OpenStory dashboard, an assessment of adopting **shadcn/ui**
as the project's component foundation, and wireframes for two net-new tabs: a **Command
center** (live 2×2 watch grid) and a **Reports** gallery/viewer (open the HTML reports
agents already generate).

This is a **spike** — exploration, not a commitment. Nothing here touches the production
UI. The artifacts are threads to pull on, not a plan to approve. Audience: a senior
engineer who wants density, keyboard-first control, and zero fluff.

## Scope (decided up front)

- **Depth:** review + wireframes only. No production code in this spike.
- **shadcn role:** evaluate a *full* design-system migration (shadcn as the component
  foundation across all tabs), honestly — including where it rubs against this project's
  "minimal, honest, no-abstractions-without-justification" principle.
- **New surfaces:** two separate tabs — Command and Reports.

## Files

| File | What it is |
|------|------------|
| [`01-current-ui-review.md`](01-current-ui-review.md) | Grounded critique of the live UI — styling, primitives, accessibility, what's genuinely good, refactor candidates. Every claim has `file:line` or a count behind it. |
| [`02-shadcn-migration.md`](02-shadcn-migration.md) | Full-migration assessment. Bespoke→primitive map, the Tokyonight→shadcn-token bridge, a phased order, the dependency/philosophy honest-take, and what *not* to adopt. |
| [`03-command-center-tab.md`](03-command-center-tab.md) | ASCII wireframes for the Command tab. Builds on the existing [`../watch-command-center/`](../watch-command-center/) exploration, re-housed as an integrated tab with shadcn chrome. |
| [`04-reports-tab.md`](04-reports-tab.md) | ASCII wireframes for the Reports tab — gallery + viewer for agent-generated HTML reports. |
| [`mocks/shadcn-prototype.html`](mocks/shadcn-prototype.html) | **Interactive** single-iteration prototype. Click the Command / Reports tabs, click reports, press ⌘K. Authentic shadcn token system + Card/Button/Badge/Command fidelity, mapped to Tokyonight. No build/deps — open directly. **Start here.** |
| [`mocks/reports-tab.html`](mocks/reports-tab.html) | Earlier static wireframe of the Reports gallery (superseded by the interactive prototype; kept for reference). |

The Command tab already has a self-contained static mock at
[`../watch-command-center/command-center.html`](../watch-command-center/command-center.html);
this spike reuses it rather than duplicating.

## The one-paragraph review

The **data layer is production-grade** — RxJS streams with resilient reconnect, ~70 pure
functions in `lib/`, deterministic color assignment, real BDD coverage. The **presentation
layer is scattered**: 643 hardcoded hex values inlined in `className` strings completely
bypass the CSS-variable token system that already exists; ~19% of components have any a11y
attributes and 377 `div`s carry `onClick` without being buttons; and nine primitive
categories (Button, Card, Badge, Input, Dialog, Command, DropdownMenu, Table, Tooltip) are
either missing or hand-rolled inconsistently across 48 components. **Architecture maturity:
strong engine, weak chrome.** That asymmetry is exactly what a design-system pass fixes.

## Why shadcn actually fits here (the non-obvious part)

shadcn is not a dependency-heavy framework — it's a **CLI that vendors component source
into your repo**. You own the code; it's yours to edit. That aligns with this project's
"user-owned, minimal, honest code" ethos far better than the name suggests. The genuine new
dependency is **Radix primitives** (accessibility-grade, well-audited, tree-shakeable) —
worth a deps audit ([[feedback_audit_deps]]), but not a black box.

The leverage point: shadcn themes via CSS variables (`--background`, `--foreground`,
`--primary`, `--border`, `--destructive`…). The 643 hardcoded hex values map **1:1** onto
those tokens. So the migration's first phase — tokenize — simultaneously kills the biggest
liability *and* lays the shadcn foundation. One sweep, two wins.

## Threads to pull on

1. **Tokenize first, decide shadcn second.** The hex→CSS-var sweep is valuable on its own
   merits and is a no-regret move whether or not shadcn lands. It's the natural first commit.
2. **Adopt primitives by need, not by catalog.** YAGNI applies: the new tabs need `Dialog`,
   `Command`, `ScrollArea`, `Resizable` (already a dep via `react-resizable-panels`),
   `Badge`. Don't import all 50 shadcn components.
3. **The two tabs are independently shippable.** Reports is mostly plumbing over artifacts
   that already exist (needs a `/api/reports` manifest endpoint + `<meta>` tags in the
   generators). Command is a new live view over the existing `events.>` stream — no new
   transport, reuse the dataflow ([[feedback_reuse_existing_dataflow]]).
4. **New tabs get their own home** ([[feedback_new_tab_for_visuals]]) — Command and Reports
   join Live / Explore / Story / Users rather than being folded into one.
5. **Visual-regression safety net.** A full migration touches all 48 files; baseline the
   current UI with Playwright screenshots before any sweep so regressions are mechanical to
   catch.

## Anti-patterns this spike respects

- Doesn't merge live and stored data in one view (`docs/soul/patterns.md`): Command is
  ephemeral/live, Reports is durable artifacts. Two tabs, two honest data sources.
- Doesn't build before looking at the data: every number here came from reading the actual
  components and scripts, not guessing.
