# Backlog

Ideas and future work for Open Story. Each entry describes *what* and *why* in a short paragraph. When work begins, create a branch — the backlog entry is the spec.

---

## Observability

### Cost & Token Tracking
Surface token usage (input, output, cache reads/writes) per session with estimated cost calculations based on model pricing. Token timelines and cache hit ratios give financial visibility into agent work. Token usage analytics scripts exist (`scripts/token_usage.py`); this is about surfacing it in the UI.

### Anomaly Detection & Behavioral Alerts
Rule-based detection for unusual patterns: destructive git commands, high error rates, tool loops, token spikes. Rules are pure functions evaluated during event ingestion, surfacing alerts without interfering with agent execution. Builds on the existing pattern detection pipeline.

### Stream Architecture: Live Events + Live Story + Explore Rewrite
The next branch after `chore/cut-legacy-detectors`. Crystallizes the
architecture the cleanup branch was building toward: **two pure streams,
each with a single source of truth, plus a queryable history view.**

The shape:
```
  Source (file watcher → translator)
      │
      ▼
   CloudEvents (the pure observation)
      │
      ├─→ persist consumer       → events collection (queryable via REST)
      │
      ├─→ patterns consumer      → PatternEvents (derived narrative)
      │       │                       │
      │       ├─→ persist patterns → patterns collection (queryable via REST)
      │       └─→ NATS patterns.{project}.{session}
      │                              │
      │                              ▼
      │                         Live Story consumer
      │                              │
      │                              ▼
      │                         WebSocket subject "live.story"
      │
      └─→ NATS events.{project}.{session}
              │
              ▼
         Live Events consumer
              │
              ▼
         WebSocket subject "live.events"
```

Three views, each backed by exactly one source, no preload soup:

| View       | Source                                                | Behavior                                              |
|------------|-------------------------------------------------------|-------------------------------------------------------|
| Live       | WebSocket `live.events` stream                        | Empty on connect, fills as new CloudEvents arrive.    |
| Live Story | WebSocket `live.story` stream                         | Empty on connect, fills as new patterns are detected. |
| Explore    | REST `/api/events` + `/api/patterns` against the      | Whatever shape we want. Rebuilt from scratch.         |
|            | queryable collections                                 |                                                       |

**What this branch needs to do:**

1. **Decompose the broadcast consumer (Actor 4)** into two stream-forwarders.
   - `LiveEventsConsumer`: subscribe to NATS `events.>`, forward CloudEvents to WebSocket clients on the `live.events` channel. Pure forwarder, no shared state.
   - `LiveStoryConsumer`: subscribe to NATS `patterns.>` (already published by Actor 2 after `chore/cut-legacy-detectors`), forward PatternEvents on the `live.story` channel. Pure forwarder.
2. **Redesign the WebSocket protocol.** Today's `initial_state` / `enriched` shape goes away. New shape: `kind: "event"` and `kind: "pattern"` — minimal, pure, streaming. No preload payload by default; if a client wants recent history it asks via REST.
3. **Delete `ingest_events`.** After Actor 4 is decomposed, the function has no production callers. The 14 integration tests that use it as a convenient driver need a thin replacement helper or to be rewritten against the actor pipeline directly.
4. **Rewrite the UI.**
   - **Live tab**: subscribes to `live.events`, renders events as they arrive. Empty on first connect — it's a window into "now," not a buffer. If you reload mid-session you start from now.
   - **Story tab**: subscribes to `live.story`, renders patterns as they arrive. Same shape — live window, no preload.
   - **Explore tab**: deleted and rebuilt from scratch on REST endpoints. Old explore code doesn't migrate; it needs a redesign.
5. **Delete `state.store.detected_patterns`** after the broadcast rewire completes. Once the broadcast consumers forward live, the in-memory cache that `build_initial_state` reads is no longer needed (initial_state itself goes away).

