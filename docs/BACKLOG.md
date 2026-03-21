# Backlog

Ideas and future work for Open Story. Each entry describes *what* and *why* in a short paragraph. When work begins, create a branch — the backlog entry is the spec.

---

## Observability

### Cost & Token Tracking
Surface token usage (input, output, cache reads/writes) per session with estimated cost calculations based on model pricing. Token timelines and cache hit ratios give financial visibility into agent work. Token usage analytics scripts exist (`scripts/token_usage.py`); this is about surfacing it in the UI.

### Anomaly Detection & Behavioral Alerts
Rule-based detection for unusual patterns: destructive git commands, high error rates, tool loops, token spikes. Rules are pure functions evaluated during event ingestion, surfacing alerts without interfering with agent execution. Builds on the existing pattern detection pipeline.

### Workspace Impact Summary
Aggregate file and code changes from a session into a high-level impact view: files created/modified/deleted, lines added/removed, git commits made. Derived from Edit/Write/Bash tool calls already captured in events.

### Agent Behavior Patterns
Cross-session analytics revealing longitudinal trends: tool preferences, session duration, token consumption over time, error rates by task type. Answers questions like "I spend 60% of tokens on test-writing" by aggregating over persisted event data.

### Plan Visibility
Make extracted plans first-class objects in the UI: filterable in the Live timeline, searchable in Explore, viewable inline, with plan counts on session cards. Plans are already extracted during ingestion; this brings them into the frontend.

### Live Token Counter
Real-time running token accumulator in the session header that ticks up as events arrive. Shows input tokens, output tokens, and estimated cost as a pure UI component subscribing to WebSocket assistant events.

---

## Search & Navigation

### Session Search & Full-Text Query
Search across all sessions by prompt text, tool calls, file paths, and commands. Server-side substring search with result ranking and highlighted snippets. Semantic search via Qdrant is already wired; this adds structured full-text search.

### Session Replay & Playback
Chronological playback of session events with transport controls (play/pause/speed) and a visual timeline showing event density. Works client-side with persisted event data — lets you experience a session's narrative flow.

### Session Comparison
Side-by-side comparison of two sessions highlighting deltas in duration, token usage, tool distribution, files touched, and error counts. Enables learning from repeated tasks and calibrating agent directives.

### Session Bookmarks & Annotations
Mark important events with bookmarks and attach free-text notes, persisted as user-owned JSONL. Stores separately from event data (observe, never interfere) while enabling users to curate their understanding.

### Click-to-Open Event
Navigate from faceted views (turns, files, tools) and error lists directly to specific events, auto-scrolling and expanding them with a brief highlight. Connects the Explore outline to the event list.

---

## Export & Portability

### Export Formats
Client-side export of sessions to Markdown transcripts, JSON archives, and CSV summaries with session metadata headers. User data should be useful without Open Story.

### Offline & Local-First Mode
Load persisted JSONL files directly into the UI without a server connection for air-gapped review, CI artifact analysis, and portable data sharing. Reuses all existing read-only views by swapping the data source from WebSocket to file parsing.

### CSV Export for APIs
Server-side `?format=csv` query parameter across analytics endpoints (sessions, token usage, daily trends, project pulse, tool journeys, file impact). Enables spreadsheet analysis and data pipeline integration.

---

## UI

### Card-Based Live Event Feed
Redesign event timeline from table rows to visually distinct cards grouped by event type (prompts, tools, results, thinking) with color-coded badges and automatic entrance animations.

### Interactive Explore
Add filterable event timeline, conversation view, and search-within-session to the Explore tab. All client-side over fetched records. The Explore view shell exists; this fills it out.

### Explore Tree View
Render the causal event tree (parent_uuid relationships) as a collapsible, interactive tree within Explore, showing actual session structure rather than a flat list.

### Event Graph Navigation
Faceted navigation for Explore: turn outline + file/tool/agent facets, with intersection queries to answer "what happened in turn 3 to file auth.rs?" The FacetPanel component exists; this wires it to real queries.

