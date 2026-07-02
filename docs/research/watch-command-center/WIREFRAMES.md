# Watch Command Center — Wireframes

A 2×2 grid of split-screen cards for watching **four live agent sessions at once**.
NOC / mission-control for AI coding agents. Read-only mirror — observe, never interfere.

Audience: a senior software engineer who wants density, scannability, and zero fluff.
The whole screen must answer one question in under a second: **which agent needs me?**

All fields below come from real session data: `project_name`, `host`, `user`, `status`
(`ongoing` | `completed` | `errored` | `stale`), `last_event`, `event_count`,
`total_input_tokens`, `total_output_tokens`, plus the live event stream (timestamped
tool uses, assistant text, thinking, errors, turn-complete).

---

## Card anatomy

```
 ┌──────────────────────────────────────────────────────────────┐
 │ ● OpenStory      main · a1 · @max                  12s ago  ⤢ │  ← HEADER
 │   └status dot   └project·branch·host·user   └last event └focus│
 ├──────────────────────────────────────────────────────────────┤
 │ 14:22:41 · Read    rs/server/src/ws.rs                        │
 │ 14:22:44 · Edit    rs/server/src/ws.rs  +18 −4               │  ← BODY
 │ 14:22:51 · Bash    cargo test -p open-story-server          │     live ticker
 │ 14:23:09 · Think   "the broadcast channel needs…"           │     newest at BOTTOM
 │ 14:23:12 · Bash    cargo test  ✓ 214 passed              ▌  │  ← caret = newest
 ├──────────────────────────────────────────────────────────────┤
 │ ⏎ running cargo test          1.2k ev · 84k↑ 12k↓ · ~$0.41  │  ← FOOTER stat strip
 │   └current activity           └events  └tokens in/out └cost   │
 └──────────────────────────────────────────────────────────────┘
```

Three bands, fixed roles:

- **Header** — identity + liveness. Status dot is the loudest pixel on the card; it is
  also encoded redundantly as color *and* glyph (`●` live, `○` idle, `⚠` error, `✓` done)
  so it survives colorblindness and a glance from across the room. `Xs ago` is the
  heartbeat — a stalled agent betrays itself when this number climbs.
- **Body** — the live event ticker. The most recent N rows in a fixed three-column grid:
  `HH:MM:SS · Tool · one-line detail`. Monospace, tool name in a fixed-width column so the
  eye can vertical-scan the *kind* of work without reading prose.
- **Footer** — the at-rest summary: current activity verb, cumulative event count, token
  in/out, rough cost. This is the "is it expensive / is it productive" line.

**Newest at the bottom**, terminal/`tail -f` convention. A senior engineer reads logs
bottom-anchored; the caret `▌` marks the live edge so the eye knows where "now" is without
hunting. New rows flash once (accent wash, 1.5s) then settle — motion only where something
changed, never ambient.

---

## State 1 — all four quadrants active and streaming

```
┌────────────────────────────────────────┬────────────────────────────────────────┐
│ ● OpenStory   main · a1 · @max  4s   ⤢ │ ● agent-harness  dev · a1 · @max 9s  ⤢ │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ 14:22:51 Bash  cargo test -p server    │ 14:22:38 Read  src/harness/run.py      │
│ 14:23:09 Think "broadcast channel…"    │ 14:22:44 Edit  src/harness/run.py +6   │
│ 14:23:12 Bash  cargo test ✓214 passed  │ 14:22:55 Bash  pytest -k smoke         │
│ 14:23:18 Edit  ws.rs +18 −4            │ 14:23:01 Read  tests/test_run.py       │
│ 14:23:20 Read  router.rs            ▌  │ 14:23:10 Write fixtures/seed.json   ▌  │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ ⏎ editing router.rs  1.2k·84k↑12k↓ $.41│ ⏎ running pytest  642·31k↑5k↓  ~$0.14  │
├────────────────────────────────────────┼────────────────────────────────────────┤
│ ● web-dashboard feat · b2 · @kat 2s ⤢ │ ● infra-deploy  main · b2 · @kat 6s  ⤢ │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ 14:23:02 Edit  App.tsx +22 −9          │ 14:22:48 Bash  terraform plan          │
│ 14:23:08 Bash  npm run build           │ 14:22:59 Read  main.tf                  │
│ 14:23:14 Read  vite.config.ts          │ 14:23:05 Edit  main.tf +4 −1            │
│ 14:23:17 Edit  index.css +3            │ 14:23:11 Bash  terraform apply         │
│ 14:23:19 Bash  npm test ✓ 88 passed ▌  │ 14:23:16 Task  provision vpc        ▌  │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ ⏎ running vitest  980·52k↑9k↓  ~$0.27  │ ⏎ terraform apply 410·19k↑3k↓ ~$0.09  │
└────────────────────────────────────────┴────────────────────────────────────────┘
```