**Why this is a separate branch:** the cleanup branch removed the *cause* of the duplication and the legacy parallel pipelines. Steps 1–5 above are a UX redesign that changes how the dashboard works and needs its own focused branch with screenshots, acceptance criteria, and probably a feature flag for the cutover.

**Architectural principles this enforces:**
- Functional-first, side effects at the edges (CLAUDE.md principle 4): `ingest_events`'s monolithic god-function is gone.
- Actor systems and message-passing (principle 3): every consumer subscribes to NATS, no shared mutable state.
- Reactive and event-driven (principle 5): two streams, one direction each, no buffering at the source.
- Minimal, honest code (principle 7): the dashboard says "this is what's happening NOW" instead of pretending to be a queryable history that you reload to refresh.

**Validation criterion:** the Live tab on a fresh page reload starts with **zero records**, and fills only with events that arrive after connect. The Story tab does the same with patterns. The Explore tab serves whatever the new design wants from the REST endpoints. No view should depend on `initial_state` or `BroadcastMessage::Enriched` after this branch lands.

### Synthetic Event ID Stability (file_snapshot drift)
After `chore/cut-legacy-detectors` made Actor 2 the sole pattern detector,
the sentence-pattern duplication ratio dropped from 1.76× to 1.05×. The
remaining 5% comes from a separate, smaller bug: the translator/watcher
generates fresh UUIDs for *synthetic* events (`file_snapshot`,
`system_event`) on each backfill pass instead of deriving them from the
underlying source state. So the conversation events for a given turn are
stable across reprocessing (user/assistant/tool_call/tool_result/reasoning
all match) but the synthetic events around them have different IDs each
time. The patterns consumer assembles "structurally identical" sentences
that nonetheless reference different event_id sets, and the persisted rows
land under different Mongo `_id`s.

Empirical evidence: in the validation run for the cleanup branch, the
single remaining duplicate group had **212 stable event_ids and 25
unstable** — and the 25 unstable were `file_snapshot=16,
<not in records>=8, system_event=1`. Zero user/assistant/tool drift.

Two possible fixes:
1. **Content-derived IDs for synthetic events** — `file_snapshot` ID =
   `hash(path + sha256(content))`, `system_event` ID = `hash(subtype +
   timestamp)`. Same source state → same ID across passes. The persisted
   rows naturally collapse via the EventStore PRIMARY KEY.
2. **Exclude synthetic events from sentence identity** — when computing
   sentence pattern `_id`, hash only the conversation backbone
   (user/assistant/tool/reasoning), ignoring synthetic noise around the
   turn boundaries. Smaller change but doesn't fix the underlying ID
   instability for other consumers.

I'd lean (1) — it's the right fix and improves the property for *every*
downstream consumer, not just the sentence detector. The
`<not in records>` orphans are a related symptom: they're event_ids
referenced by old patterns whose underlying records were "garbage
collected" (probably by the same restamping issue, where the new pass
generates a new ID and the old one becomes unreferenced).

Estimate: ~30 lines in `rs/core/src/translate.rs` (and the equivalent
for pi-mono in `translate_pi.rs`). Plus a BDD spec asserting that
re-translating the same JSONL line yields the same event ID for synthetic
event types.

### Subagent Task Labels — Restore After Cut
The previous `agent_labels` feature mapped subagent identities to their parent's Task-tool prompt so the dashboard could show "Find the eval/apply lineage doc" in the sidebar instead of "agent-a47118017b71c6821". It was cut in `chore/cut-legacy-detectors` because the legacy implementation was broken end-to-end on real data: (a) the detector checked `tool_name == "Agent"` but the Claude Code tool is named `"Task"` (rs/patterns/src/eval_apply.rs and rs/patterns/src/agent_delegation.rs both had this stale string), so it fired ~5 times in 9 sessions of real data instead of for every subagent invocation; (b) even when it fired, ingest.rs keyed the label by the parent Task-call's event_id while the UI looked it up by the subagent's session_id, so the UI never found it. With both bugs the feature was a no-op. Today the dashboard falls back to the standard `sessionLabels` path (the subagent's own first user_message), which is functional but verbose. To restore the cleaner labels: (1) detect Task tool calls in the new pipeline (StructuralTurn.applies, where `tool_name == "Task"` is the right check), capturing the prompt; (2) key the label by the *subagent's* session_id, not the parent event_id, so the UI lookup actually resolves. Both fixes are small but each must be present for the feature to work — fixing only one is worse than cutting it. Estimate: ~50 lines including a BDD spec for the keying invariant.

