# Command center tab — wireframes

A new top-level tab: **Command** — a 2×2 live grid for watching up to four agent sessions at
once. NOC / mission-control for AI coding agents. Read-only mirror — observe, never interfere.

This builds directly on the prior exploration in
[`../watch-command-center/`](../watch-command-center/) (which has the full card-anatomy
breakdown and all four grid states — all-active, mixed, empty, focus). **Read that first
for the card design rationale.** This document covers only what's new: housing it as an
*integrated tab* in the app, and rebuilding its chrome on shadcn primitives.

Handles below are anonymized (`@you`, `@teammate`); hosts `a1`/`b2` are illustrative.

---

## 1. In app context — the tab bar grows to six

The header gains two tabs. `Command` carries a live count badge of currently-watched
sessions; the dot pulses only when an event has arrived in the last second.

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ Open Story   [Live] [Explore] [Story] [Users] [●Command 4] [Reports]      ● Connected  │
└──────────────────────────────────────────────────────────────────────────────────────┘
   └ h1        └ Tabs (shadcn) — active = primary bg, inactive = muted-foreground
                                          └ Badge "4" = watched count
```

shadcn mapping for the shell: `Tabs` for the bar, `Badge` for the count, the connection
indicator stays as-is. Nothing about the existing four tabs changes.

---

## 2. The grid — same cards, shadcn chrome

The 2×2 grid and card anatomy are unchanged from the prior exploration (header band /
live-ticker body / footer stat-strip; newest event at the bottom; color *and* glyph encode
status for colorblind-safety). What changes is the implementation substrate:

```
┌─ Command ────────────────────────────────────────────────────  [⌘K] [grid|focus] ─┐
│ ┌────────────────────────────────────┬────────────────────────────────────────┐   │
│ │ ● OpenStory  main·a1·@you    4s  ⤢ │ ● agent-harness dev·a1·@you   9s   ⤢  │   │
│ │ ────────────────────────────────── │ ──────────────────────────────────────  │   │  ← Card
│ │ 14:22:51 Bash  cargo test -p server │ 14:22:38 Read  src/harness/run.py        │   │    (shadcn)
│ │ 14:23:09 Think "broadcast channel…" │ 14:22:44 Edit  src/harness/run.py +6     │   │
│ │ 14:23:12 Bash  cargo test ✓214      │ 14:22:55 Bash  pytest -k smoke           │   │  ← ScrollArea
│ │ 14:23:18 Edit  ws.rs +18 −4         │ 14:23:10 Write fixtures/seed.json     ▌  │   │    ticker body
│ │ ────────────────────────────────── │ ──────────────────────────────────────  │   │
│ │ ⏎ editing ws.rs  1.2k·84k↑12k↓ $.41 │ ⏎ running pytest  642·31k↑5k↓  ~$0.14   │   │  ← CardFooter
│ ├────────────────────────────────────┼────────────────────────────────────────┤   │
│ │ ⚠ infra-deploy main·b2·@teammate41s │ ○ web-dashboard feat·b2·@teammate  6m   │   │
│ │ ────────────────────────────────── │ ──────────────────────────────────────  │   │
│ │ 14:23:01 Bash  terraform apply      │ 14:17:02 Bash  pytest ✓                  │   │
│ │ 14:23:04 ⚠ERR  bucket already       │ 14:17:08 Text  "all smoke tests pass"   │   │
│ │          exists; apply aborted   ▌  │ 14:17:09 Turn  ⏹ turn complete           │   │
│ │ ────────────────────────────────── │     · idle 6m — awaiting input ·         │   │
│ │ ✖ ERRORED · halted 41s · open log → │ ○ IDLE · last turn 6m ago · 642 ev       │   │
│ └────────────────────────────────────┴────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────────────────────┘
   Color channel:  ● primary = active   ○ muted = idle/stale   ⚠ destructive = errored
   Equal weight when healthy; the errored card's left rail + footer banner are the only loud pixels.
