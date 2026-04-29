# Backlog

Ideas and future work for Open Story. Each entry describes *what* and *why* in a short paragraph. When work begins, create a branch — the backlog entry is the spec.

---

## Actor pipeline — follow-ups from Phase 1.4.5 (async boot replay)

### Boot-replay status on `/api/health`
Async replay lets HTTP bind in ~3s, but projections keep populating in the background for 5–60s (depending on SQLite event count). During that window `/api/sessions` returns rows with empty/zero `label`/`event_count`/`tokens` — looks broken to the user. Add a `replay_status: "in_progress" | "complete"` field to `/api/health` (and the WebSocket `initial_state` handshake) so the UI can render a "Reconstructing sessions…" hint instead of silent-empty rows. ~20 LOC.

### Bounded `full_payloads` cache
`state.store.full_payloads` is `Arc<DashMap<(String, String), String>>` — grows unbounded as truncated tool outputs >100KB get cached for the lazy-load endpoint. With a 10GB data dir full of large tool outputs this can balloon memory during replay. Add a configurable LRU (e.g., `full_payload_cache_bytes = 512_000_000`) that evicts oldest entries when the size threshold is crossed. Cache misses fall back to the EventStore `full_payload()` path.

### Live-event-during-replay race bound
Watcher publishes live events while `replay_boot_sessions` is still walking the same session's history. `SessionProjection::seen_ids` dedups correctly, but `event_count` / `timeline_rows` during the overlap window can be temporarily inconsistent with the SQLite `events` table. Self-corrects after replay ends. Add a test that asserts the final state converges even under a concurrent live-event stream, and document the window as expected EC behavior.

### DashMap discipline guardrail
Six `StoreState` fields are now `Arc<DashMap>`. The concurrency model requires: never hold a `RefMut` from `.entry()` across an `.await` or across a second `.get()` / `.entry()` on the same map (shard-lock deadlock). Scope guards tightly. Document this in `CLAUDE.md` principles so new contributors don't have to learn it from a stuck test. Consider a lightweight runtime assertion in debug builds that panics on held guards across await points.

### Retire `ingest_events` fully (test migration)
`ingest_events` is production-dead after Phase 1.5 but still pub-exported and used by ~15 integration test files (70 call sites) that do `state.write() + ingest_events(&mut s, ...)`. The tests work because `&mut AppState` auto-derefs to `&AppState` and the inline `!bus.is_active()` demo-mode guard still persists events when the test harness uses `NoopBus`. Migrate all test call sites to `TestActors::drive_batch` (which runs the full four-actor pipeline), delete the demo-mode guard, delete `ingest_events` + `IngestResult` from `rs/server/src/ingest.rs`, remove the re-exports in `rs/src/server/mod.rs`. Pure housekeeping — no behavior change in production.

### Replay-window load test
Existing tests use small fixtures where replay completes before the first API request. Write a soak test that starts the server with a fat fixture (>10K events), hammers `/api/sessions` concurrently, and asserts (a) API never returns 5xx during replay, (b) session list monotonically fills in, (c) final state matches the golden snapshot. This closes the only real gap in the Phase 0b safety net: nobody's measured what the "API serving during long replay" path actually does.

---

## Observability

### Cost & Token Tracking
Surface token usage (input, output, cache reads/writes) per session with estimated cost calculations based on model pricing. Token timelines and cache hit ratios give financial visibility into agent work. Token usage analytics scripts exist (`scripts/token_usage.py`); this is about surfacing it in the UI.

### Per-Call Model-Aware Cost Estimation
The model string (`claude-opus-4-6`, `claude-haiku-4-5-20251001`, etc.) is already present in the raw event payload at `data.raw.message.model`. Today `token_usage.py` and the MCP `token_usage` tool apply a single flat pricing tier across all sessions — the user has to guess which model they were running. The fix: extract the model string from each assistant message, map it to a pricing tier, and compute cost per-call at the correct rate. This gives actual spend instead of hypothetical spend. Prototype: `scripts/cost_by_model.py`. Production path: update `token_usage.py` to default to per-call model extraction (with `--model` as an override), update the MCP `token_usage` and `daily_token_usage` tools to return model-aware costs, and add a `model` column to the Rust `token_usage` and `daily_token_usage` analytics queries.

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