### Domain Events & Workspace Impact — SHIPPED
`ToolOutcome` enum implemented in the translate layer: `FileCreated`, `FileModified`, `FileRead`, `CommandExecuted`, `SearchPerformed`, `SubAgentSpawned`. Domain fact badges visible on every Story card. `SubAgentSpawned` carries `agent_id` for parent-child linking. Remaining: `ToolOutcome` for pi-mono (`translate_pi.rs`).

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

### Story Tab — Narrative Session View — PARTIALLY SHIPPED
Five-layer turn cards with sentence diagram, domain fact badges, syntax-highlighted code output, eval-apply phase detail, main/sub agent badges. Recursive CycleCard for inline subagent expansion (fetches records, derives eval-apply cycles client-side). Collapsible sidebar with session selection. Remaining: sidebar replication from Live tab, Rust-side cycle detector (`turn.cycle` pattern), scoped SSE (per-client NATS subscriptions on WebSocket).

### Card-Based Live Event Feed
Redesign event timeline from table rows to visually distinct cards grouped by event type (prompts, tools, results, thinking) with color-coded badges and automatic entrance animations.

### Interactive Explore
Add filterable event timeline, conversation view, and search-within-session to the Explore tab. All client-side over fetched records. The Explore view shell exists; this fills it out.

### Explore Tree View
Render the causal event tree (parent_uuid relationships) as a collapsible, interactive tree within Explore, showing actual session structure rather than a flat list.

### Event Graph Navigation
Faceted navigation for Explore: turn outline + file/tool/agent facets, with intersection queries to answer "what happened in turn 3 to file auth.rs?" The FacetPanel component exists; this wires it to real queries.

### Syntax Highlighting — SHIPPED
Implemented via `react-syntax-highlighter` (Prism + VS Code Dark+) with `detectLanguage()` from file path extensions. Available in Story tab tool output expand and RecordDetail in Live tab.

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

### Pi-Mono Assistant Message Rendering
The dashboard renders some pi-mono `assistant_message` events as blank cards. The data is present in the API (verified via `curl`), but the UI's content block extraction doesn't handle all pi-mono response formats correctly. The pi-mono format uses `content: [{text: "..."}]` arrays where Claude Code uses plain strings. The views layer branches on `agent` field but some assistant message structures still fall through. Fix in the views crate (`from_cloud_event.rs`) and/or the UI's `EventCard` component.

### Pi-Mono Skipped Entry Types
The pi-mono translator (`translate_pi.rs`) skips 6 entry types: `thinking_level_change`, `branch_summary`, `label`, `custom`, `custom_message`, `session_info`. Real sessions produce `thinking_level_change` frequently. The others are defined in pi-mono's type system but rarely seen. Add match arms to translate these into `system.*` subtypes. The views layer's existing `system.*` catch-all handles them as SystemEvent records, so no views changes needed.

### Pi-Mono Validation Script
Automated format gap detection script (`scripts/validate_openclaw.py`) that scans session directories, translates all JSONL files, and reports subtype distribution, tool name distribution, lines that produced 0 events (format gaps), and parse errors. Reuses the pattern from `scripts/translate_pi_mono.py`. Run against `~/.pi/agent/sessions/` or `~/.openclaw/agents/` to find format gaps before they become bugs.

### Multi-Agent UI — Agent Filter & Cross-Agent Analytics
The `agent` field on CloudEvents (`"claude-code"`, `"pi-mono"`) enables filtering sessions by agent platform and comparing tool preferences, token usage, and session duration across agents. Add agent filter to the dashboard sidebar and cross-agent analytics endpoints.