### Syntax Highlighting
Integrate Shiki for VS Code-quality syntax highlighting in code blocks across the dashboard (bash, JSON, rust, etc.), with lazy loading and language detection.

### Timeline Rendering Performance
Fix virtualizer layout shifts when rows expand to show detail inline. Expanded rows should push subsequent rows down without overlap.

### Live Pattern Notifications
Toast notifications when patterns are detected (test cycles, error recovery, git workflows), with click-through to highlight relevant events and optional timeline overlay showing pattern temporal span.

### Mermaid Diagrams
Transform structured data (tool journey, token usage, session flow) into visual Mermaid diagrams (flowcharts, pie charts, sequence diagrams), with optional server-side rendering.

### Mobile Access
Watch agents from a phone via Tailscale mesh VPN. Open Story serves on `0.0.0.0` and is accessible from mobile via secure WireGuard tunnel. Mostly documentation and config.

---

## Infrastructure

### Multi-Directory Watcher
Accept multiple `--watch-dir` roots, backfill concurrently, and resolve project_id correctly across all roots with longest-prefix matching. Currently limited to a single watch directory.

### Real-time LLM API
Claude-powered analysis: running session summaries updated incrementally via pattern detections, natural language query endpoint `/api/ask`, and cross-session story arc detection.

### End-to-End Encryption
Phased encryption: make SQLCipher functional, encrypt JSONL files, add vault unlock mechanism, then add NATS TLS and HTTPS/WSS for clients. SQLCipher key config already exists but isn't exercised.

### Kubernetes Deployment
K8s manifests (NATS StatefulSet + consumer Deployment + agent sidecars), integration tests via K3s testcontainers, and a Helm chart. K3s testcontainer spike exists in the codebase.

### OpenClaw Skill Integration
CLI commands (`sessions`, `summary`, `events`, `install-skill`) for conversational session recall via OpenClaw. Includes SessionSummary reducer, digest format for hourly heartbeat, and portable SKILL.md.

### Starter Configuration
Onboarding UX with `open-story init` for first-time users: choose Claude project folder, storage backend, hooks setup, data directory, and UI mode.

---

## Quality

### UI Battle-Hardening
Performance and chaos testing: synthetic event firehose (throughput, latency, memory), render fidelity under load, interactive chaos (click storm, filter switching), DPI/viewport matrix, 8-hour soak tests.

### Test Cycle False Negative
Fix TestCycleDetector substring matching — "0 failed" in passing output shouldn't trigger failure detection. Use context-aware classification or check pass keywords first.

### Maintenance Script
Create `just check` command verifying project health: tests pass, Docker images current, dependencies updated, lint clean, E2E fixtures present, git state clean.

### Testcontainers + NATS Integration
Add NATS integration tests verifying the full event bus path: watcher → NATS → consumer → ingest, with multi-container networking.

### Performance Bottleneck Fixes
Chunked backfill with inter-chunk yields to prevent overwhelming the consumer. Diagnose and fix the 20KB payload cliff. Add LRU session cache for bounded memory.

### Multi-Container Load Test
Docker Compose setup simulating many concurrent agents posting to a single Open Story instance. Measure SQLite contention, NATS throughput, WebSocket broadcast latency, and find the concurrent session ceiling.

### Multi-Listener Test
Prove multiple publishers feed a single consumer via NATS. Verify both sessions appear with correct project_ids despite different watch directories.

### Testcontainer Improvements
Fix container test infrastructure: shared container pattern, silent fixture mtime failures, log capture on failure. Add comprehensive endpoint sweep, WebSocket testing, error path coverage.

---

## Done (not tracked here)

Completed work lives in git history. For reference, major completed features include: pattern detection pipeline (5 detectors), SQLite event store, pub/sub via NATS, live timeline, explore view split, subagent enrichment, stateful BFF projection, enriched event envelopes, view model crate, testcontainers E2E, configurable projects dir, syntax highlighting, and open-source licensing cleanup.