## Hermes Agent Integration

A coordinated set of items for letting OpenStory observe Hermes Agent sessions and letting Hermes agents query OpenStory for structural views of their own past work. Full design and runnable prototype at [`docs/research/HERMES_INTEGRATION.md`](research/HERMES_INTEGRATION.md) and [`docs/research/hermes-integration/`](research/hermes-integration/). Architectural framing at [`docs/research/LISTENER_AS_ALGEBRA.md`](research/LISTENER_AS_ALGEBRA.md).

The work splits into two parallel tracks (OpenStory side, standalone-package side) with one shared prerequisite. The standalone-package approach intentionally avoids asking the Hermes maintainers to merge anything — Hermes already supports third-party plugins via the `hermes_agent.plugins` entry-point group, so the integration ships independently.

### Hermes message shape verification — PREREQUISITE
Boot a Hermes session in a container, run a 5-turn task that exercises a tool call and a thinking block, finalize, capture `~/.hermes/logs/session_{id}.json`. Resolve every `# VERIFY:` marker in `docs/research/hermes-integration/translate_hermes.py` and `plugin_sketch.py`. Required before either of the next two items can ship. Two providers should be checked, not one: an Anthropic-direct provider and an OpenAI-shaped provider, since Hermes is provider-polymorphic and the assistant message shape may differ. Estimated 30 minutes if Hermes boots cleanly. Detailed protocol in [`hermes-integration/DISTRIBUTION_PLAN.md`](research/hermes-integration/DISTRIBUTION_PLAN.md).

### Hermes translator (`rs/core/src/translate_hermes.rs`)
Port [`docs/research/hermes-integration/translate_hermes.py`](research/hermes-integration/translate_hermes.py) to Rust, parallel to `translate.rs` (Claude Code) and `translate_pi.rs` (pi-mono). The Python sketch is the executable spec — 12 tests in `test_translate.py` cover the structural shape. Add `# VERIFY:` resolution after the prerequisite step. Add Hermes file recognition to the watcher (path pattern `*/openstory-events/*.jsonl` plus a sniff of the first line for `"source": "hermes"`). ~150 lines of Rust + ~30 lines for the watcher routing + parallel test cases. Estimate: 1 day after the prerequisite is done.

### Standalone `hermes-openstory` plugin package
Build the plugin scaffolding in [`docs/research/hermes-integration/plugin_sketch.py`](research/hermes-integration/plugin_sketch.py) and [`recall_tool_sketch.py`](research/hermes-integration/recall_tool_sketch.py) into a real pip-installable package, using the entry-point declaration in [`pyproject.toml.example`](research/hermes-integration/pyproject.toml.example). Hooks `post_llm_call`, `post_tool_call`, `on_session_finalize`, etc. and writes Hermes-native events as JSONL into a watched directory. Registers the `recall` tool that wraps OpenStory's `/api/sessions/{id}/synopsis`, `/patterns`, `/file-impact`, `/errors`, `/tool-journey`, and `/api/search` endpoints — these endpoints are *already shipped* in OpenStory; this work makes them callable from inside a Hermes agent loop. Lives in its own repo, published to PyPI. No upstream PR to hermes-agent required. Estimate: 1 day for the package layout, CI, smoke test, and v0.1.0 publish.

### Hermes session backfill script (`scripts/backfill_hermes_sessions.py`)
One-shot script that reads existing `~/.hermes/logs/session_*.json` files and emits Hermes-native event JSONL into the watched directory. Lets users retroactively ingest sessions that existed before they installed the plugin. Lower priority than the live path (which is the high-leverage integration), but cheap once the translator exists.