### Query clock injection for full determinism
The time-windowed analytics queries (`project_pulse`, `tool_evolution`, `productivity_by_hour`, `token_usage(days, ...)`, `daily_token_usage(days)`) all call `chrono::Utc::now()` internally. This works for backend-parity tests because both backends call `now()` at the same instant during the test, but it makes the queries non-deterministic across test runs and harder to test against fixed data. The right answer is to refactor each query to take an `as_of: DateTime<Utc>` parameter that defaults to `Utc::now()` at the call site, with conformance tests passing a fixed value. Touches all the query method signatures + every API handler + the CLI surface, so it's intentionally deferred from Phase 5 of the MongoDB sink work — see `docs/research/mongo-analytics-parity-plan.md` §10.1 #1.

### Multi-Directory Watcher
Accept multiple `--watch-dir` roots, backfill concurrently, and resolve project_id correctly across all roots with longest-prefix matching. Currently uses `watch_dir` + `pi_watch_dir` as separate config fields. Generalize to `watch_dirs = [...]` array.

### Real-time LLM API
Claude-powered analysis: running session summaries updated incrementally via pattern detections, natural language query endpoint `/api/ask`, and cross-session story arc detection.

### End-to-End Encryption
Phased encryption: make SQLCipher functional, encrypt JSONL files, add vault unlock mechanism, then add NATS TLS and HTTPS/WSS for clients. SQLCipher key config already exists but isn't exercised.

### Kubernetes Deployment
K8s manifests (NATS StatefulSet + consumer Deployment + agent sidecars), integration tests via K3s testcontainers, and a Helm chart. K3s testcontainer spike exists in the codebase.

### OpenClaw Skill Integration
CLI commands (`sessions`, `summary`, `events`, `install-skill`) for conversational session recall via OpenClaw. Includes SessionSummary reducer, digest format for hourly heartbeat, and portable SKILL.md.

### OpenClaw Watchdog via OpenStory
Cron job or systemd timer on the server that queries the OpenStory API to detect when OpenClaw is stuck — consecutive zero-token error responses, or no successful completion in N minutes. When detected, automatically `docker restart openclaw`. This is the dogfood approach: OpenStory's own data powers the health check instead of generic Docker healthchecks that can't distinguish "running but spinning on rate limits" from "working normally." Could be a simple Python script in `scripts/` querying `http://open-story:3002/api/sessions`.

### Starter Configuration
Onboarding UX with `open-story init` for first-time users: choose Claude project folder, storage backend, hooks setup, data directory, and UI mode.

### Eval-Apply Cycle Detector (Rust)
Add `turn.cycle` as a new pattern type alongside `turn.sentence`. Each eval-apply cycle (model evaluates → dispatches tools → gets results) becomes a detectable pattern. Currently cycles are derived client-side via `extractCycles()` in `ui/src/lib/eval-apply.ts`. Moving to Rust enables real-time cycle streaming via the patterns consumer. Key insight from data: main agents and subagents have identical cycle structure — subagents just lack `turn.complete` markers.

### Scoped Server-Sent Events
Per-client NATS subscriptions on WebSocket. Currently all events broadcast to all clients. With hierarchical subjects, the UI could subscribe to `events.{project}.{session}.>` and get only one session's events (main + subagents). Reduces bandwidth, enables multiple tabs watching different sessions.

### Remove Hooks
With NATS as the transport, hooks are redundant with the file watcher. Both read the same JSONL and produce the same CloudEvents. The dedup logic exists solely because they race. Removing hooks eliminates dedup, the HTTP endpoint, transcript path resolution, and the `seen_event_ids` HashSet.

### Update Architecture Tour
`docs/architecture-tour.md` is stale — the Big Picture diagram shows the old monolithic path (`ingest_events()`) without NATS or actor-consumers. The 14-stop tour needs updating to reflect hierarchical subjects, independent actors, the boot path change, and the eval-apply recursive model. The tour is the onboarding doc for new contributors and agents.

