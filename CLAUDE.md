# Open Story

## Soul

Open Story exists to enable **personal sovereignty** for humans working with AI agents.

When you use a coding agent, it acts on your behalf — reading your files, running commands, making decisions. Open Story gives you full visibility into that process, in real time. You don't have to trust blindly. You can observe, understand, and decide.

This is a mirror, not a leash. The listener watches but never interferes. It never mutates the source, never injects behavior, never stands between the user and their agent. It translates what happens into a form you can see, search, and reason about — and the data is yours, in open formats, portable and unencumbered.

## Principles as Constraints

These principles flow from the soul above. They are the rules that shape every decision in this codebase.

### 1. Observe, never interfere
- The listener is read-only. It watches transcript files and receives hook events. It never writes back, never modifies agent behavior, never blocks execution.
- If a proposed feature would require mutating the source or inserting the listener into the agent's execution path, it does not belong here.

### 2. Behavior-Driven Development (BDD)
- Start from behavior: describe what the system *should do* from the user's perspective, then make it real.
- Write specs as `describe("when X happens") / it("should Y")` — readable descriptions of behavior, not implementation details.
- Use the pure BDD helpers in `ui/tests/bdd.ts`: `scenario(given, when, then)` — data flows explicitly through the pipeline. No shared mutable state, no hidden closures. Given returns context, When transforms it, Then asserts on the output.
- Red → green → refactor, no exceptions. No production code without a failing spec first.
- Tests verify *correctness*, not just *presence*. Assert on actual values, not just that something rendered.
- Test pyramid: unit specs for pure logic, integration specs for boundaries, E2E specs for real user flows.
- Tests should be *visible*. Use Vitest UI (`vitest --ui`) and Playwright UI mode (`playwright test --ui`) during development. Tests are part of the product, not a chore hidden in the terminal.

### Backlog
- **`docs/BACKLOG.md`** is the single source of truth for future work. Each entry describes *what* and *why* in a short paragraph, grouped by theme.
- When work begins on an item, create a branch. The backlog entry is the spec.
- When work is complete, remove the entry from `BACKLOG.md`. Completed work lives in git history.

### Branch strategy
- `master` is protected. All changes go through pull requests with review.
- Create a feature branch for every change, no matter how small.
- PRs require 1 approving review before merge.
- CI status checks will be required once the GitHub Actions pipeline is validated.
- Commits should be atomic — one logical change per commit.
- Run tests before pushing.

### Commit messages
- Write detailed commit messages. Other agents will read these to understand context.
- First line: concise summary of *what changed* (imperative mood, under 72 chars).
- Body: explain *why* and *how*. Structure as **Problem → Solution → Test coverage** when the change is non-trivial.
- List affected files/modules and what changed in each when touching multiple areas.
- Include test counts or coverage notes (e.g., "Added 17 boundary-table tests for project resolution").
- A good commit message lets another agent pick up where you left off without reading every diff.

### 3. Actor systems and message-passing
- The system is a network of independent actors communicating through messages.
- Each actor has a single responsibility and its own lifecycle: the **watcher** observes files, the **translator** converts formats, the **ingester** deduplicates and persists, the **broadcaster** pushes to subscribers.
- Actors don't share mutable state. They send events. In Rust: Tokio tasks + broadcast channels. In the UI: RxJS subjects and observables.
- This is the natural architecture for sovereignty — each actor is auditable, replaceable, and independently testable.

### 4. Functional-first, side effects at the edges
- Core logic is pure functions: data in, data out. `translate.rs`, `lib/`, `streams/` — no side effects.
- Side effects (file I/O, network, DOM) live at the actor boundaries: `watcher.rs`, `connection.ts`, React components.
- Prefer immutable data and transformation pipelines over stateful mutation.
- Composition over inheritance, always.

### 5. Reactive and event-driven
- Events are the fundamental unit of data. Everything the agent does becomes a CloudEvent.
- Data flows one direction: source → translate → ingest → broadcast → render.
- RxJS observables on the frontend, broadcast channels on the backend. The UI reacts to state, it doesn't poll for it.
- Real-time awareness is a sovereignty requirement — after-the-fact logs are not enough.

### 6. Open standards, user-owned data
- CloudEvents spec 1.0 for all events. No proprietary formats.
- JSONL for persistence — human-readable, grep-able, portable.
- Markdown for plans. The user's data should be useful without this tool.

### 7. Minimal, honest code
- No abstractions without justification. Three clear lines beat a clever helper.
- Don't build for hypothetical futures. Solve the problem in front of you.
- If you're adding complexity, articulate what sovereignty benefit it provides.