**Rationale.** All four cards are visually identical-weight — when everything is healthy,
nothing should grab the eye. Equal weight *is* the signal: a calm board means a calm fleet.
Tokens and cost live in the footer, small, because in steady state they are reference data,
not alerts. The senior engineer's job here is to *not* be interrupted; the design earns its
keep by being boring when the work is boring.

---

## State 2 — mixed: two active, one idle/stale, one errored (the money shot)

```
┌────────────────────────────────────────┬────────────────────────────────────────┐
│ ● OpenStory   main · a1 · @max  3s   ⤢ │ ⚠ infra-deploy main · b2 · @kat 41s ⤢ │  ← red card
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ 14:23:09 Think "broadcast channel…"    │ 14:22:48 Bash  terraform plan          │
│ 14:23:12 Bash  cargo test ✓214 passed  │ 14:22:59 Edit  main.tf +4 −1           │
│ 14:23:18 Edit  ws.rs +18 −4            │ 14:23:01 Bash  terraform apply         │
│ 14:23:20 Read  router.rs            ▌  │ 14:23:04 ⚠ERR  Error: bucket already   │
│                                        │          exists; apply aborted (1)  ▌  │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ ⏎ editing router.rs  1.2k·84k↑12k↓ $.41│ ✖ ERRORED · halted 41s ago · open log →│  ← red footer
├────────────────────────────────────────┼────────────────────────────────────────┤
│ ● web-dashboard feat · b2 · @kat 5s ⤢ │ ○ agent-harness dev · a1 · @max 6m  ⤢ │  ← dim card
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ 14:23:08 Bash  npm run build           │ 14:17:02 Bash  pytest -k smoke ✓        │
│ 14:23:14 Read  vite.config.ts          │ 14:17:08 Text  "all smoke tests pass"   │
│ 14:23:17 Edit  index.css +3            │ 14:17:09 Turn  ⏹ turn complete          │
│ 14:23:19 Bash  npm test ✓ 88 passed ▌  │                                        │
│                                        │      · idle 6m — awaiting input ·       │  ← centered hint
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ ⏎ running vitest  980·52k↑9k↓  ~$0.27  │ ○ IDLE · last turn 6m ago · 642 ev      │
└────────────────────────────────────────┴────────────────────────────────────────┘
```

**Rationale.** This is the case the whole feature exists for, so the hierarchy is tuned for
triage at a glance:

- **Errored card screams, quietly.** A red left-edge rail + red status glyph `⚠` + a red
  footer banner with one action (`open log →`). The error *row* in the ticker keeps its
  timestamp so you immediately know *when* it broke and what the last good step was. No
  modal, no toast — sovereignty means the tool reports, it doesn't nag.
- **Idle card recedes.** Status `○`, body text dimmed to muted, ticker frozen at the last
  turn, a centered `awaiting input` hint instead of a live caret. It's clearly *parked*, not
  *dead* — distinguishable from errored at 10 feet by color temperature alone (gray vs red).
- **Active cards stay normal weight.** Because two siblings are demanding attention, the
  healthy ones must *not* compete. The contrast does the triage work: your eye goes red →
  gray → (ignore the two blues). One glance answers "infra-deploy, now."

The ordering rule worth stating: cards hold their quadrant position (muscle memory — card
3 is always card 3), they do **not** reflow by severity. Predictable placement beats
auto-sorting for a board you stare at all day. Severity is carried by color, not position.

---

## State 3 — empty slots (only 2 sessions being watched)

