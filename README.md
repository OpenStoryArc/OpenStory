# Open Story

[![CI](https://github.com/OpenStoryArc/OpenStory/actions/workflows/test.yml/badge.svg)](https://github.com/OpenStoryArc/OpenStory/actions/workflows/test.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Real-time observability for AI coding agents. Open Story watches what your agents do — every tool call, file edit, command, and decision — translates it into [CloudEvents 1.0](https://cloudevents.io/) via NATS JetStream, and serves a live dashboard with narrative visualization. Your data stays local, in open formats, fully portable.

```
┌─────────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Coding Agent    │────▶│  Transcript  │────▶│  Translate   │────▶│    NATS      │
│  (JSONL files)   │     │  Watcher     │     │  (CloudEvent)│     │  JetStream   │
└─────────────────┘     └──────────────┘     └──────────────┘     └──────┬───────┘
                                                                         │
                                                          ┌──────────────┼──────────────┐
                                                          │              │              │
                                                          ▼              ▼              ▼
                                                   ┌──────────┐  ┌──────────┐  ┌──────────┐
                                                   │ persist  │  │ patterns │  │broadcast │
                                                   │ consumer │  │ consumer │  │ consumer │
                                                   └────┬─────┘  └────┬─────┘  └────┬─────┘
                                                        │              │              │
                                                        ▼              ▼              ▼
                                                   ┌──────────┐  ┌──────────┐  ┌──────────┐
                                                   │  SQLite  │  │ Patterns │  │   React  │
                                                   │ + JSONL  │  │ + Turns  │  │Dashboard │
                                                   └──────────┘  └──────────┘  └──────────┘
```

## What you see

Four dashboard views, each a different lens on the same data:

**Live** — real-time event stream as your agent works. Every tool call, file read, command execution, and model response appears as it happens. Session sidebar shows all active sessions with event counts, token usage, depth sparklines, and subagent hierarchy.

**Story** — narrative view of agent work. Each turn is a card showing what Claude did and why: a sentence diagram ("Claude edited TurnCard.tsx, after reading 3 files, while testing 1 check, because 'Can we start with surfacing UUIDs?' → answered"), domain fact badges (files created/modified, commands run, searches performed), and eval-apply phase detail. Subagent delegations expand inline — click an Agent apply to see the subagent's eval-apply cycles nested recursively. The same structure at every depth.

**Explore** — historical browse and search across sessions. Full-text search, event filtering, session comparison.

**Subagent visibility** — when Claude delegates to subagents (Explore, Plan, etc.), the parent-child relationship is structural. NATS subjects encode it (`events.{project}.{session}.agent.{agent_id}`), Story cards show `main` vs `sub` badges, and inline expansion reveals the subagent's complete eval-apply cycle history.

## Philosophy

Open Story is a mirror, not a leash. It observes but never interferes — it never writes back to the agent, never modifies transcripts, never blocks execution. The data is yours: CloudEvents 1.0, JSONL, Markdown. Open formats, portable, unencumbered.

See [docs/soul/](docs/soul/) for the full philosophy, architecture narrative, and patterns we've learned building this system.

## How it works

The file watcher detects JSONL transcript changes, translates them into CloudEvents via `translate_line()`, and publishes to NATS JetStream with hierarchical subjects (`events.{project}.{session}.agent.{agent_id}`). Four independent actor-consumers process events in parallel:

- **persist** — dedup + SQLite + JSONL backup + full-text search index
- **patterns** — eval-apply cycle detection → sentence generation → PatternEvents
- **projections** — session metadata (tokens, labels, branches, agent relationships)
- **broadcast** — CloudEvent → ViewRecord → WireRecord → WebSocket to UI

HTTP hooks provide an additional near-real-time ingestion path for Claude Code events.

Each actor is an independent tokio task with its own state and NATS subscription. No shared locks between actors — if pattern detection is slow, persistence and broadcast continue unblocked. NATS JetStream provides durable delivery, replay on restart, and hierarchical subject filtering. `just up` starts NATS automatically; `nats.conf` at the project root configures JetStream with 8MB max payload for large sessions.

### The eval-apply model

Agent sessions have recursive structure. A **turn** (one human prompt → complete response) contains multiple **eval-apply cycles** — each cycle is the model evaluating what it knows, dispatching tools, and processing results. Subagents spawned via the Agent tool have the same recursive cycle structure, just nested one level deeper.

The Story tab renders this as paragraphs (turns) containing sentences (cycles). Subagent work nests inside parent turns. The same `CycleCard` component renders at every depth — it's the recursive visual unit of agent work.

### For agents: using OpenStory

Agents working on this project (or any project with OpenStory running) should use the API to understand session context. From experience building this system, here's what works best:

**REST API is your primary tool.** Fast, structured, reliable:
```
GET /api/sessions                                  — list all sessions with metadata
GET /api/sessions/{id}/records                     — all events for a session
GET /api/sessions/{id}/patterns?type=turn.sentence — narrative turns with sentence diagrams
GET /api/search?q=...                              — full-text search across events
```

**Patterns API for narrative understanding.** The `turn.sentence` patterns carry the sentence diagram (verb/object/subordinates), domain facts (files touched, commands run), eval-apply phases, and subagent delegations. Use this to understand WHAT happened, not just the raw events.

**Records API for ground truth.** When you need the actual tool output, file contents, or exact sequence of events, fetch the records. The `extractCycles()` function in `ui/src/lib/eval-apply.ts` derives eval-apply cycles from records — same structure at every depth (main agent and subagents).

**Scripts for data science.** `scripts/analyze_eval_apply_shape.py --all` maps the structural shape of every session. `scripts/query_store.py` inspects SQLite directly. Write scripts for questions — don't guess.

**Avoid raw JSONL grep.** The raw transcript files are Claude Code's native format, not CloudEvents. The translate layer adds `agent_payload`, `tool_outcome`, `agent_id`. Always query through the API to get the translated, typed data.

**Avoid direct SQLite JSON queries.** The internal serde structure (`AgentPayload` with `#[serde(tag = "_variant")]`) makes JSON path queries brittle. Use the API.

### Deployed agent observability (OpenClaw)

Open Story can observe autonomous agents running in containers. The `docker-compose.openclaw.yml` defines a split deployment:

```
claude-runner ──transcripts──► listener (publisher) ──NATS──► consumer (API/dashboard)
              ──HTTP hooks──►
```

The listener runs as root (to read Claude's mode-600 transcript files), watches the shared volume, translates events, and publishes to NATS. The consumer runs separately with its own data volume, subscribes from NATS, and serves the dashboard. Start with:

```bash
docker compose -f docker-compose.openclaw.yml up -d
```

See `docker-compose.openclaw.yml` for full setup including API key configuration and volume management.

## Quick Start

Requires:
- [Rust](https://rustup.rs/) (stable, edition 2021)
- [Node.js](https://nodejs.org/) 20+
- [NATS Server](https://nats.io/) — `brew install nats-server` (event bus, hard dependency)
- [just](https://github.com/casey/just) — command runner (recommended)
- [Docker](https://docker.com/) or [Podman](https://podman.io/) — for E2E/container tests only

### With `openstory` command

For a `code .`-style experience, copy the launcher script to your PATH:

```bash
cp scripts/openstory ~/.local/bin/openstory
chmod +x ~/.local/bin/openstory
# Edit OPEN_STORY_ROOT in the script to match your checkout location
```

Then from any project directory:

```bash
openstory .          # Start server + UI, watching the current directory
openstory            # Start with default watch dir (~/.claude/projects/)
openstory stop       # Kill server + UI
openstory test       # Run all tests
```

### With `just` (recommended)

```bash
just up          # Start NATS + server + UI (Ctrl+C to stop)
just test        # Run all tests (Rust + UI)
```

### Manual setup

```bash
# 1. Start NATS JetStream
nats-server -c nats.conf &

# 2. Build and run the server
cd rs
cargo build --release -p open-story-cli
cargo run -p open-story-cli -- serve

# 3. Start the UI dev server (in another terminal)
cd ui
npm install
npm run dev
```

The server starts on `http://localhost:3002` and watches `~/.claude/projects/` for transcript files. The UI dev server runs on `http://localhost:5173` and proxies API requests to the server.

### Watch pi-mono sessions (optional)

Open Story can observe multiple coding agents simultaneously. To add pi-mono alongside Claude Code, set the watch directory:

```bash
# Via environment variable
OPEN_STORY_PI_WATCH_DIR=~/.pi/agent/sessions just up

# Or add to data/config.toml
# pi_watch_dir = "/Users/you/.pi/agent/sessions"
```

Both watchers run simultaneously — sessions from all configured coding agents appear in the same dashboard. Format detection is automatic (per-file, based on the first JSONL line). Each event carries an `agent` field identifying its source.

### With Docker/Podman

Run the full stack (server + UI + NATS) in containers:

```bash
docker compose up        # or: podman compose up
```

This starts NATS on `:4222`/`:8222`, the server on `:3002`, and the UI on `:5173`. The server watches `~/.claude/projects/` (mounted read-only) and accepts hooks.

**Container runtime:** [Podman](https://podman.io/) is recommended on Windows — it's a drop-in Docker replacement that runs on WSL2 without Docker Desktop. Install with `winget install RedHat.Podman`, then `podman machine init --rootful && podman machine start`. Existing Dockerfiles and docker-compose files work as-is.

### Configure Claude Code hooks (recommended)

Hooks give near-real-time event delivery. Add this to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          { "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }
        ]
      }
    ],
    "PostToolUse": [
      {
        "hooks": [
          { "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }
        ]
      }
    ],
    "SubagentStop": [
      {
        "hooks": [
          { "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }
        ]
      }
    ]
  }
}
```

- **Stop** — captures final response after each turn (low frequency, high value)
- **PostToolUse** — fires after every tool call for near-real-time updates
- **SubagentStop** — captures work from subagents (Task tool spawns)
- **timeout: 5** — 5 second timeout; failures are non-blocking, Claude Code continues normally

**WSL / Linux:** Same config at `~/.claude/settings.json`. If the Open Story server runs on Windows and Claude Code runs in WSL, hooks still work — WSL can reach Windows `localhost`. The file watcher won't see WSL transcripts (different filesystem), so hooks are the only event path for cross-OS setups.

### Verify it works

```bash
# Check the server is running
curl http://localhost:3002/api/sessions

# Start a Claude Code session — events should appear in the dashboard
claude
```

## Keyboard Navigation

The dashboard supports full keyboard navigation across panels.

### Live tab

| Key | Sidebar (sessions) | Timeline (events) |
|-----|--------------------|--------------------|
| `↑` / `↓` | Move between sessions | Move between event cards (skips turn dividers) |
| `→` | Jump focus to timeline | — |
| `←` | — | Jump focus to sidebar |
| `Enter` | Select highlighted session | Open selected card in Explore |
| Click | Select session + start keyboard nav | Select card + start keyboard nav |

Only the focused panel shows the selection ring. Your position is remembered when switching between panels.

### Explore tab

| Key | Sidebar (turns/facets) | Event list |
|-----|------------------------|------------|
| `↑` / `↓` | — | Move between event cards |
| `→` | Jump focus to event list | — |
| `←` | — | Jump focus to sidebar |
| Click | — | Select card + expand/collapse |

### Cross-linking

- **Explore ↗** button on each Live card deep-links directly to that event in the Explore view
- **Enter** on a selected Live card does the same thing via keyboard

## CLI Reference

```
open-story serve [OPTIONS]     Start the dashboard server (default)
  --host <HOST>                  Bind address [default: 0.0.0.0]
  --port <PORT>                  Listen port [default: 3002]
  --data-dir <DIR>               Session persistence directory [default: ./data]
  --static-dir <DIR>             Built UI static files directory
  --watch-dir <DIR>              Transcript watch directory [default: ~/.claude/projects/]

open-story watch [OPTIONS]     Watch transcripts, emit CloudEvents to stdout
  --watch-dir <DIR>              Directory to watch [default: ~/.claude/projects/]
  --output <FILE>                Output file (JSONL append)
  --backfill                     Process existing files before watching
  --quiet                        Suppress stdout output

open-story synopsis <SESSION_ID> Show session synopsis (goal, journey, outcome)
  --data-dir <DIR>               Session data directory [default: ./data]
  --format <FMT>                 Output format: text or json [default: text]

open-story pulse [OPTIONS]     Project activity over N days
  --days <N>                     Number of days to look back [default: 7]
  --data-dir <DIR>               Session data directory [default: ./data]
  --format <FMT>                 Output format: text or json [default: text]

open-story context <PROJECT>   Recent sessions for a project
  --data-dir <DIR>               Session data directory [default: ./data]
  --format <FMT>                 Output format: text or json [default: text]

open-story backfill [OPTIONS]  Embed existing events into Qdrant for semantic search
  --data-dir <DIR>               Session data directory [default: ./data]
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sessions` | List all sessions |
| GET | `/api/sessions/{id}/events` | Raw CloudEvents for a session |
| GET | `/api/sessions/{id}/events/{event_id}/content` | Full content for a truncated event |
| GET | `/api/sessions/{id}/view-records` | Typed ViewRecords for a session |
| GET | `/api/sessions/{id}/records` | WireRecords from projections |
| GET | `/api/sessions/{id}/summary` | Session summary analytics |
| GET | `/api/sessions/{id}/activity` | Activity timeline |
| GET | `/api/sessions/{id}/tools` | Tool usage distribution |
| GET | `/api/sessions/{id}/transcript` | Reconstructed conversation |
| GET | `/api/sessions/{id}/conversation` | Structured conversation view |
| GET | `/api/sessions/{id}/file-changes` | File change history |
| GET | `/api/sessions/{id}/patterns` | Detected behavioral patterns |
| GET | `/api/sessions/{id}/plans` | Plans for a session |
| GET | `/api/sessions/{id}/meta` | Session metadata |
| GET | `/api/plans` | List all plans |
| GET | `/api/plans/{id}` | Get a specific plan |
| GET | `/api/tool-schemas` | Tool schema definitions |
| GET | `/api/search?q=` | Semantic search over events |
| GET | `/api/agent/search?q=` | Session-grouped semantic search (agentic) |
| GET | `/api/agent/tools` | Agent tool definitions (MCP-style) |
| GET | `/api/agent/project-context?project=` | Recent sessions for a project |
| GET | `/api/agent/recent-files?project=` | Files modified in recent sessions |
| GET | `/api/sessions/{id}/synopsis` | Session synopsis (goal, journey, outcome) |
| GET | `/api/sessions/{id}/tool-journey` | Sequence of tools used |
| GET | `/api/sessions/{id}/file-impact` | Files read vs written |
| GET | `/api/sessions/{id}/errors` | Session errors with timestamps |
| GET | `/api/insights/pulse?days=` | Project activity over N days |
| GET | `/api/insights/tool-evolution` | Tool usage evolution across sessions |
| GET | `/api/insights/efficiency` | Session efficiency insights |
| GET | `/api/insights/productivity?days=` | Event density by hour of day |
| DELETE | `/api/sessions/{id}` | Delete a session |
| GET | `/api/sessions/{id}/export` | Export session as JSONL |
| GET | `/ws` | WebSocket for live event streaming |
| POST | `/hooks` | Coding agent hook receiver |

## Project Layout

```
open-story/
├── rs/                          Rust workspace (9 crates)
│   ├── core/                    open-story-core (CloudEvent types, translate, reader)
│   ├── bus/                     open-story-bus (NATS JetStream event bus)
│   ├── store/                   open-story-store (persistence, analysis, projection)
│   ├── views/                   open-story-views (BFF: CloudEvent → ViewRecord)
│   ├── patterns/                open-story-patterns (5 streaming detectors)
│   ├── semantic/                open-story-semantic (embedding, search, Qdrant)
│   ├── server/                  open-story-server (HTTP/WS, API, hooks, ingest)
│   ├── src/                     open-story lib (watcher + server orchestration)
│   ├── cli/                     open-story-cli binary (thin CLI wrapper)
│   └── tests/                   Integration tests
├── ui/                          React dashboard
│   ├── src/
│   │   ├── streams/             RxJS observable state management
│   │   ├── components/          React components
│   │   └── hooks/               Custom React hooks
│   └── ...
├── scripts/                     Analysis tools and data exploration
├── docs/                        Stories, backlog, and architecture docs
└── e2e/                         Playwright E2E tests
```

## Development Commands

Run `just` to see all available commands. Key ones:

| Command | Description |
|---------|-------------|
| `just up` | Start NATS + server + UI (Ctrl+C to stop) |
| `just nats` | Start NATS JetStream standalone |
| `just nats-stop` | Stop NATS |
| `just test` | Run all tests (Rust + UI) |
| `just test-rs` | Run Rust tests only |
| `just test-ui` | Run UI tests only |
| `just e2e` | Run Playwright E2E tests |
| `just explore` | Launch Jupyter notebook for data exploration |
| `just events` | Live event viewer (pretty-print event log) |

## Security Notes

- **Authentication** is off by default (suitable for localhost). For non-localhost deployments, set `api_token` in `data/config.toml` to require bearer token auth on all API/WS requests.
- **`/metrics`** endpoint intentionally bypasses auth so Prometheus can scrape without a token.
- **`docker-compose.observe.yml`** sets the Grafana password to `openstory` — this is for local development only. Change it for any shared or exposed deployment.

## Contributing

Start with the [soul documents](docs/soul/) to understand what this project believes. Then see [CONTRIBUTING.md](CONTRIBUTING.md) for setup, workflow, and PR guidelines.

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