### 8. Shift prototyping left
- Use lightweight tools to explore concepts and make quick prototypes before committing to implementation.
- Python scripts, notebooks, and small standalone experiments are cheaper than building the wrong thing in Rust.
- Prototype visualizations, data models, and algorithms in `scripts/` first. Validate with real data. Then port to production code.
- The prototype is the spec. If the prototype works, the production implementation has a clear target.

### 9. Scripts over rawdogging
- When doing data analysis, exploration, or one-off tasks, write a proper script file — don't rawdog Python/bash inline via the shell.
- Scripts are artifacts: saved to `scripts/`, committed, reusable, reviewable.
- Scripts should have a `__main__` block, argparse or simple CLI args, and clear output.
- Prefer scripts that can also be imported as modules (functions, not top-level side effects).
- Even exploratory scripts get lightweight tests when the logic is non-trivial (`--test` flag).
- **Use existing scripts first.** Before writing inline queries or one-off analysis, check `scripts/` for an existing script that answers the question. Run it, extend it, or compose from it.

**Why this matters:** Scripts tell a story. When another agent, a collaborator, or a curious reader opens `scripts/`, they should see a trail of inquiry — what questions were asked, how the data was explored, what was discovered. Raw shell one-liners vanish. Scripts endure.

**Agent self-awareness:** Agents working on this project can query the live event store to understand session characteristics, tool usage patterns, and detected behaviors. Use `scripts/query_store.py` to inspect the SQLite database. If a question can be answered by data, write a query — don't guess.

**Maintaining scripts:** When the data model changes (new tables, renamed fields, new event subtypes), update the scripts that query it. Scripts with `--test` flags should be run as part of validation. A broken script is a broken understanding.

Scripts in `scripts/` are runnable standalone (`uv run python scripts/foo.py`) and often have `--test` flags. Use `scripts/query_store.py` to inspect the SQLite database. See `scripts/` for the full inventory.

### Learned anti-patterns

These mistakes have been made and corrected. Don't repeat them:

- **Don't build before looking at data.** We built a tree abstraction, then discovered the data is a linked list. Write a script, check the shape, then build.
- **Don't merge live and stored data into one view.** WebSocket data is ephemeral. REST data is durable. Merging creates a view that's partially both and fully neither. Keep views honest about their data source.
- **Don't add abstractions for problems that don't exist.** We wrote a lazy-loading list for 2000 records that render in milliseconds. Three clear lines beat a clever helper.

Full list with context: `docs/soul/patterns.md`

---

## Project Structure

```
rs/           — Rust workspace (9 crates)
  core/       — open-story-core: CloudEvent types, translate, reader, paths
  bus/        — open-story-bus: NATS JetStream event bus abstraction
  store/      — open-story-store: persistence, analysis, projection
  views/      — open-story-views: CloudEvent → ViewRecord BFF transform
  patterns/   — open-story-patterns: streaming pattern detection (5 detectors)
  semantic/   — open-story-semantic: embedding, vector search, Qdrant integration
  server/     — open-story-server: HTTP/WS server, API, hooks, ingest
  src/        — open-story: orchestration library (watcher + server wiring)
  cli/        — open-story-cli: thin CLI binary
  tests/      — integration tests
ui/           — React dashboard (Vite, TailwindCSS v4, RxJS, Recharts)
e2e/          — Playwright E2E tests
```

The Rust codebase is a **workspace with 9 crates**. Core domain logic lives in `open-story-core`, `open-story-views`, `open-story-patterns`, `open-story-store`, and `open-story-semantic`. Infrastructure lives in `open-story-bus` (NATS) and `open-story-server` (HTTP/WS). The `open-story` lib crate orchestrates watcher + server + bus wiring. The `open-story-cli` binary is a thin wrapper. This separation means `cargo test` never needs to build or touch the binary, avoiding Windows file-lock conflicts when the dev server is running.

## Build & Test

```bash
# Rust — test all crates + integration tests (never touches the binary)
cd rs && cargo test

# Rust — test a specific crate
cd rs && cargo test -p open-story-core
cd rs && cargo test -p open-story-views
cd rs && cargo test -p open-story-server

# Rust — build the CLI binary
cd rs && cargo build -p open-story-cli

# React dashboard
cd ui && npm install && npm run dev    # dev server (port 5173)
cd ui && npm run build                 # production build

# E2E tests (uses Docker container for backend, Vite for frontend)
cd rs && docker build -t open-story:test .   # build test image first
cd e2e && npx playwright test

# Docker — run server + UI in containers
docker compose up
```

## Development Quick Reference

