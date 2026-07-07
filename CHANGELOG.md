# Changelog

All notable changes to OpenStory are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [0.4.0] — 2026-07-07

The "the UI emerges from the data" release. A navigation canopy over the whole
store, a full-duplex agent-in-UI seam, and the scale work to make it hold up on
real sessions. Spans the (untagged) 0.3.0 development line and PR #96.

### Added
- **Agent-in-UI MCP seam** — an agent can now *drive and follow* the dashboard,
  not just query history. Three new tools bring the surface to **24**:
  `ui_control` (navigate / present / toggle / query), `where_is_user`
  (point-read the human's current view), and `subscribe_ui_state` (live-follow
  their navigation). Everything the agent authors flows on a dedicated `ui.*`
  JetStream namespace — **never** the observed `events.*` stream, so agent
  activity is never mutated. Includes present/annotation overlays and a
  bidirectional replay engine (retrace / rewind a captured journey).
- **`open-story init` wires the MCP for you** — the setup wizard now offers to
  run `claude mcp add --transport stdio openstory -- <path>` when the
  `open-story-mcp` binary and the `claude` CLI are both present. Explicit opt-in;
  prints the exact command either way so nothing is hidden.
- **Navigation canopy** — every count is a place: `event→turn`, `plan→turn`,
  `error→event`, `toolcall⇄result`, `subagent→parent`, `file→session`, and the
  session carries across tabs. Deep-linkable events (`route.eventId` scrolls to
  its turn). Bookmarkable filter query tails on Explore routes.
- **Canvas** — a curated visualization tab (sunburst, annotated scatter, 3D
  heatmap, Gantt, beeswarm, constellation, tool-flow); every datum drills to its
  session. Plus an **Ask** command palette and rich conversation rendering
  (syntax-highlighted code, calm pattern rollups, Live filter pills).

### Changed
- **The app is 7 tabs** — Live · Explore · Story · Canvas · Ask · Users · Admin.
  Overview was absorbed into Explore (`#/overview` is now an Explore alias); Lab
  and Storm were retired (preserved on the `ui-improvements` branch).
- **The MCP reads through the REST API** (`HttpEventStore`, #88) — it never opens
  the database on disk, so it runs from any directory. The vestigial `mongo`
  feature (which gated zero code in the MCP) was removed.
- Mobile: the 7 tabs use drawers, not slivers, at phone width.

### Performance
- Survive **100k-event sessions** — the Explore list virtualizes and caps its
  fetch.
- `/records` pagination pushed into SQL (**0.9 s → 0.09 s** per page); `/summary`
  served from projections (**0.78 s → sub-ms**, now carrying turns/tokens/files);
  Story header renders from `/summary` (**117 MB / 2.0 s → 1.1 kB / 2 ms**);
  windowed `/conversation` with load-older; a shared read-model cache fetches a
  session's records once.

### Fixed
- Federation: host is carried in the NATS subject (#58) and Codex events are
  host-stamped (#86), so multi-machine fleets attribute correctly.
- Plan store reads from a single source of truth (#84).

## [0.2.0] — 2026-06-09

The "effortless install" release. `brew install openstory` now brings up the
whole stack with one command — no manual NATS, no JetStream config.

### Added
- **`open-story serve --manage-nats`** — serve launches and supervises its own
  JetStream `nats-server` when none is reachable (reuses an existing one in dev),
  and cleans it up on SIGTERM/SIGINT. The Homebrew service uses this, so a single
  `brew services run openstory` brings up NATS + API + dashboard.
- **`open-story init`** — interactive, zero-dependency first-run setup wizard:
  days-of-history, watch dir(s), port + data dir; writes `config.toml` and offers
  to start the service. Non-interactive contexts fail fast with guidance.

### Changed
- Homebrew docs/wizard default to **`brew services run openstory`** (background,
  this login session only); `brew services start` is the explicit launch-at-login
  opt-in.
- One process serves the API **and** dashboard on `:3002` (no separate UI server
  outside dev).

### Removed
- Retired the dead HTTP `/hooks` ingestion residue (stale docs, the WSL setup
  hook config, dead tests). NATS JetStream + the file watcher are the sole
  real-time path. Observed `system.hook`/`progress.hook` events are unaffected.

### Fixed
- `rs/Dockerfile` now copies `mcp/benches/`, so `docker build` (and the e2e image)
  succeed again.
- A `check_docs.py` guard keeps the retired `/hooks` endpoint out of the docs.

## [0.1.0]

Initial Homebrew distribution: source-build formula + bottle workflow.