### Skill-extraction signal feed (Hermes consuming OpenStory)
Once the translator and plugin are in place, the next high-value integration is feeding OpenStory's structural metrics back into Hermes's autonomous skill creation. Hermes currently uses LLM judgment to decide when a sequence of actions is worth turning into a skill; OpenStory's `StructuralTurn` data (cycle counts, error rates, file impact, user follow-up sentiment) gives that judgment deterministic features. Implementation lives in the `hermes-openstory` package, not in OpenStory itself. Tracked here so the backlog reflects the full integration story.

### Cross-provider behavioral comparison endpoint
A new `GET /api/insights/provider-comparison` endpoint that aggregates structural metrics per provider for the same task: cycles per task, error rates, tool selections, terminal stop reasons. Useful for Hermes's `smart_model_routing.py` decisions and as a research output in its own right. Requires running the same task across providers (Hermes already supports this via `batch_runner.py`); OpenStory's job is the aggregation and the view. Lower priority — listed for completeness as the most novel research output of the integration.

### StructuralTurn training data export
A new `GET /api/sessions/{id}/training-export?format=structural-jsonl` endpoint that emits `StructuralTurn`s (with eval/apply phases separated, domain facts extracted, subagent boundaries explicit, `ToolOutcome` typed) as training data. Lets Hermes's trajectory pipeline (`trajectory_compressor.py`, the tinker-atropos integration) consume the structurally-decomposed view alongside or instead of raw messages. Open research question: does training on structurally-decomposed traces produce better tool-calling models? The two repos are uniquely positioned to answer it. Higher-effort; depends on the translator and plugin being in place first.

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

### HOTFIX: Redact NATS token from startup logs
Verified on the Hetzner production deploy on 2026-04-11: `open-story serve` prints the full NATS URL to stderr at boot, including the shared secret:

```
NATS bus: nats://44a08379a1eae2cecb5e1dcadea358e6bed9dd1eb59e5f89@nats:4222
```

The offending line is in `rs/cli/src/main.rs` at the bus-connect log: `eprintln!("  \x1b[2mNATS bus:\x1b[0m        {nats_url}");`. Any token present in the URL userinfo is written verbatim to `docker logs openstory-open-story-1` and persisted in the journald buffer for as long as the container runs.

**Exposure on this deploy:** limited — logs live inside the `open-story:prod` container on the VPS, reachable only via SSH as `deploy@` or by anyone who can `docker exec`. Not in git, not in CI, not in OpenStory's own session capture (the server's stderr doesn't flow into the event stream). But the token has now been in plaintext in at least one set of container logs since the deploy, so treat it as compromised the moment this hotfix lands.

**Fix (tiny):** extend `NatsBus` (or `rs/cli/src/main.rs` at the log site) with a `redact_userinfo(url: &str) -> String` helper that replaces anything between `://` and `@` with `<redacted>`. Apply it to both the success log and the error log paths. ~10 lines + a unit test. Can land as a standalone PR to master, ahead of the broader "Distributed Deployment Security Hardening" item — they cover the same concern, but this one is a one-shot scope-isolated change the deploy docs already expect.