### Decompose Broadcast Consumer
The broadcast consumer is the last one still using `ingest_events()` with shared `AppState`. It needs projection state for `BroadcastMessage` assembly. Decomposing it requires the projections consumer to publish session metadata to `changes.{project}.{session}`, which the broadcast consumer then consumes.

---

## Quality

### Eval-Apply Data Quality Hardening (recurring)
Regular exercise: run `scripts/analyze_turn_shapes.py --all` against live sessions to map the problem space, update probability-class test fixtures (`rs/tests/fixtures/turn_probability_classes.json`), and add assertions for any new edge cases discovered. The distribution of real event sequences is the ground truth — the detector must handle what agents actually produce, not what we imagine they produce. Key metrics to track: turns/sentences ratio (should be 1.0), is_error capture rate (should match raw data), turn number continuity (no gaps), env_delta accuracy. Current known gaps: 7 session mismatches between turns and sentences, subagent sessions produce flushed turns that may lack enough content for meaningful sentences.

### Finish CloudEvent::new Typed EventData Migration
A multi-week half-finished refactor: someone tightened `CloudEvent.data` from `serde_json::Value` to typed `EventData`, plus changed several store constructor signatures (`SessionStore::new`, `EventLog::new` from `PathBuf` → `&Path` returning `Result`; `PersistConsumer::new` from 0-arg → 2-arg). The production code was updated. Most test fixtures and a few production call sites were *not*. CI has been red on every commit since at least `74cffd60` because of it.

**Already fixed in commit X (todo: fill commit hash post-merge):**
- `rs/views/src/from_cloud_event.rs` — `make_cloud_event` and `make_legacy_event` test helpers now return typed `CloudEvent`. New `make_event_data` helper wraps logical fixture fields into `AgentPayload::ClaudeCode` shape so the typed payload accessors find what they expect. Replaced 2 obsolete malformed-input tests with new tests at the deserialization boundary. **Plus a real production bug fix:** the single-tool typed path in `from_cloud_event` was hardcoding `call_id: String::new()` instead of extracting it from the raw content block — empty call_id breaks the join between tool_use and tool_result records, so this was a data-fidelity bug, not just a test issue.
- `rs/store/src/ingest.rs` — new `to_cloud_event` helper that wraps test fixtures into the typed AgentPayload shape; 2 call sites updated.
- `rs/store/src/state.rs` — `ingest_event_into_store_state` test fixture rewritten with typed `agent_payload`.
- `rs/store/src/queries.rs` — `insert_tool_event` and `insert_error_event` SQL helpers wrap fields in `agent_payload` so the production `json_extract($.data.agent_payload.tool)` queries match.
- `rs/bus/src/lib.rs` and `rs/bus/tests/nats_integration.rs` — 2 `CloudEvent::new` call sites switched from raw `Value` to `EventData::new(...)`.

**Still broken (this entry):**
- `rs/server/src/ingest.rs` — 7 sites at lines 584, 672, 730, 773, 817, 861, 902 calling `CloudEvent::new(... json!({...}) ...)` where the third arg should be `EventData::new(...)`. Mechanical fix.
- `rs/server/src/consumers/persist.rs` — 6 sites around lines 130, 141–149 with the older constructor signatures (`PersistConsumer::new()` without args, `SessionStore::new(PathBuf)` instead of `&Path`, missing `.expect()` on `Result` returns). Deeper test rot — multiple constructors changed and the tests weren't updated.
- `rs/tests/` — 6 integration test files (`test_consumers.rs`, `test_subject_hierarchy.rs`, `test_view_api.rs`, `test_pattern_integration.rs`, `test_ingest.rs`, `test_api.rs`, `helpers/mod.rs`) reference `CloudEvent::new` and may have similar stale call sites; status unknown until the server crate compiles.