```bash
just up              # Build + start server + UI (Ctrl+C to stop)
just serve           # Start Rust server only
just dev             # Start Vite UI dev server only
just test            # Run all tests (Rust + UI)
just test-rs         # Rust tests only
just test-ui         # UI tests only
just test-ui-dev     # Vitest interactive UI
just e2e             # Run Playwright E2E tests
just e2e-dev         # Playwright interactive UI mode
just docker-build    # Build test container (required before container tests)
just test-container  # Run container integration tests
just test-compose    # Run compose tests (full NATS bus path)
just observe         # Start full stack + Prometheus + Grafana
```

## Architecture

**Pipeline:** `watcher.rs` (notify crate) → `reader.rs` (incremental byte-offset reads) → `translate.rs` (JSON → CloudEvent 1.0) → `server/ingest.rs` (ingest, persist, broadcast)

**Ingest fan-out** — events flow through a single `ingest_events()` orchestration point, then fan out to multiple sinks:
```
watcher/hooks → bus → ingest_events()
                         ├→ SQLite (events, sessions, patterns)
                         ├→ JSONL (append-only backup)
                         ├→ pattern pipeline → SQLite
                         └→ embedding channel → worker → Qdrant
```
All sinks are optional and non-blocking. The system works with any subset active.

**Server crate** (`rs/server/src/`):
- `ingest.rs` — `ingest_events()` pipeline (dedup → persist → project → broadcast)
- `state.rs` — AppState (shared state behind `Arc<RwLock>`)
- `router.rs` — `build_router()` (all HTTP/WS routes)
- `api.rs` — REST endpoints (`/api/sessions`, `/api/search`, `/api/agent/search`, etc.)
- `ws.rs` — WebSocket broadcast for live updates
- `hooks.rs` — `POST /hooks` endpoint for Claude Code HTTP hooks
- `config.rs` — Config struct + TOML loading
- `auth.rs` — Bearer token authentication middleware
- `metrics.rs` — Prometheus metrics endpoint
- `broadcast.rs` — WebSocket broadcast channel management
- `transcript.rs` — Transcript reconstruction
- `tool_schemas.rs` — Tool schema definitions

**Store crate** (`rs/store/src/`):
- `sqlite_store.rs` — SQLite persistence (events, sessions)
- `projection.rs` — SessionProjection (incremental materialized views)
- `persistence.rs` — SessionStore (JSONL append-only backup)
- `plan_store.rs` — Plan extraction and storage
- `analysis.rs` — Session summaries and tool analytics
- `queries.rs` — CLI query functions (synopsis, pulse, context)

**Semantic crate** (`open-story-semantic`):
- `SemanticStore` trait + `NoopSemanticStore` (same pattern as `Bus`/`NoopBus`)
- `QdrantStore` — vector search via Qdrant gRPC (feature-gated `qdrant`)
- `OnnxEmbedder` — local all-MiniLM-L6-v2 embeddings (feature-gated `onnx`)
- `extract.rs` — pure text extraction from ViewRecords
- `worker.rs` — background embedding (tokio task, bounded channel, batched upserts)
- `backfill.rs` — batch-embed existing events from SQLite

**Event type:** All events use `type: "io.arc.event"` with hierarchical `subtype` for classification:
- `message.user.prompt`, `message.user.tool_result`
- `message.assistant.text`, `message.assistant.tool_use`, `message.assistant.thinking`
- `system.turn.complete`, `system.error`, `system.compact`, `system.hook`
- `progress.bash`, `progress.agent`, `progress.hook`
- `file.snapshot`
- `queue.enqueue`, `queue.dequeue`

## Key Conventions

- CloudEvents spec 1.0 for all events
- UUID-based dedup at translate layer + event ID dedup at ingest layer
- Server port: 3002 (default)
- Watch dir: `~/.claude/projects/` (default)
- Data dir: `./data` (JSONL persistence + plans)

## Configuration

Config file: `data/config.toml` (auto-created with `open-story serve --init-config`).

**Load order:** defaults → config.toml → CLI flags → env vars (each layer overrides the previous).

**Key fields (with defaults):**