```

**shadcn substrate:**
- Each quadrant = `Card` with `CardHeader` (identity) / `ScrollArea` body (the ticker) /
  `CardFooter` (stat strip). The error rail is a `border-l-2 border-destructive`.
- `open log →` and `⤢ focus` are `Button` (variant `ghost`/`link`).
- Status color comes from tokens (`--primary` / `--muted-foreground` / `--destructive`) —
  the *same* tokens the rest of the app now uses post-tokenize. No new palette.
- `Sonner` is **deliberately not** used for agent errors — sovereignty means the board
  *reports* in place; it doesn't nag with a toast. (Toasts are reserved for *app* errors like
  a dropped WebSocket.)

---

## 3. New: binding a session to a quadrant (⌘K)

The prior exploration left empty quadrants as dashed drop-targets advertising `press 3 to
bind here`. As a tab, that action routes through a shadcn **`Command` palette** — the
keyboard-first entry point a senior engineer expects:

```
        ┌─ ⌘K ───────────────────────────────────────────────┐
        │  > terra|                                           │   ← cmdk fuzzy filter
        │ ─────────────────────────────────────────────────  │
        │  BIND TO QUADRANT 3                                 │
        │  ● infra-deploy   main · b2 · @teammate · 3s ago    │   ← live sessions,
        │  ● terraform-mod  main · b2 · @teammate · 1m ago    │      newest-active first
        │ ─────────────────────────────────────────────────  │
        │  ACTIONS                                            │
        │  ⤢ Focus quadrant 1        ⏹ Unbind quadrant 2     │
        │  ↳ Open Reports for OpenStory                       │   ← cross-tab nav
        └────────────────────────────────────────────────────┘
```

One palette does three jobs: bind a live session to a slot, jump focus between quadrants,
and cross-navigate to other tabs (e.g. open the Reports for a watched project). It's the
same `Command` instance the whole app shares — Phase 3 of the migration plan.

Empty quadrants still render as dashed `Card` drop-targets ("`+ attach a session — ⌘K or
press 3`") so capacity stays visible and placement stays stable (cards never reflow by
severity — position is muscle memory, color is severity).

---

## 4. New: focus mode as a Sheet/Dialog, not a route

The prior exploration's focus mode (one card full-screen, three collapse to a left rail) is
the natural home for a shadcn **`Sheet`** (full-height side panel) or `Dialog`:

```
┌─ RAIL ─┬─ FOCUS: OpenStory  main·a1·@you ───────────────── 3s ago   [esc] back ─┐
│ ●Open  │ 14:22:51 · Bash   cargo test -p open-story-server                       │
│ main 3s│ 14:22:58 · Result 214 passed; 0 failed; finished in 6.10s               │  ← ScrollArea,
│ ────── │ 14:23:09 · Think  "the broadcast channel needs a bounded buffer so a    │    wider detail
│ ⚠infra │            slow subscriber can't stall the persist consumer…"           │    columns,
│ HALT41s│ 14:23:18 · Edit   rs/server/src/router.rs   +6 −2                     ▌ │    full thinking
│ ────── │ ─────────────────────────────────────────────────────────────────────  │    text
│ ○web 6m│ ⏎ editing router.rs   1.2k ev · 84k↑12k↓ · ~$0.41 · 38m elapsed        │
│        │ [esc] grid  [1-4] jump  [j/k] scroll  [t] transcript                    │  ← keymap footer
└────────┴─────────────────────────────────────────────────────────────────────────┘
```

The left rail keeps peripheral awareness alive — the `⚠ HALT 41s` chip still pulses while
you're deep in one agent. `1–4` jump focus directly (no round-trip to grid); `t` opens the
full transcript via the existing Explore/Live deep-link. Keyboard-drivable end to end.

---

## 5. Data — reuse the existing stream, add nothing

Per [[feedback_reuse_existing_dataflow]], the Command tab needs **no new transport**. Every
field on a card already flows through the `events.{project}.{session}.>` CloudEvent stream
the Live tab consumes:

- ticker rows ← `message.assistant.tool_use`, `message.assistant.thinking`, `system.error`,
  `system.turn.complete`
- header identity ← session metadata (`project`, `host`, `user`, `branch`)
- footer stats ← the projections consumer's token/event tallies
- status (`active`/`idle`/`stale`/`errored`) ← derive from `last_event` age + error events,
  same logic the Sidebar already uses

The only new client state is "which ≤4 sessions are bound to which quadrants" — a small piece
of URL/local state, bookmarkable like the other tabs. **No backend change required.** This is
a pure presentation feature over data that already arrives.

---

## 6. Open question (inherited, still a taste call)

Empty quadrants as stable dashed drop-targets (capacity always visible, placement is muscle
memory) **vs** auto-collapse to fit only watched sessions (more pixels per agent, but the
layout shifts under you). The prior exploration picked *stable*; this spike agrees — a board
you stare at all day rewards predictable geography over density. But it's worth a human's
taste, so it stays open.

> Static mock for the grid (mixed state) already exists and is worth opening in a browser:
> [`../watch-command-center/command-center.html`](../watch-command-center/command-center.html).