**Rotation procedure after the fix ships:**
1. SSH to VPS, generate a new token: `NEW=$(openssl rand -hex 24)`
2. `sed -i "s/^NATS_LEAF_TOKEN=.*/NATS_LEAF_TOKEN=$NEW/" .env`
3. `sed -i "s|token: \".*\"|token: \"$NEW\"|" deploy/nats-hub.conf`
4. `docker compose -f docker-compose.prod.yml restart nats open-story`
5. Update the token on every leaf node that was using the old value (local Mac, friends' machines).

Related to the broader "Distributed Deployment Security Hardening" item below, but split out because it's (a) a verified live exposure, (b) a trivial fix, and (c) should land before any further deploys create more contaminated log buffers.

### Rotate NATS token already published in BACKLOG.md
The hotfix entry directly above quotes the live NATS token verbatim, and that commit (`290c91d`) is on `origin/master` in the public OpenStoryArc repo. Practical exposure is low — `:7422` is only reachable over Tailscale, so the token is useless without tailnet access — but rotate anyway, scrub the literal value from the entry above, and stop pasting raw startup logs into docs.

### Build `open-story:test` in CI so docker-required tests run for real
The `test_convergence_invariants`, `test_compose_*`, `test_container`, `test_pi_mono_container`, `test_config_degrade`, and `test_config_full` suites all depend on a locally-built `open-story:test` Docker image. The CI workflow (`.github/workflows/test.yml`) doesn't build that image, so these tests are marked `#[ignore]` and never exercised on PRs — coverage is on the honor system (devs run them locally before pushing). The honest fix is to add a `docker build -t open-story:test rs/` step before `cargo test` in the Rust job and drop the `--skip compose --skip container --skip pi_mono` filter (plus the `#[ignore]` markers on the convergence tests). Cost is one extra ~2-min Docker build per CI run; benefit is real convergence/compose/container coverage on every PR instead of trust-me coverage.

### Multi-Machine Session Aggregation — SHIPPED
Implemented via NATS leaf node architecture over Tailscale. Hub NATS on VPS accepts leaf connections on :7422 with token auth; each machine runs a local leaf NATS that forwards events to the hub. `NatsBus::connect()` supports `nats://TOKEN@host:port` URLs. All sessions from all machines land on every node (JetStream propagates bidirectionally). Dual MCP servers (local + remote) let agents query either instance. See `docs/deploy/distributed.md`, `deploy/nats-hub.conf`, `deploy/nats-leaf.conf`. Integration tests cover solo local → solo+VPS → team hub → team+guests state machine (`rs/tests/test_deployment_states.rs`).

### Distributed Deployment Security Hardening
With NATS leaf node streaming, every machine gets a full copy of all team data (sessions, prompts, file contents, tool outputs). This is the correct sovereignty behavior but raises security concerns for team deployments. Items to address:

**NATS accounts for team partitioning.** Today all leaf nodes share a single NATS account — everyone sees everything. NATS accounts would let each team member publish to their own subject namespace and selectively subscribe to others. This enables the "Team Partitioned" deployment state where alice sees only her sessions locally unless she explicitly subscribes to bob's. Requires NATS account configuration on the hub and per-user credentials on each leaf.

**Credential files instead of token-in-URL.** The NATS token currently appears in the URL (`nats://TOKEN@host:port`), which shows up in process listings and Docker inspect. NATS supports credential files (`.creds`) that keep secrets out of command-line args and environment variables. Update `NatsBus::connect()` to accept a `--nats-creds` path. (Log-output leakage is covered separately as a hotfix — see "HOTFIX: Redact NATS token from startup logs" above.)

**SQLCipher for local stores.** Every machine's SQLite database contains all team sessions in plaintext. The `db_key` config field already exists but isn't exercised in the distributed deployment. Document and test SQLCipher with the leaf node setup so stolen laptops don't leak team data.

**API auth on the hub dashboard.** The VPS hub serves the common dashboard. Without `OPEN_STORY_API_TOKEN`, anyone on the Tailscale network can browse all sessions. Document setting the token and update the Caddy config to pass auth headers.

### Multi-Directory Watcher
Accept multiple `--watch-dir` roots, backfill concurrently, and resolve project_id correctly across all roots with longest-prefix matching. Currently uses `watch_dir` + `pi_watch_dir` as separate config fields. Generalize to `watch_dirs = [...]` array.

### SQLite as Always-On Analytics Layer
Today the server uses either SQLite or MongoDB as its EventStore — one or the other. Scripts like `token_usage.py` query SQLite directly, so they break when the server runs with the Mongo backend. SQLite should always be populated regardless of the primary backend, the same way the JSONL backup is always written. The persist consumer would gain a second write path: (1) write to the configured EventStore (Mongo or SQLite), (2) always write to a local SQLite copy for analytics/scripts/FTS. This makes `token_usage.py`, `sessionstory.py`, and `query_store.py` work no matter which backend is active. The SQLite copy is the local analytics layer — cheap, fast, always available — while Mongo is the durable primary for multi-machine aggregation.

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

### Sentence Identity & Query API
Two pieces: identity and querying.

**Identity.** The sentence detector emits `PatternEvent`s with a deterministic DB key (`{pattern_type}:{started_at}:{session_id}`) but no first-class `sentence_id` field. The MCP server derives this key client-side, which is fragile. Refactor the sentence detector (`rs/patterns/src/sentence.rs`) to emit a `sentence_id: Uuid` — deterministic hash of the sorted `event_ids` — as a field on the `PatternEvent` metadata. This gives sentences a content-addressed identity: same events always produce the same ID regardless of timestamp precision. The sentence ID becomes the stable key for the paragraph/story hierarchy (paragraphs reference sentence IDs, stories reference paragraph IDs — see `openstory-research/memory/` for the fold design).

**Cross-session query endpoint.** `GET /api/sentences` — queries the patterns table for `type = 'turn.sentence'` with filters, not scoped to a single session. This is the foundation for the MCP `session_sentences` tool to support time-range queries ("last 3 days") and cross-session analytics.

Filters (all optional, composable):
- `days=N` / `since=ISO8601` — time range on `start_time`
- `session_id=X` — scope to one session
- `verb=committed` — filter on `metadata.verb` (SQLite `json_extract`, Mongo dotted-path)
- `entity=patterns.rs` — substring match on `metadata.object`
- `role=Verificatory` — filter on `metadata.subordinates[].role`
- `human=benchmark` — FTS or LIKE on `metadata.human.content`
- `min_duration=120000` — duration threshold on `metadata.duration_ms`
- `limit=50` / `offset=0` — pagination

Response: lean sentence index (id, turn, session_id, summary, verb, object, human_prompt truncated, started_at, event_count). Full event_ids and metadata available via `GET /api/sentences/{id}` detail endpoint.

**Both backends.** Must be implemented in `SqliteStore` (via `json_extract` + `strftime` + `LIKE`) and `MongoStore` (via dotted-path + `$dateFromString` + `$regex`). Add conformance helpers following the existing C1/C2/C3 parity model in `rs/store/tests/event_store_conformance.rs`.

Estimate: ~30 lines in detector for sentence_id, ~150 lines per backend for the query, ~50 lines API handler, ~100 lines conformance tests, MCP tool update.

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

### Bump async-nats to clear rustls-webpki CVE
Dependabot alert #15 — `rustls-webpki 0.102.8` is flagged for [GHSA-4cqp-r62p-h3hg](https://github.com/rustsec/advisory-db) (CRLs not considered authoritative by Distribution Point due to faulty matching logic). The fix is in 0.103.10. We can't bump it directly: it comes through `async-nats` (currently pinned to `0.38` in `rs/bus/Cargo.toml`), and `async-nats 0.38` requires `rustls-webpki ^0.102`. I tested bumping to `async-nats 0.39` — it builds clean but **still** pulls in 0.102.x. The actual fix is somewhere further up the async-nats version line (latest is 0.47). Each minor bump in pre-1.0 land is potentially API-breaking, so this needs: (a) find the smallest bump that pulls in rustls-webpki 0.103, (b) update `bus/src/lib.rs` and `bus/tests/nats_integration.rs` for any API drift, (c) verify against a live NATS server with `just test-compose`. Deferred from PR #16, which closed the other 4 alerts (vite × 2, lodash × 2). One medium-severity CVE remains until this lands.

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

### Readonly DB Access from OpenClaw Container
Give the OpenClaw container readonly access to Open Story's SQLite database (or a replica). OpenClaw agents could query their own session history, tool patterns, and behavioral analytics directly — enabling self-reflection without going through the REST API. This is the "let the coalgebra read its own algebra" path for pi-mono, parallel to the Hermes recall tool but using direct DB access instead of HTTP. Design considerations: SQLite WAL mode allows concurrent readers, but cross-container file sharing needs a shared volume. Alternative: a readonly SQLite replica synced from the primary, or a dedicated readonly API endpoint scoped to the agent's own sessions.

### Tool Result Syntax Highlighting (T1 from architecture audit)
`ToolResultDetail` in `ui/src/components/RecordDetail.tsx:252` renders Read tool output as `<CodeBlock>{output}</CodeBlock>` with no language/path/toolName props, so `detectLanguage` falls through to `"text"` and rust/python/toml files display uncolored. The paired ToolCall carries the file path via `call_id` — fix is UI-side: parent component already has the ViewRecord list, look up the paired ToolCall and pass `filePath` + `toolName` down to `ToolResultDetail` → `CodeBlock`. Also wire `strip-line-numbers.ts` into this path (pi-mono bakes line numbers into Read output; they interfere with highlighting). Write UI unit test first — expect `language="rust"` when a paired ToolCall has `.rs` input. See `docs/research/architecture-audit/T1_SYNTAX_HIGHLIGHTING.md` for full recon.

### Case-insensitive Tool Map in UI (T1b)
`ui/src/lib/detect-language.ts:38` TOOL_MAP uses PascalCase keys (`Bash`, `Grep`, `Glob`) — mirror the case-sensitivity bug fixed in `rs/views/src/tool_input.rs` for pi-mono. Lowercase-normalize tool name before lookup, or add lowercase aliases. Low-risk, one-liner.

### NATS Subject Sanitization (T3 from architecture audit)
`rs/core/src/paths.rs:38` `nats_subject_from_path()` composes subjects via raw string interpolation of project and session names. Path segments containing `.`, ` `, `*`, or `>` flow into the subject unchanged — dots create extra tokens that break `events.{project}.>` hierarchical subscriptions, spaces produce NATS-invalid subjects that fail at publish, and wildcard characters shadow subscription matching. Not hit in practice today (Claude Code / pi-mono default dirs use UUIDs) but a latent footgun. Fix: lightweight sanitizer that replaces the four problem characters with `_` and logs a warning when rewriting. See `docs/research/architecture-audit/T3_NATS_SUBJECT_ALIGNMENT.md` for three design options (sanitize / percent-encode / hash-prefix) and the recommendation. L1 characterization tests are already in place at `paths.rs` `subject_*` tests — they'll catch any divergence when the sanitizer lands.

### Wire ↔ Projection Sync When Decomposing Broadcast (T5 from architecture audit)
Today's broadcast path at `rs/server/src/ingest.rs:136-253` calls `proj.append(&val)` then `to_wire_record(vr, proj)` inside the same synchronous loop iteration, so the wire record always reflects its own event. The actor-consumer architecture stated goal (see comment at `rs/src/server/mod.rs:242` — "This is the last consumer to decompose") is to move broadcast onto its own NATS subscription, at which point Actor 4 and Actor 3 (projections) are independent subscribers and the "wire before projection" race opens. Options documented in `docs/research/architecture-audit/T5_WIRE_PROJECTION_SYNC.md`: (1) RwLock-shared projection with wait-for-catchup, (2) per-batch NATS-sequence barrier, (3) Actor 4 maintains its own projection. Also: Actor 3 today writes projections that nothing reads — dead code until this decomposition lands.

### Promote Agent Payload Round-Trip Tests into Conformance Suite (T6 from architecture audit)
Three inline tests in `rs/store/src/sqlite_store.rs` (`t6_pi_mono_agent_payload_round_trips`, `t6_claude_code_agent_payload_round_trips`, `t6_hermes_agent_payload_round_trips`) cover AgentPayload variant + typed-field round-trip for SQLite. Move them (with a backend-agnostic builder helper) into `rs/store/tests/event_store_conformance.rs` so MongoStore inherits the same guarantees. Mongo uses BSON which has real type-width quirks (i32 vs i64, datetime coercion) that a blob-TEXT SQLite pass can hide — this is the natural place to catch them. Low risk; one builder refactor.

### Decompose Actor 4 (Broadcast Consumer) from Shared AppState
Documented in-code at `rs/src/server/mod.rs:240`: "Actor 4: broadcast consumer (uses ingest_events for now) — Still uses shared AppState because BroadcastMessage assembly depends on projection state. **This is the last consumer to decompose.**" Actors 1–3 (persist, patterns, projections) own their state and talk only to NATS. Actor 4 still reaches into `state.store.projections`, `state.store.full_payloads`, `state.store.session_projects`, etc. via `ingest_events`, which keeps a monolithic code path alive in a system that's otherwise actor-sharded.

Work: move broadcast onto its own independent NATS subscription, owning its own state needed for WireRecord assembly (truncation cache, full_payloads). Two sub-concerns baked in:
- **Projection freshness** (T5): once Actor 4 can't read Actor 1's projection synchronously, a barrier is needed so wire records never reference a parent_uuid the projection hasn't seen.
- **Single-owner invariants**: `ingest_events` currently does work that rightfully belongs to Actors 1–3 — the JSONL append was one (fixed 2026-04-15), but FTS indexing and plan extraction still live there. Each needs to move to its rightful owner or be explicitly declared dual-write with a justification.

The JSONL torn-line bug at BACKLOG entry "JSONL Escape-Hatch Append Integrity" is the first of these to get caught in the wild — expect more as the decomposition work surfaces them. Track additions here as they land.

### JSONL Escape-Hatch Append Integrity (surfaced by schema registry capstone)
**Severity: high — violates the sovereignty contract.** Running `cargo test -p open-story-schemas --test test_jsonl_escape_hatch -- --ignored` against real committed data surfaces 273 malformed lines across 3 of 40 sampled session files. Failure is not a schema mismatch — `serde_json::from_str` fails on "trailing characters," meaning two CloudEvents were written to a single line with no newline between them. Worst offenders: `55ceca28-...jsonl` (169 bad lines), `06907d46-...jsonl` (137), `0f7b6541-...jsonl` (129). All written 2026-04-07 — this is a current bug, not ancient history.

Suspected root cause: concurrent writes into the `SessionStore` JSONL appender without locking, or a torn write followed by unlocked append. Per CLAUDE.md the JSONL backup is explicitly the sovereignty escape hatch: "your data is always grep-able from outside the database." Torn lines break `jq`, `grep -c`, any external tool that trusts the one-event-per-line invariant.

Fix approach: audit `rs/store/src/persistence.rs::SessionStore::append`. Confirm it acquires an exclusive lock (advisory `fcntl`/`flock` on Unix, or equivalent), holds it across the `write + newline` pair, and fsyncs. Also: the appender should never silently drop — if it can't write a full line, the error must surface, not truncate.

Test in place at `rs/schemas/tests/test_jsonl_escape_hatch.rs` — will go green the day this is fixed.

### Pair tool_result to pending_apply by call_id (eval-apply walk F-1)
**Severity: medium — silent data corruption on pi-mono parallel tools.** `rs/patterns/src/eval_apply.rs:240-280` resolves each `message.user.tool_result` event against `pending_applies.first().clone()` and drains FIFO, ignoring `tool_call_id`. Sequential tool use is fine; **parallel tool use** (pi-mono's bundled `[toolCall, toolCall]` decomposing into 2 assistant events + 2 result events) corrupts when results arrive in completion order rather than call order — the fast tool's outcome attaches to the slow tool's call and vice versa.

Fix: extend `PendingApply` with `call_id: String`, capture from `assistant.tool_use` event's `agent_payload.tool_use_id`/`tool_call_id` (depending on agent), and on `tool_result` find by id rather than `[0]`. ~30 LOC. Test `parallel_tool_results_out_of_call_order_currently_misattribute` characterizes the bug today; flips green → red on fix; delete it then. See `docs/research/architecture-audit/EVAL_APPLY_WALK.md` F-1.

### Accumulate Assistant Text Across Multi-Event Turns (eval-apply walk F-2)
`rs/patterns/src/eval_apply.rs:282-336` overwrites `pending_eval.content` on each `message.assistant.*` event. For pi-mono decomposed turns where `assistant.text` and `assistant.tool_use` both arrive, the second overwrites the first — narrative content is silently dropped. Fix: append rather than replace, OR push into a `Vec<String>` and join at `turn_complete`. Test `assistant_text_then_tool_use_overwrites_pending_eval_content` characterizes today's behavior. See `docs/research/architecture-audit/EVAL_APPLY_WALK.md` F-2.

### WebSocket Lagged Notification (WS walk F-1)
`rs/server/src/ws.rs:180-183` swallows `RecvError::Lagged(n)` with only a `log_event` line. The UI never knows it missed `n` broadcast messages — sidebar counts, timeline, and token totals silently diverge from server truth until a manual page reload triggers a fresh `initial_state`. Fix: send a `{kind: "lagged", skipped: n}` notification so the UI can refetch (cheapest), or close the socket so the client reconnects (most honest). See `docs/research/architecture-audit/WS_LAYER_WALK.md` F-1.

### `delete_session` Should Probably Remove the JSONL Backup (API walk F-2)
`DELETE /api/sessions/{id}` (`rs/server/src/api.rs:1230`) removes events from EventStore + projections + caches + project mappings, but leaves `data/{session_id}.jsonl` (the SessionStore backup file) on disk. The file is inert (boot replay reads from EventStore, not JSONL) so the session doesn't resurrect, but the local trace remains until manually `rm`'d. Decide: should DELETE be a "forget completely" operation, or does sovereignty mean we never touch the user's local backup? If "forget completely," add `SessionStore::delete_session(sid)` and call it from the API handler. If sovereignty wins, document it explicitly in the endpoint doc comment so users know the file remains. See `docs/research/architecture-audit/API_WALK.md` F-2.

### Cap `search_events.limit` at a sane upper bound (API walk F-4)
`/api/search?limit=` is an unbounded `usize` (`rs/server/src/api.rs:932`). `limit=1000000` returns up to 1M FTS5 hits, killing the client and the server's response-serialization. Trivial fix: `query.limit.min(MAX_SEARCH_LIMIT)` where `MAX_SEARCH_LIMIT = 500` or similar. See `docs/research/architecture-audit/API_WALK.md` F-4.

### Pi-Mono Sessions Have No Story (Recursion Principle Test, F-1)
The recursive-observability principle test surfaces this: pi-mono sessions never produce `turn.sentence` patterns because pi-mono doesn't emit `system.turn.complete`. The eval-apply state machine waits for that subtype to crystallize a `StructuralTurn`; without it, no turns, no sentences, no story. Pi-mono visibility in the UI was fixed during the hermes-integration branch (the line-is-unbroken commit), but pi-mono *narration* — the rendered SVO sentence per turn — still doesn't work. Fix shape: derive a turn boundary from pi-mono's own signals (e.g., `stop_reason: "stop"` on the assistant message, OR end-of-response marker, OR session timeout). See `rs/tests/test_principle_recursive_observability.rs` for the test that catches this.

### Story-Rendering Catch-Up for Sessions Without Hooks (Recursion Principle Test, F-2)
~40 historical claude-code sessions in the local instance have ZERO `system.turn.complete` events because they were ingested via the watcher path without the Stop hook configured. They have full event history but no turn boundaries → no sentences. New sessions with hooks work fine. Fix shapes (any of): (1) infer turn boundaries from event clustering on watcher-only sessions; (2) document hook setup in onboarding so this doesn't keep happening; (3) backfill turn.complete events on a re-ingest pass. Surfaced by the recursion test.

### CI Testcontainers Spike
Investigate what's needed to run Docker-based testcontainer tests (compose tests, container integration tests) in GitHub Actions CI. Currently skipped because CI runners lack the local `open-story:test` image and Docker setup. Spike should cover: GitHub Actions Docker service containers vs Docker-in-Docker, building the test image in CI (caching strategies for the Rust build), NATS sidecar setup, and whether the compose tests can run within the free-tier minute budget. Goal is a concrete proposal, not implementation.

---

## Done (not tracked here)

Completed work lives in git history. For reference, major completed features include: pattern detection pipeline (5 detectors), SQLite event store, pub/sub via NATS, live timeline, explore view split, subagent enrichment, stateful BFF projection, enriched event envelopes, view model crate, testcontainers E2E, configurable projects dir, syntax highlighting, and open-source licensing cleanup.