```
┌────────────────────────────────────────┬────────────────────────────────────────┐
│ ● OpenStory   main · a1 · @max  3s   ⤢ │ ● web-dashboard feat · b2 · @kat 5s ⤢ │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ 14:23:12 Bash  cargo test ✓214 passed  │ 14:23:14 Read  vite.config.ts          │
│ 14:23:18 Edit  ws.rs +18 −4            │ 14:23:17 Edit  index.css +3            │
│ 14:23:20 Read  router.rs            ▌  │ 14:23:19 Bash  npm test ✓ 88 passed ▌  │
│ ────────────────────────────────────── │ ────────────────────────────────────── │
│ ⏎ editing router.rs  1.2k·84k↑12k↓ $.41│ ⏎ running vitest  980·52k↑9k↓  ~$0.27  │
├────────────────────────────────────────┼────────────────────────────────────────┤
│                                        │                                        │
│              ┌  +  ┐                    │              ┌  +  ┐                    │
│                                        │                                        │
│         attach a session               │         attach a session               │
│                                        │                                        │
│    ↑↓ pick from 5 live sessions        │    ↑↓ pick from 5 live sessions        │
│       or press  3  to bind here        │       or press  4  to bind here        │
│                                        │                                        │
│    · · · · · · · · · · · · · · · ·     │    · · · · · · · · · · · · · · · ·     │  ← dashed slot
└────────────────────────────────────────┴────────────────────────────────────────┘
```

**Rationale.** Empty quadrants are kept as **first-class dashed drop-targets**, not
collapsed away. Two reasons: (1) the 2×2 is a *spatial workspace* — keeping the grid stable
means the slot you bind to card 3 stays card 3 next time, preserving muscle memory; (2) the
empty slot is the natural place to *advertise the action* — it tells you how many other live
sessions exist and exactly which key binds one here (`3`, `4`). The board never silently
shrinks to a 1×2; it always shows its full capacity so you remember you have spare eyes.

---

## State 4 — focus mode (one card full-screen, three collapse to a rail)

```
┌──────┬───────────────────────────────────────────────────────────────────────────┐
│ RAIL │ ● OpenStory          main · a1 · @max          last event 3s ago        ⤢ │
│      │ ───────────────────────────────────────────────────────────────────────── │
│ ●Ope │ 14:22:41 · Read   rs/server/src/ws.rs                                      │
│ main │ 14:22:44 · Edit   rs/server/src/ws.rs   +18 −4                            │
│  3s  │ 14:22:51 · Bash   cargo test -p open-story-server                         │
│ ──── │ 14:22:58 · Result 214 passed; 0 failed; finished in 6.10s                 │
│ ●web │ 14:23:09 · Think  "the broadcast channel needs a bounded buffer so a slow  │
│ feat │            subscriber can't stall the persist consumer…"                   │
│  5s  │ 14:23:12 · Bash   cargo test                       ✓ 214 passed           │
│ ──── │ 14:23:18 · Edit   rs/server/src/router.rs          +6 −2                  │
│ ⚠inf │ 14:23:20 · Read   rs/server/src/router.rs                             ▌  │
│ HALT │ ───────────────────────────────────────────────────────────────────────── │
│  41s │ ⏎ editing router.rs    1.2k ev · 84k↑ 12k↓ tokens · ~$0.41 · 38m elapsed  │
│ ──── │                                                                            │
│ ○age │   [esc] back to grid   [1-4] jump   [j/k] scroll   [t] transcript          │  ← keymap
│ idle │                                                                            │
└──────┴───────────────────────────────────────────────────────────────────────────┘
```

**Rationale.** Focus mode trades breadth for depth: one card expands to show wider detail
columns, full thinking text, and tool results that were one-line-clamped in grid view —
without leaving the watch context. The other three **shrink to a left rail** rather than
disappearing, so peripheral awareness survives: the `⚠ HALT 41s` chip on the rail still
pulses, and a senior engineer can dive deep on one agent while the rail keeps the errored
one in the corner of their eye. `esc` returns to the grid; `1–4` jumps focus directly
between agents without a round-trip to the grid. The persistent keymap footer makes the
whole thing keyboard-drivable — no mouse needed to run the board.

---

## Cross-cutting interaction model

- **Keyboard-first.** `1`–`4` focus a quadrant; `esc` back to grid; `j/k` scroll the focused
  ticker; `t` open the full transcript; `g` cycle grid ↔ focus. The board is operable without
  a mouse, which is what a senior engineer running four agents actually wants.
- **Click-to-focus.** Clicking anywhere on a card (or its `⤢` glyph) enters focus mode for
  that agent. Clicking an error footer jumps straight to the error in the transcript.
- **Color is a status channel, not decoration.** Blue/normal = active, gray = idle/stale,
  red = errored, green check = completed. Redundantly glyph-encoded for accessibility.
- **Motion is information.** New rows flash once and settle; live caret marks the edge;
  idle/errored cards are *still*. Nothing animates ambiently — movement always means a real
  event arrived. This is the read-only mirror staying honest: the screen only moves when the
  agent does.
- **Stable placement over auto-sort.** Cards never reflow by severity. Position is muscle
  memory; severity is carried by color. You learn the board once.
