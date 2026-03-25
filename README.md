# Open Story

[![CI](https://github.com/OpenStoryArc/OpenStory/actions/workflows/test.yml/badge.svg)](https://github.com/OpenStoryArc/OpenStory/actions/workflows/test.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Open Story gives you full visibility into what your AI coding agents are doing — in real time. It watches coding agent sessions (Claude Code, pi-mono, and more), translates transcript events into typed records via a [CloudEvents 1.0](https://cloudevents.io/) pipeline, and serves a live dashboard. Your data stays local, in open formats, fully portable.

Real-time observability dashboard for AI coding agent sessions. Watches JSONL transcript files from multiple agents, auto-detects the format, translates them into typed ViewRecords, and serves a live web dashboard.

```
┌─────────────────┐     ┌──────────────┐     ┌──────────────┐     ┌───────────┐
│  Coding Agent    │────▶│  Transcript  │────▶│  Translate   │────▶│  Server   │
│  (JSONL files)   │     │  Watcher     │     │  (CloudEvent)│     │  (Axum)   │
└─────────────────┘     └──────────────┘     └──────────────┘     └─────┬─────┘
        │                                                               │
        │  POST /hooks                          ┌───────────┐           │
        └──────────────────────────────────────▶│  Hooks     │───────────┘
           (near-real-time)                     │  Handler   │     │
                                                └───────────┘     │
                                                                  │ ingest
                                                                  ▼
                                          ┌──────────┐     ┌───────────┐
                                          │  Qdrant  │◀────│  SQLite   │
                                          │ (vectors)│     │ (events)  │
                                          └──────────┘     └───────────┘
                                                │               │
                                                ▼               ▼
                                          ┌───────────────────────────┐
                                          │      React Dashboard      │
                                          │  Live · Explore · Search  │
                                          └───────────────────────────┘
```

## Philosophy

Open Story is a mirror, not a leash. It observes but never interferes — it never writes back to the agent, never modifies transcripts, never blocks execution. The data is yours: CloudEvents 1.0, JSONL, Markdown. Open formats, portable, unencumbered.

See [docs/soul/](docs/soul/) for the full philosophy, architecture narrative, and patterns we've learned building this system.

## How it works

Two ingestion paths work in parallel:
- **File watcher** — polls `~/.claude/projects/` for new/changed `.jsonl` files (background discovery)
- **HTTP hooks** — coding agents POST to `/hooks` on each event (near-real-time delivery, currently Claude Code)

Both produce `io.arc.event` CloudEvents with typed subtypes. The server transforms these into typed ViewRecords before broadcasting to the UI. Deduplication ensures no duplicates.

## Quick Start

Requires:
- [Rust](https://rustup.rs/) (stable, edition 2021)
- [Node.js](https://nodejs.org/) 20+
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
just up          # Build and start server + UI dev server (Ctrl+C to stop)
just test        # Run all tests (Rust + UI)
```

### Manual setup

```bash
# 1. Build and run the server
cd rs
cargo build --release -p open-story-cli
cargo run -p open-story-cli -- serve

# 2. Start the UI dev server (in another terminal)
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

### Enable semantic search (optional)

Semantic search lets you find sessions by meaning — "how did we approach the auth refactor?" — using a local embedding model. Data never leaves your machine.

**1. Download the ONNX Runtime library** (~16MB) from [GitHub releases](https://github.com/microsoft/onnxruntime/releases) and place the shared library in `data/models/`:

| Platform | Archive | Library file |
|----------|---------|-------------|
| Linux/WSL | `onnxruntime-linux-x64-1.20.1.tgz` | `lib/libonnxruntime.so` |
| macOS (Apple Silicon) | `onnxruntime-osx-arm64-1.20.1.tgz` | `lib/libonnxruntime.dylib` |
| macOS (Intel) | `onnxruntime-osx-x86_64-1.20.1.tgz` | `lib/libonnxruntime.dylib` |
| Windows | `onnxruntime-win-x64-1.20.1.zip` | `lib/onnxruntime.dll` |

```bash
# Example (Linux/WSL):
curl -sL https://github.com/microsoft/onnxruntime/releases/download/v1.20.1/onnxruntime-linux-x64-1.20.1.tgz | tar xz -C /tmp
mkdir -p data/models
cp /tmp/onnxruntime-linux-x64-1.20.1/lib/libonnxruntime.so data/models/
```

**2. Download the model and start services:**

```bash
just download-model            # Download embedding model (~86MB)
just qdrant                    # Start Qdrant vector database
just backfill                  # Embed existing events into Qdrant
```

Add to `data/config.toml`:
```toml
semantic_enabled = true
qdrant_url = "http://localhost:6334"
embedding_model_path = "data/models"
```

Then `just up` — the Search tab in the Explore view and `/api/agent/search` will be active.

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
| `just up` | Build and start server + UI (Ctrl+C to stop) |
| `just test` | Run all tests (Rust + UI) |
| `just test-rs` | Run Rust tests only |
| `just test-ui` | Run UI tests only |
| `just e2e` | Run Playwright E2E tests |
| `just explore` | Launch Jupyter notebook for data exploration |
| `just tree` | Launch tree explorer notebook |
| `just patterns` | Run streaming pattern detection prototype |
| `just events` | Live event viewer (pretty-print event log) |
| `just qdrant` | Start Qdrant vector database |
| `just download-model` | Download ONNX embedding model |
| `just backfill` | Embed existing events into Qdrant |

## Security Notes

- **Authentication** is off by default (suitable for localhost). For non-localhost deployments, set `api_token` in `data/config.toml` to require bearer token auth on all API/WS requests.
- **`/metrics`** endpoint intentionally bypasses auth so Prometheus can scrape without a token.
- **`docker-compose.observe.yml`** sets the Grafana password to `openstory` — this is for local development only. Change it for any shared or exposed deployment.

## Contributing

Start with the [soul documents](docs/soul/) to understand what this project believes. Then see [CONTRIBUTING.md](CONTRIBUTING.md) for setup, workflow, and PR guidelines.

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
