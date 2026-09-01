# Reports tab — wireframes

A new top-level tab: **Reports** — a gallery and viewer for the HTML reports that agents
already generate. The insight that makes this cheap: **OpenStory's report-generation pipeline
already exists** — it just has no front door.

Today, ~9 scripts in `scripts/` produce self-contained HTML reports (`skill_live_reports.py`
→ 6 skill reports, `cost_report.py`, `session_report.py`, `archetype_charts.py`,
`story_html.py`, `build_prompt_library.py`, plus the `profile_view`/`archetype_view` dev
servers). They land in `/tmp/` or `scripts/*.html` and you open them by hand. There is **no
`/api/reports` endpoint, no manifest, no index** — the Reports tab is the missing surface that
turns scattered artifacts into a browsable library.

---

## 1. Two-pane gallery + viewer

Left: a filterable list of reports. Right: the selected report rendered inline. The same
master/detail shape the Explore and Live tabs already use, so it's familiar.

```
┌─ Reports ─────────────────────────────────────────────────────────  [⌘K] [↻ Generate] ─┐
│ ┌─ LIBRARY ──────────────────┐ ┌─ VIEWER ──────────────────────────────────────────┐  │
│ │ [ search reports… ]        │ │  /openstory:cost — live over REST                  │  │
│ │                            │ │  What have my agent sessions cost?      [↗][⟳][⤓] │  │  ← Button row:
│ │ TYPE  ▾   PROJECT ▾  7d ▾  │ │ ──────────────────────────────────────────────────│  │   open / refresh /
│ │ ───────────────────────    │ │                                                    │  │   download
│ │ ● Cost            live  ◀──┼─┤   ┌── KPI strip ──────────────────────────────┐   │  │
│ │   2m ago · georgetown      │ │   │  $4.12 spent   ·  84% cache  ·  2.1M tok   │   │  │  ← iframe /
│ │ ─ Recap           live     │ │   └────────────────────────────────────────────┘   │  │   sandboxed
│ │   2m ago · georgetown      │ │   ▁▃▅█▆▃▅█  tokens / day                           │  │   report HTML
│ │ ─ Archetype       static   │ │   ──────────────────────────────────────────────  │  │
│ │   1h ago · deliberate↔spon │ │   Project          in↑      out↓     est. $        │  │
│ │ ─ Developer profile static │ │   OpenStory        1.2M     180k     $2.40         │  │
│ │   1h ago · 5 dimensions    │ │   web-dashboard    520k      71k     $1.10         │  │
│ │ ─ Session story   session  │ │   infra-deploy     210k      28k     $0.62         │  │
│ │   3h ago · OpenStory       │ │                                                    │  │
│ │ ─ Prompt library  static   │ │                                                    │  │
│ │   1d ago · 20 templates    │ │                                                    │  │
│ └────────────────────────────┘ └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────────────┘
   Left rail: each row = Card. Badges carry TYPE (live / static / session) + theme/claim.
   Filters = DropdownMenu (Type, Project) + the shared TimeFilter. Search = Input.
```

**shadcn substrate:** `Card` per report row, `Badge` for type/theme, `DropdownMenu` for the
Type/Project facets, `Input` for search, `Button` for the action row, `ScrollArea` for both
panes, `Skeleton` while a live report regenerates.

---

## 2. The honest rendering boundary: sandboxed iframe

Reports are **self-contained HTML documents with their own CSS** (Georgetown serif, Chart.js,
Vega-Lite SVG) — a different visual language from the Tokyonight app shell, on purpose. Don't
try to re-style them into the app; **render each report in a sandboxed `<iframe>`** inside the
viewer pane. This keeps the boundary honest:

- the report owns its look (it's a portable artifact — sovereignty: it's useful *without* this
  tool),
- the app shell owns the chrome around it (tab bar, action row, filters),
- `sandbox` keeps a report's scripts (Chart.js, Vega) from touching the host app.

This mirrors the project's own anti-pattern lesson: *don't merge live and stored data into one
view.* The chrome is live app state; the report is a durable artifact. The iframe is the seam.