| Field | Default | Description |
|-------|---------|-------------|
| `host` | `127.0.0.1` | Bind address (localhost only by default) |
| `port` | `3002` | Listen port |
| `api_token` | `""` (no auth) | Bearer token for API authentication |
| `db_key` | `""` (unencrypted) | SQLCipher encryption key for the database |
| `allowed_origins` | `[]` (localhost) | CORS allowed origins |
| `data_dir` | `./data` | Directory for SQLite DB, JSONL, plans |
| `watch_dir` | `~/.claude/projects/` | Transcript watch directory |
| `nats_url` | `nats://localhost:4222` | NATS server URL |
| `max_initial_records` | `2000` | Max records in WebSocket initial_state handshake |
| `boot_window_hours` | `24` | Hours of history to load from JSONL on first boot |
| `truncation_threshold` | `100000` (100KB) | Payload size above which tool outputs are truncated |
| `stale_threshold_secs` | `300` | Seconds of inactivity before session shows as stale |
| `metrics_enabled` | `false` | Enable Prometheus `/metrics` endpoint |
| `retention_days` | `0` (no cleanup) | Auto-delete sessions older than N days on boot |
| `semantic_enabled` | `false` | Enable semantic search (requires Qdrant) |
| `qdrant_url` | `http://localhost:6334` | Qdrant gRPC endpoint URL |
| `embedding_model_path` | `""` | Path to ONNX embedding model directory |

**Env var convention:** `OPEN_STORY_*` (e.g., `OPEN_STORY_PORT=8080`, `OPEN_STORY_API_TOKEN=secret`).

See `rs/server/src/config.rs` for the full Config struct and defaults.

## Hooks Setup

Open Story works best with Claude Code HTTP hooks configured. The hooks provide near-real-time event delivery (vs. the file watcher which polls). See README.md for setup instructions, or add this to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }],
    "PostToolUse": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }],
    "SubagentStop": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }]
  }
}
```

## Development Workflow (TDD)

1. Write a failing test that describes the expected behavior
2. Run the test to confirm it fails (`cargo test` / `npm test`)
3. Implement the minimum code to make it pass
4. Refactor if needed, keeping tests green

### Running tests
- Rust: `cd rs && cargo test`
- UI: `cd ui && npm test`
- E2E: `cd e2e && npx playwright test`
- All: `cd rs && cargo test && cd ../ui && npm test`

### Test locations
- Rust unit tests: inline `#[cfg(test)]` modules
- Rust integration tests: `rs/tests/`
- Rust test helpers: `rs/tests/helpers/mod.rs`
- UI pure function tests: `ui/tests/lib/`, `ui/tests/streams/`
- E2E tests: `e2e/tests/`

## Testing Conventions

- Integration tests live in `rs/tests/` with shared helpers in `rs/tests/helpers/mod.rs`
- `test_state()` creates an isolated AppState backed by a temp directory
- `send_request()` + `body_json()` for HTTP endpoint testing
- Test fixtures in `rs/tests/fixtures/`

### Testcontainers (Rust integration tests)

The `testcontainers` crate runs open-story inside a Docker container during integration tests. Helpers in `rs/tests/helpers/container.rs` provide `start_open_story(fixture_dir)` which mounts fixtures at `/data`, exposes a random port, and waits for health check.

```bash
cd rs && docker build -t open-story:test .   # required before running container tests
cd rs && cargo test -p open-story --test test_container
```

### E2E tests (Playwright + Docker)

E2E tests use a Docker container for the backend (not `cargo run`). This avoids Windows file-lock conflicts and provides deterministic seed data.

- **Backend**: `docker run open-story:test` on port 3099 (configurable via `API_PORT` env)
- **Frontend**: Vite dev server on port 5188 (configurable via `UI_PORT` env), proxies API/WS to the container
- **Seed data**: `e2e/fixtures/seed-data/` mounted at `/data` inside the container
- **Config**: `e2e/playwright.config.ts` manages both webServers
- **MSYS path fix**: `MSYS_NO_PATHCONV=1` prevents Git Bash from mangling Docker volume paths on Windows
- **Shared helpers**: `e2e/tests/helpers.ts` — `expandAllProjects()` (project groups collapse after 24h), `selectHookSession()`

```bash
cd rs && docker build -t open-story:test .   # rebuild after Rust changes
cd e2e && npx playwright test                  # run all E2E tests
cd e2e && npx playwright test --ui             # interactive UI mode
```

## Use Cases

`docs/soul/use-cases.md` contains concrete code examples of each principle. Review it before starting work on a new feature or architectural change. If your change invalidates a use case reference, update it in the same commit.

## Further Reading

For deep work — new features, architectural changes, or understanding *why* the system works the way it does — read the soul documents:

- `docs/soul/philosophy.md` — why things are the way they are
- `docs/soul/architecture.md` — system design narrative
- `docs/soul/patterns.md` — what works, what doesn't, and how we build
- `docs/soul/use-cases.md` — each principle demonstrated in real code
- `docs/soul/sicp-lessons.md` — theoretical foundations (streams, actors, abstraction barriers)
- `docs/architecture-tour.md` — 14-stop guided code walkthrough