**Verification:** the fix is complete when `just test` (which runs `cargo test --workspace --exclude open-story-cli` plus `npm test` and clippy) is fully green. Today the workspace compiles cleanly for `views`, `store`, `bus`. Once `server` and `rs/tests/` are clean, the whole Rust suite should be green for the first time in a week.

**Note for whoever picks this up:** the pattern of every fix is the same — wrap fixture data in `AgentPayload::ClaudeCode` (with `_variant: "claude-code"` and `meta.agent: "claude-code"`), or use `EventData::new(raw, seq, session_id)` when constructing `CloudEvent::new` directly. Look at the `make_event_data` helper in `views/src/from_cloud_event.rs` and the `to_cloud_event` helper in `store/src/ingest.rs` for the canonical wrapping rules. Surfaced by `just test` after `scripts/check_docs.py` revealed how stale the docs were.

### Eval-Apply Scope Open/Close Imbalance
Sessions show a ~4× ratio of `eval_apply.scope_open` to `eval_apply.scope_close` patterns. Example: session `06907d46` had 2754 opens vs 721 closes. Two candidate causes: (1) the detector is missing close events in some compound-procedure shapes, (2) subagent flushes (`SubAgentSpawned` outcomes) close scopes implicitly without emitting `scope_close`. Either way scopes should balance — the imbalance breaks any consumer that tries to use scope nesting to reconstruct call hierarchies. Fix: add detector instrumentation/assertions that every `scope_open` eventually emits a `scope_close` (or a typed flush event), then audit which paths drop one. See `docs/research/sessions/06907d46-feat-story-tab-data.md` for the original observation.

### Remove Orphaned Semantic Crate
`rs/semantic/` exists on disk with its own `Cargo.toml` (`open-story-semantic`, with feature flags for Qdrant + ONNX), but it's **not** a workspace member in `rs/Cargo.toml` and no other crate depends on it. It's vestigial Qdrant-based semantic search code from before SQLite FTS5 replaced it. The replacement is real and working: `rs/store/src/sqlite_store.rs` has an `events_fts` virtual table (line 146), an `index_fts()` function, and a `search_fts()` function that powers `GET /api/search`. The `/api/search` endpoint already routes through FTS5, not Qdrant. Action: `git rm -r rs/semantic/`, drop the `qdrant_url` / `embedding_model_path` / `semantic_enabled` fields from `Config`, remove any documentation references that still mention semantic search via Qdrant. Surfaced by `scripts/check_docs.py` — the validator caught that 4 docs claimed 9 crates while the workspace had 8 because the orphan was on disk but not in the build.

### Turn Vocabulary Collision
Two scripts disagree on what "turn" means: `sessionstory.py` counts `system.turn.complete` events (true model turns, e.g., 63 for session `06907d46`), while `analyze_event_groups.py` counts user-prompt windows (e.g., 155 for the same session). Both are correct for their question but the shared label is confusing — a reader of one script's output and the other's will get incompatible numbers. Resolution: rename `analyze_event_groups.py`'s "Turn N" output to "Window N" or "Prompt N", and add a short note to both scripts' docstrings clarifying the distinction. Optional: add a `--turn-mode={model,prompt}` flag where it makes sense.

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

### CI Testcontainers Spike
Investigate what's needed to run Docker-based testcontainer tests (compose tests, container integration tests) in GitHub Actions CI. Currently skipped because CI runners lack the local `open-story:test` image and Docker setup. Spike should cover: GitHub Actions Docker service containers vs Docker-in-Docker, building the test image in CI (caching strategies for the Rust build), NATS sidecar setup, and whether the compose tests can run within the free-tier minute budget. Goal is a concrete proposal, not implementation.

---

## Done (not tracked here)

Completed work lives in git history. For reference, major completed features include: pattern detection pipeline (5 detectors), SQLite event store, pub/sub via NATS, live timeline, explore view split, subagent enrichment, stateful BFF projection, enriched event envelopes, view model crate, testcontainers E2E, configurable projects dir, syntax highlighting, and open-source licensing cleanup.