---

## 3. Live vs static is a first-class distinction in the UI

The pipeline has two kinds of report and the gallery must not blur them:

| Kind | Examples | Badge | Behavior |
|------|----------|-------|----------|
| **live** | cost, recap, standup, coach, scan, recall | `live` (primary) | `⟳ Refresh` re-runs against the REST API; shows "generated 2m ago", can go stale |
| **static** | archetype, developer profile, prompt library | `static` (muted) | computed snapshot; refresh = recompute |
| **session** | session story, single-session report | `session` + project chip | scoped to one session; opened from a session's deep-link too |

A `live` report carries an age ("2m ago") and a stale indicator once it ages out — the same
`stale_threshold` honesty the rest of the app uses. Clicking `⟳` on a live report re-invokes
the generator and swaps the iframe `src`; a `Skeleton` covers the gap.

---

## 4. What the backend needs (the only real new work)

The UI is mostly plumbing; the genuinely new piece is a **discovery + serving seam** the app
can read. Minimal shape, reusing the existing REST conventions (`/api/...`):

```
GET  /api/reports            → manifest: [{ id, title, type, kind, project,
                                            theme, generated_at, path, live }]
GET  /api/reports/{id}       → the report HTML (for the iframe src)
POST /api/reports/{id}/run   → (live reports) regenerate, return fresh HTML + new generated_at
```

Two supporting changes in the generators (small, mechanical):

1. **Emit `<meta name="report:*">` tags** (`title`, `type`, `kind`, `project`, `theme`,
   `generated_at`) into each generated HTML so the manifest can be built by scanning the
   output dir — no separate database needed. The metadata the second-pass inventory found is
   all already computable at generation time; it's just not written down yet.
2. **Stamp a `generated_at`** — currently no script records when it ran. Add it.

A manifest-by-scan keeps this aligned with the project's "JSONL/files on disk, grep-able,
user-owned" posture: the reports *are* the source of truth, the endpoint just indexes them.
No new store, no migration. (Whether the generators run on-demand from the server or stay
CLI-invoked is an open question — see §6.)

---

## 5. ⌘K integration

The shared `Command` palette gains report verbs, so Reports is reachable without the mouse
and cross-links to the rest of the app:

```
   ⌘K  > cost
   ───────────────────────────────────────────
   REPORTS
   ↳ Open  Cost  (live · 2m ago)
   ⟳ Regenerate  Cost
   ↳ Open  Developer profile  (static)
   ───────────────────────────────────────────
   GENERATE
   + New report…   →  cost · recap · standup · coach · scan · archetype · session story
```

`+ New report…` is how you trigger a generator you haven't run yet — it lists the known
report types (derived from the generator inventory) and kicks off a `POST …/run`.

---

## 6. Open questions (taste / scope calls)

1. **Who runs the generators?** Option A: the server shells out to the Python scripts on
   `POST /run` (simplest, but couples the Rust server to Python tooling). Option B: a
   lightweight sidecar / the existing `profile_view`-style stdlib servers register themselves.
   Option C: generators stay CLI-invoked and the tab is read-only over whatever's on disk
   (most honest to "observe, never interfere", least magical). Leaning C for v1 — the tab
   *surfaces* artifacts; a human or an agent still *produces* them. Worth a decision.
2. **Where do reports live?** Today `/tmp` (ephemeral) + `scripts/*.html` (committed-ish). A
   Reports tab probably wants a stable `data/reports/` dir under the data dir, alongside the
   JSONL — so the library survives a reboot and stays in the user-owned data envelope.
3. **Per-session reports in two places?** A session's report is reachable both here and from
   that session's deep-link in Live/Explore. Good (convergent navigation) or confusing
   (which is canonical)? Probably fine — same artifact, two doors.

> Static mock — gallery + viewer + tab bar + ⌘K, self-contained, Tokyonight:
> [`mocks/reports-tab.html`](mocks/reports-tab.html). Open directly in a browser.
