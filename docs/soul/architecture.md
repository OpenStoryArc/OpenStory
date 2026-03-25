# Architecture

## The pipeline

Events flow one direction through a series of independent actors:

```
Source → Translate → Ingest → Persist → Broadcast → Render
```

**Source**: File watchers (notify crate) monitor coding agent transcript directories. Each supported agent has a configurable watch directory (`watch_dir` for Claude Code, `pi_watch_dir` for pi-mono). HTTP hooks provide near-real-time event delivery for agents that support them (currently Claude Code).

**Translate**: The reader auto-detects the transcript format on the first JSONL line and dispatches to the correct per-agent translator. Each translator extracts metadata in the coding agent's native field names — no normalization, no mutation of raw data. Every CloudEvent carries an `agent` field (e.g., `"claude-code"`, `"pi-mono"`) so downstream code can branch on format. UUID-based deduplication prevents double-processing.

**Ingest**: Events are validated, deduplicated again by event ID, and routed to persistence and broadcast.

**Persist**: Dual-write to SQLite (durable, queryable) and in-memory projections (fast, pre-computed views).

**Broadcast**: WebSocket push to all connected clients. Each event is a self-contained message.

**Render**: React dashboard transforms events into visual cards with syntax highlighting, markdown rendering, and pattern detection.

## Two data paths

The system maintains two independent data paths, each serving a different UI:

```
Events arrive
    |
    +---> SQLite (durable) ---> REST API ---> Explore tab
    |
    +---> WS broadcast ------> WebSocket ---> Live tab
```

**SQLite** is the authoritative store. All events, all sessions, queryable with SQL. Boot rebuilds projections from SQLite. REST endpoints read from here (or from projections for pre-computed views).

**WebSocket** is the live pipe. Events are broadcast as they arrive. Clients receive a snapshot on connect, then incremental updates. This is ephemeral — refresh clears it.

These paths are never merged in the UI. The Live tab shows only WebSocket data. The Explore tab shows only REST/SQLite data. This prevents the confusion of "partially loaded" views.

## Projections

Projections are in-memory views rebuilt from SQLite on every boot. They pre-compute:
- Timeline rows (ViewRecords transformed for the UI)
- Filter counts per session
- Session metadata (labels, branches, token counts)
- Tree structure (depth, parent_uuid indexes)

Projections are updated incrementally as new events arrive. They're the source for the WebSocket initial state and the `/records` REST endpoint.

## Inverted indexes (client-side)

The Explore tab builds inverted indexes from fetched WireRecords:
- **Turn index**: events grouped by user_message boundaries
- **File index**: file path to event IDs (from tool_call payloads)
- **Tool index**: tool name to event IDs
- **Agent index**: agent_id to event IDs

These are built in a single O(n) pass. Facet selections intersect: click a turn AND a file to see the intersection. Pure functions, no server round-trips.

## Session hierarchy

Sessions have two kinds:
- **Main sessions** — human-initiated, have UUIDs
- **Agent sessions** — subagent spawns, prefixed with `agent-`

Agent sessions are grouped under their parent by `project_id`. The sidebar shows main sessions as top-level cards with expandable agent dropdowns. This collapses dozens of agent sessions into a few parent entries.

## Pattern detection

Five streaming pattern detectors run on every event:
- **Test cycles**: detect test-run → pass/fail → fix → re-run loops
- **Git workflows**: commit, branch, push sequences
- **Error recovery**: error → investigation → fix patterns
- **Agent delegation**: main agent spawning subagents
- **Turn phases**: time spent in thinking vs. tool use vs. response

Patterns are detected incrementally and stored in both memory and SQLite. They appear as badges on timeline events and in the status bar.

## Content rendering

Events render as cards with full content:
- **Prompts/responses**: Markdown with syntax highlighting via ReactMarkdown
- **Tool calls**: Language-specific rendering (bash commands, file paths with dir/file coloring, regex patterns, code diffs)
- **Tool results**: Read tool output gets `cat -n` line numbers stripped and syntax highlighting applied based on the file extension from the parent tool call
- **Errors**: Red-styled with checkmark/cross indicators

The rendering pipeline:
1. Server may truncate large tool results (configurable threshold, default 100KB)
2. `toTimelineRows()` creates summaries (truncated to 500 chars for compact mode)
3. `CardBody` reads full text from the ViewRecord payload, bypassing summary truncation
4. Compact mode shows the summary. Expanded mode shows full content.

## Keyboard navigation

The dashboard supports spatial keyboard navigation across panels. The pure navigation logic lives in `ui/src/lib/keyboard-nav.ts` — a single function `nextCardIndex()` that computes the next card index given direction and row data, skipping turn dividers. No side effects.

**Live tab**: Arrow keys move within the focused panel (sidebar sessions or timeline events). Left/right arrows switch focus between panels. Enter in the sidebar selects a session; Enter in the timeline opens the selected card in Explore (deep-links to the exact event). Only the focused panel shows its selection ring — both panels remember position but defer to focus for visibility.

**Explore tab**: Same left/right spatial navigation between the turns/facets sidebar and the event list. Up/down moves between events. Click to select and expand/collapse.

**Design decisions:**
- Focus tracking via `onFocus`/`onBlur` state, not global focus management — each panel is self-contained
- `requestAnimationFrame`-gated scrolling prevents layout thrashing from rapid key presses
- No side effects inside React state updaters — compute next index first, then set state and scroll separately
- `data-focus-zone` attributes for cross-panel focus switching via DOM query

## Crate structure

```
rs/
  core/       — CloudEvent types, per-agent translators, reader, paths
  bus/        — NATS JetStream event bus abstraction
  store/      — SQLite persistence, projections, queries, plans
  views/      — CloudEvent to ViewRecord transform (branches on agent type), WireRecord truncation
  patterns/   — Streaming pattern detection (5 detectors)
  server/     — HTTP/WS server, API routes, hooks, ingest
  src/        — Orchestration library (watcher + server wiring)
  cli/        — Thin CLI binary
  tests/      — Integration tests
```

Core domain logic lives in `core`, `views`, `patterns`, and `store`. Infrastructure lives in `bus` (NATS) and `server` (HTTP/WS). The `open-story` lib crate orchestrates everything. The CLI binary is intentionally thin — this means `cargo test` never needs to build or touch the binary, avoiding file-lock conflicts on Windows.
