# Backlog

Ideas and future work for Open Story. Each entry describes *what* and *why* in a short paragraph. When work begins, create a branch — the backlog entry is the spec.

---

## Overlay annotations (the pin-a-note layer)

Annotations are user/agent-authored notes pinned to sessions — the overlay
namespace (`annotations.jsonl`, never the observed event stream). Add/list/
remove exist (`POST`/`GET`/`DELETE /api/annotations`) + a corner overlay with a
× remove. Follow-ups to make them first-class:

### Show annotations in context, not just the corner overlay
A note pinned to a session only appears in the global bottom-right overlay. It
should render *on that session* — inline in Explore/Story (e.g. a margin note
on the turn/event it targets) and as a badge on the session's cards — so the
note lives where the thing it annotates lives.

### Anchor annotations to a target finer than a session
Today `session_id` is the only anchor. Let a note target a specific event/turn,
a file, or even a viz element (a Canvas node, a heatmap day), carrying an
optional `anchor` (event_id / turn / file / selector). The interaction schema
already captures selections — reuse it so "annotate what I'm looking at" works.

### Provenance & ownership on delete
`DELETE /api/annotations/{id}` currently lets anyone remove any note. With the
person/principal model, deletion (and edit) should respect authorship — you can
remove your own notes; an agent's notes are labeled and removable by their
principal. Also add EDIT (`PATCH`) so a note can be reworded without delete+re-add.

### Surface created_at + issuer richly
Notes store `created_at` and `issuer` but the overlay only shows issuer + a
short id. Show a relative timestamp (with absolute on hover — see the
timestamps-everywhere sweep) and make provenance legible (person vs agent).

### Annotation threads / replies
A single body per note is thin for a real conversation-in-the-margin. Consider
threading (reply to a note) so a human and an agent can discuss a session
inline — the overlay becomes a lightweight review surface.

## Federation & transport — follow-ups from the host-in-subject work

### Container/e2e tests: NATS-at-boot
`rs/tests/test_container.rs` fails because the `open-story:test` image's
CMD (`serve …` with no `--manage-nats` and no bundled NATS) can't reach a
NATS server — pre-existing, identical on master, surfaced once NATS became
a hard boot dependency. Fix the test image to manage/bundle NATS so the
Dockerized data path can be verified end-to-end.

### Federated sessions have records but no turn.sentence patterns (blank Story)
`turn.sentence` patterns are derived by the patterns-consumer actor as events
flow through the *live local* NATS pipeline (eval-apply → sentence detectors).
When a session streams in from another node, the raw CloudEvents replicate but
the receiving node never re-runs the detectors, so remote-host sessions have
records but **zero patterns** — and the Story view (which renders sentences,
not raw records) is blank for them. Confirmed 2026-07-02: a local session had
106 `turn.sentence` patterns; a `Katies-Mac-mini` session had 36 records / 0
patterns. Diagnose with `GET /api/sessions/{id}/patterns?type=turn.sentence`
returning empty while `/records` is populated. Given the fleet's host spread
(a1, Maxs-Air, Katies-Mac-mini, …) this blanks the Story for a large fraction
of federated sessions. Three fixes, roughly increasing cost:
1. **Client-side fold fallback (quick win):** when the patterns fetch is empty,
   the Story view fetches records and runs `ui/src/lib/eval-apply.ts::extractCycles(records)`
   to render turns locally. No backend change; works for any records-bearing
   session, federated or not. Renders structural turns but not the full
   sentence grammar (verb/adverbial) — that still needs the detectors.
2. **Consume patterns from other hosts:** federate the `patterns` stream too
   (mirror/source it like `events`), so a peer's derived sentences replicate
   alongside its raw events.
3. **Run sentences locally on federated events:** re-run the pattern detectors
   on inbound federated/backfilled events at ingest so patterns materialize for
   remote sessions on the receiving node. Correct long-term; touches the
   federation + patterns-consumer path.
Surfaced 2026-07-02 while giving a UI tour — navigated to a federated session's
Story and it looked empty.

### Federated catch-up re-injects to a host-less subject (silent no-op)
`rs/server/src/catch_up.rs:110` publishes healed events to
`events.{sid}` (flat, pre-host form, with `project_id = ""`). In federated
mode a leaf's local `events` stream binds only `events.{host}.>`
(`nats_bus.rs` `events_stream_config`), so the publish matches no stream and
is dropped — `catch_up_once` treats it as not-healed and the session never
converges. Catch-up is the anti-entropy backstop *for federated cold-boot
loss*, so it fails in the exact mode it exists for. (Solo works because the
stream binds `events.>`.) Catch-up only heals sessions a peer actually
published into the network — a node running with `publish_sessions = false`
never put its sessions on the bus, so they are invisible to catch-up by
construction. Fix: build the re-injection subject with the originating host
+ real project
(`events.{host}.{project}.{sid}.main`), taken from the peer event's own
fields, not relabeled as local. Surfaced 2026-06-23 in the PR #58 deep review.

### Unsanitized project/session/agent_id in NATS subjects
`rs/core/src/paths.rs` `nats_subject_from_path` sanitizes only the `host`
token (via `host::normalize`); `project`, `session`/`file_stem`, and
`agent_id` are interpolated raw (the repo's own tests at `paths.rs`
document a dotted project name inflating the token count and a space
producing an invalid subject → publish failure → silent per-session loss).
The host prefix being sanitized means the double-count guard holds, so this
is HIGH-not-blocker, and Claude Code project dirs are path-encoded
(dot-free) — but Codex/pi-mono/arbitrary watch dirs aren't guaranteed safe.
Fix: lift `host::normalize` to a shared `sanitize_subject_token` and apply
it to all interpolated tokens. Surfaced 2026-06-23 in the PR #58 deep review.

### cargo-vet apparatus exists but isn't enforced in CI
This branch added the full `rs/supply-chain/` cargo-vet directory
(config/audits/imports), and `cargo vet check` passes today — but no
workflow runs it (`grep 'cargo vet' .github/workflows` → nothing). So the
supply-chain dir gives a false sense of enforcement: the next PR adding an
un-exempted crate won't be caught. Fix: add a `cargo vet --locked` step to
`.github/workflows/test.yml` (or document it as a required manual pre-merge
check). Surfaced 2026-06-23 in the PR #58 deep review.

---

## Actor pipeline — follow-ups from Phase 1.4.5 (async boot replay)

### Boot-replay status on `/api/health`
Async replay lets HTTP bind in ~3s, but projections keep populating in the background for 5–60s (depending on SQLite event count). During that window `/api/sessions` returns rows with empty/zero `label`/`event_count`/`tokens` — looks broken to the user. Add a `replay_status: "in_progress" | "complete"` field to `/api/health` (and the WebSocket `initial_state` handshake) so the UI can render a "Reconstructing sessions…" hint instead of silent-empty rows. ~20 LOC. **Folded into the broader "Self-reporting `/api/health` endpoint" entry below.**

### Self-reporting `/api/health` endpoint (silent-state-mismatch detector)
Recurring class of debugging pain: every layer of OpenStory thinks it's working, the symptom appears downstream, and finding the cause requires SSH-ing into containers and running SQL. Today's instances (in one session): old container served stale UI bundle, browser tab pointed at pre-PR client hammered an unbounded endpoint, schema migration silently fell back to JsonlStore (so `/api/sessions` returned `event_count: 0` for everything while persist consumer logs showed real activity), NATS leaf reachability un-verifiable from one machine. Each of these took 10–30 minutes of spelunking; each would have been one curl with a real health endpoint.

**The shape:** `GET /api/health` returns a JSON document where every layer reports what *it* thinks is true. The human (or another agent) compares them. Drift between layers = bug.

```json
{
  "version": "0.X.Y+commit-sha",
  "uptime_secs": 14400,
  "build_time": "2026-05-01T15:25:39Z",
  "stores": {
    "event_store": {
      "backend": "JsonlStore",          // ← this would have screamed today
      "expected_backend": "SqliteStore",
      "fallback_reason": "no such column: host"
    },
    "schema_version": 7,
    "schema_migrations_pending": ["add_host_column"]
  },
  "watcher": {
    "watch_dirs": ["/watch"],
    "exists": [true],
    "last_file_event_ago_secs": 12
  },
  "bus": {
    "nats_url": "nats://...",
    "connected": true,
    "leaf_connected": true,
    "leaf_remote": "100.77.40.95:7422",
    "leaf_rtt_ms": 37,
    "streams": {
      "events":   { "msgs": 6771, "bytes": 1073568465, "last_seq": 15970 },
      "patterns": { "msgs": 5131, "bytes": 268342025 }
    }
  },
  "boot_replay": {
    "status": "complete",
    "events": 106308,
    "sessions": 442,
    "duration_secs": 3
  },
  "ingestion": {
    "events_last_60s": 23,
    "last_persist_ago_secs": 4,
    "lagged_ws_messages": 0
  },
  "ui_bundle_version": "..."
}
```

**Why each field:**
- `event_store.backend` vs `expected_backend` — surfaces silent SQLite→JSONL fallback (the single most surprising failure mode in this codebase).
- `schema_migrations_pending` — surfaces stale databases pre-emptively, before they trigger fallback.
- `watcher.last_file_event_ago_secs` — surfaces a stuck watcher (active coding but no events flowing). Distinguishes "you're not coding" from "watcher broke."
- `bus.leaf_connected` + `bus.streams` — surfaces NATS issues (the leaf-to-hub path that none of us can otherwise verify from a single machine). Pulls from `/leafz` + `/jsz` on the local NATS HTTP monitor port; OpenStory just re-exposes them in one place.
- `boot_replay` — folds in the existing "Boot-replay status" item above.
- `ingestion.events_last_60s` + `last_persist_ago_secs` — surfaces "nothing is flowing" without requiring a tail of the container logs.
- `ui_bundle_version` — distinguishes "old container still running" from "code is up to date" (we got bitten by this earlier today).

**UI surface:** a status indicator in the Header that pulls `/api/health` every 30s. Green = all expected. Yellow = recoverable drift (boot replay in progress). Red = drift (fallback active, watcher stuck, leaf disconnected). Click to expand the full JSON in a panel.

**Estimate:** ~150 LOC for the endpoint + ~50 LOC per store/bus/watcher contributor (each layer adds a tiny `report_health()` method) + ~80 LOC for the UI indicator. The biggest cost is socializing the convention: "every new layer adds itself to /api/health."

**Why this matters specifically for OpenStory:** the project's soul is *visibility into what your agents are doing*. The tool itself should hold to the same standard — visibility into what *it* is doing. Every silent failure mode in OpenStory is an instance of the tool failing the principle it sells.

### Lazy session hydration
Surfaced 2026-05-16 when `just up` hung 4+ minutes at 215% CPU before binding `:3002`. `replay_boot_sessions` (`rs/server/src/ingest.rs:343`) walks every session and replays every event through the views projector on every boot just to warm in-memory caches — unusable at ~200k Mongo events × BSON deserialization. Replace with a cheap `bootstrap_session_index` (one `list_sessions()` read, no event scan) plus per-session `hydrate_session(sid)` triggered on first read, coordinated via `AppState::ensure_hydrated` + single-flight `tokio::sync::OnceCell`. Persist subagent parent/child links to a new `subagent_links(child_session_id PK, parent_session_id, detected_at)` table (SQLite) / collection (Mongo), written by the persist consumer when `detect_subagent_relationship` fires for the first time per child — so they don't get rebuilt from events on every boot. FTS5 backfill moves to a noisy background task (`eprintln!` per-session progress in the project's existing logging style). Add `POST /api/admin/rehydrate[?session_id=…]` so the UI can re-trigger the audit. Large tool-payload fetches for cold sessions resolve directly from the EventStore instead of forcing a full session hydration. Closes the eager-replay class of problems that "Bounded `full_payloads` cache", "Live-event-during-replay race bound", and "Replay-window load test" all currently work around. Design plan staged for execution.

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

### Watcher emits fresh `file.snapshot` events on every boot (pollutes old sessions)
Sibling to "Synthetic Event ID Stability" below. On each `just up`,
the watcher walks `~/.claude/projects/<project>/<session>.jsonl` files
and re-emits `file.snapshot` events stamped with `time = now()`,
attributed to whichever session_id the JSONL filename encodes. So a
session that genuinely ended in April keeps accumulating "today" events
on every reboot — observed in `data/f2679c73-…jsonl` as 504
`file.snapshot` events from 2026-04-30 added to a session whose real
last activity was 2026-04-09. The upsert MIN/MAX fix on
`feat/lazy-load-initial-state` stops these new events from corrupting
the persisted span, but the events themselves still land in the wrong
session and inflate `event_count`. Symptoms: the "Today" filter on the
Sessions sidebar shows old sessions because their `last_event` is a
real-but-spurious `file.snapshot` from this morning. Two possible fixes:
(1) skip `file.snapshot` emission for sessions whose source JSONL hasn't
been written-to in N days (mtime-gated backfill), or (2) derive the
synthetic event's `time` from the source file's mtime / parent event's
timestamp rather than `now()`. Option 2 is the cleaner fix because it
also satisfies the "Synthetic Event ID Stability" content-derived-ID
goal — same `(path, mtime, content_hash)` → same id AND same time. ~40
LOC in the translator + watcher boot path; pairs naturally with the ID
stability work below.

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

### Live Timeline doesn't render `agent-*` subagent sessions (filter mismatch)
**Severity: medium — pre-existing, not caused by user-stamping.** Navigating to `/#/live/agent-<HEX_AGENT_ID>` loads the session header and successfully fetches its records via `GET /api/sessions/agent-.../records` (verified: 130 events on disk, 257KB of payload), but the Live timeline renders empty.

Cause: the records returned have `session_id = <parent UUID>` and `agent_id = <bare hex without "agent-" prefix>`, but `Timeline.tsx:364` filters with `ev.session_id === sessionFilter` where `sessionFilter` is the URL-supplied `agent-<HEX>` pseudo-id. No record's `session_id` ever equals an `agent-`-prefixed string, so the filter excludes every event. The data is loaded, deduped, indexed in `treeIndex` — only the render-time predicate is wrong.

Fix shape (~10 LOC):
```ts
// ui/src/components/Timeline.tsx ~line 364
if (sessionFilter) {
  if (sessionFilter.startsWith("agent-")) {
    const aid = sessionFilter.slice("agent-".length);
    filtered = filtered.filter((ev) => ev.agent_id === aid);
  } else {
    filtered = filtered.filter((ev) => ev.session_id === sessionFilter);
  }
}
```

Workaround today: open the same session under Explore — `SessionTimeline.tsx` fetches via `/api/sessions/{sid}/records` and renders without the parent-session filter, so events show up. Pairs with the entry above ("Subagent Task Labels"); the same area should grow proper subagent affordances together — labels for the sidebar, this filter for the timeline, possibly a per-subagent depth profile. Worth a small UI-only PR. Add a `Timeline` test that asserts events with `agent_id = X` render under route `/live/agent-X`.

### Domain Events & Workspace Impact — SHIPPED
`ToolOutcome` enum implemented in the translate layer: `FileCreated`, `FileModified`, `FileRead`, `CommandExecuted`, `SearchPerformed`, `SubAgentSpawned`. Domain fact badges visible on every Story card. `SubAgentSpawned` carries `agent_id` for parent-child linking. Remaining: `ToolOutcome` for pi-mono (`translate_pi.rs`).

### Agent Behavior Patterns
Cross-session analytics revealing longitudinal trends: tool preferences, session duration, token consumption over time, error rates by task type. Answers questions like "I spend 60% of tokens on test-writing" by aggregating over persisted event data.

### Plan Visibility
Make extracted plans first-class objects in the UI: filterable in the Live timeline, searchable in Explore, viewable inline, with plan counts on session cards. Plans are already extracted during ingestion; this brings them into the frontend.

### Live Token Counter
Real-time running token accumulator in the session header that ticks up as events arrive. Shows input tokens, output tokens, and estimated cost as a pure UI component subscribing to WebSocket assistant events.

### Agent Context Compaction via Open Story

When an agent's session overflows its context window, the current approach is self-summarization — the LLM tries to summarize its own 300K+ token history, which is expensive, slow, and fails when the context itself is too big to summarize. Open Story already has the answer: structured session analysis (sentences, patterns, tool histograms, prompt timelines) computed incrementally as events arrive.

The idea: instead of a full summary, Open Story provides a **compact map** — a small structured summary (~500 tokens) plus pointers to MCP tools for just-in-time retrieval. The agent carries the map, not the territory. When it needs specifics, it calls `search()`, `session_sentences()`, or `session_patterns()` to recover context on demand.

Shape:
1. **`session_compact` MCP tool** — takes a session ID + token budget, returns a structured summary (key sentences, tool histogram, prompt timeline, work-in-progress state) sized to fit the budget, plus a "how to find more" section listing the MCP tools and example queries.
2. **OpenClaw compaction hook** — intercept the `session_compact` extension event and call Open Story instead of the default LLM self-summarization. Falls back to default if Open Story is unreachable.
3. **Session ID awareness** — the agent needs to know its own session ID. The session header in the JSONL already has it; surface it as an env var or MCP tool.

This is the mirror being useful: Open Story has been watching the whole session, it already did the analysis, and retrieving it costs zero tokens. The agent looks at itself through Open Story and decides what to carry forward.

Motivated by: Bobby's session hit 331K tokens / 1,032 messages and OpenClaw's auto-compaction failed because the summarization request itself exceeded the 200K context limit. Open Story had the structured analysis ready the whole time.

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

### MCP performance testing — spike
We have a criterion bench scaffold at `rs/mcp/benches/` (added in commit `perf(mcp): criterion benches…`) covering per-tool latency (`tools.rs` — `tools/list`, `list_sessions`, `session_synopsis`, `search` at 10/100/1000-session sizes) and streaming throughput (`streaming.rs` — `subscribe_session` batches/sec through the stdio pipe). Run with `cargo bench -p open-story-mcp` — the baseline numbers from when it landed are in that commit message. Right now this is a measurement scaffold only: nothing runs in CI, no committed baseline, no regression gate. The spike is: figure out whether MCP performance is load-bearing enough to deserve a regression gate, and if so which form. Options sketched out in the design conversation: (a) save criterion baselines into the repo and document `cargo bench -- --baseline main` as a manual pre-merge check; (b) a CI workflow that runs benches and fails on >X% regression — known noisy on shared GitHub runners, so threshold tuning is the open problem; (c) a tiny `tests/perf.rs` that runs the same drivers with hard-coded latency ceilings — cheaper but cruder. Threads to pull: what's the actual agent workload (subscribe-heavy? query-heavy?), what's the cheapest meaningful regression signal, and whether HTML reports in `rs/target/criterion/` are worth committing as artifacts. Surfaced 2026-05-21 alongside the MCP-rewrite PR — keep the scaffold runnable-on-demand until a real perf problem makes the gate worth the noise tax.

### MCP streaming tools over WebSocket (drop the NATS dependency)
The query tools now read through the REST API (`HttpEventStore`, commit `feat(mcp): read query tools through REST API`), so the MCP no longer opens SQLite relative to its launch directory. The two streaming tools — `subscribe_session` and `subscribe_tokens` — still subscribe over NATS via `crate::subscription::Subscribe` (`NatsBus`), so the binary keeps a hard NATS dependency (`OPENSTORY_NATS_URL`, default `nats://localhost:4222`) and bails at boot if NATS is unreachable. NATS is a fixed URL, not cwd-relative, so it isn't the directory-fragility bug — but it's a second piece of infrastructure the MCP must reach. Phase 2: migrate both streaming tools to the server's `/ws` WebSocket (`rs/server/src/ws.rs`) so the MCP needs only `OPENSTORY_API_URL`. Design notes: `/ws` currently opens with an `initial_state` frame (patterns + session_labels for sessions within `watch_backfill_hours`) and then broadcasts `BroadcastMessage`s for *all* sessions; the client ignores inbound messages. To replace NATS, the streaming layer needs a per-session subscription (filter the broadcast to one `session_id`, or add a subscribe frame the server honors) and a `Subscribe` impl backed by a WS client (`tokio-tungstenite` / `reqwest`'s upgrade) that adapts the frame shape to the existing `StreamEvent` the stdio handler emits as `notifications/openstory/stream` + `notifications/openstory/tokens`. Threads to pull: does `/ws` need a new "subscribe to session X" control frame (today it's broadcast-to-all), and can `subscribe_tokens`' running tally be computed client-side from the same per-message events. Once done, the binary's NATS env and boot check come out, and `Server` no longer carries a NATS `Subscribe`. Surfaced 2026-06-13 alongside the query-decoupling cutover.

### hickory-proto stuck on 0.25 via mongodb pin
Two open Dependabot alerts on `rs/Cargo.lock` for hickory-proto can't be resolved with a plain `cargo update` because mongodb 3.6.0 pins `hickory-proto = "^0.25"` and the security patches land in 0.26.1. GHSA-94w7-72p3-jvw7 (high, NSEC3 closest-encloser proof unbounded loop) currently has no patched version listed at all. Plan: (1) watch the mongodb crate for a release that widens the constraint to `^0.25 || ^0.26` and bump when it ships; (2) if the high stays open past Q3 2026, evaluate forcing it via `[patch.crates-io] hickory-proto = "0.26"` in the workspace `rs/Cargo.toml` after a small audit of mongodb's hickory-proto API usage. The risk is API drift between 0.25 and 0.26 — mongodb only uses hickory-proto for SRV-record resolution, so the blast radius is narrow. Decline path: dismiss the alerts on GitHub with reasoning "no upstream fix in mongodb's transitive graph" so the noise stops without losing the trail. Surfaced 2026-05-20 alongside the Dependabot bump PR (PR #56) — see commit chore(deps): bump openssl/rustls-webpki/rand/astral-tokio-tar for the rest of the cleanup.

### test_container suite needs a NATS sidecar
`rs/tests/test_container.rs` boots the `open-story:test` image in a single container via the testcontainers crate, but the image's entrypoint requires NATS at boot — so the process exits before binding to port 3002 and every test fails with `PortNotExposed`. The suite has been broken on master for a while (verified 2026-05-20 by stashing the dep-bump branch and rerunning). Fix path: rework the helper at `rs/tests/helpers/container.rs:start_open_story` to either (a) bring up a NATS container alongside via testcontainers' compose support and wire `NATS_URL` into the open-story container, mirroring what `rs/tests/test_openclaw_mcp.rs` already does via raw `docker compose`, or (b) split the suite — keep the no-NATS smoke tests against a `noop-bus` build flavor and move the integration tests to a compose-based harness. The CI side may be passing because GitHub Actions doesn't run `test_container` (or runs it gated); confirm by reading `.github/workflows/test.yml` before fixing.

### CI/CD pipeline
We have GitHub Actions running `cargo test` + UI tests on PRs, but no real release / deploy pipeline. Every Hetzner redeploy is currently a manual `scripts/deploy/deploy.sh` run, and there's no automated path that exercises the production Docker images end-to-end before they ship. What we want: (1) tagged releases that build the `open-story:test`, `openclaw-mcp:latest`, and `open-story-mcp` binary artifacts; (2) on each PR, a workflow that does `docker build` for the openclaw image so we catch Dockerfile drift before merge (currently nothing builds these in CI — they break silently until a deploy); (3) a `deploy` workflow gated on a tag or manual dispatch that ships to Hetzner via SSH, with a rollback path. Bonus: testcontainer-based smoke that asserts the freshly-built openclaw image can connect to a NATS + Mongo container and serve at least one tool call. Existing pieces to compose: `.github/workflows/test.yml` (PR gate), `Dockerfile` (open-story:test), `Dockerfile.openclaw` (openclaw-mcp image), `scripts/deploy/{smoke,deploy}.sh`. Decide between GitHub Actions + GHCR vs. an external CI; prefer GHA for simplicity and visibility next to the PR workflow. Surfaced 2026-05-19 alongside the MCP-rewrite cutover, which exposed that the Docker images are part of the change surface but nothing in CI exercises them.

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

### Dual-sink EventStore (SQLite + Mongo, both live)
Today the system selects exactly one durable backend via `OPEN_STORY_DATA_BACKEND` (sqlite or mongo). `just up` forces Mongo, which means whichever backend isn't running goes stale — verified 2026-05-22 when the Mac's `data/open-story.db` was last-touched 2026-05-16 (the day Mongo became the default), while the live Mongo container had ~6 days of new events the SQLite snapshot never saw. Flipping back to `just up-no-mongo` would silently strand all post-May-16 sessions. The intent was always to have **two sinks**: SQLite as the always-on cheap durable store, Mongo as the analytics/query surface — both fed from every ingest, either able to answer reads if its sibling is down. JSONL persistence (`SessionStore`) is already parallel-to-EventStore on disk, so the principle is established; we just don't express it at the queryable-store layer.

**Shape (sketch):** introduce a `DualSinkStore` in `rs/store/src/` that holds `(primary: Arc<dyn EventStore>, secondary: Arc<dyn EventStore>)` and fans every write method (`insert_event`, `insert_batch`, `upsert_session`, `upsert_plan`, `insert_pattern`, `insert_turn`, `index_fts`) to both, with reads going to `primary`. Config grows a `secondary_backend: Option<DataBackend>` field in `rs/server/src/config.rs` so `data_backend = "sqlite"` + `secondary_backend = "mongo"` boots dual-sink mode. `just up` becomes `OPEN_STORY_DATA_BACKEND=sqlite OPEN_STORY_SECONDARY_BACKEND=mongo ...` (cleaner than encoding both into one string). The conformance suite at `rs/store/tests/event_store_conformance.rs` is already parametric over `Arc<dyn EventStore>`, so adding a `mod dual_sink_backend` that wires `DualSinkStore::new(SqliteStore, MongoStore)` and runs the same 47 helpers is straightforward (call out by `Explore` agent 2026-05-23).

**Open threads:**
- **Failure semantics when secondary is offline.** Block writes (strong consistency, fragile)? Log and continue (eventual consistency, drift risk)? Probably a `secondary_required: bool` config knob defaulting to false, with metrics on lag.
- **Dedup disagreement.** `insert_event` returns `bool` ("new or not?"). If primary says "new" and secondary says "duplicate" (or vice versa), which wins? Likely return primary's answer and log secondary disagreements — they signal historical drift worth surfacing.
- **Backend-specific query stubs.** Analytics queries (`query_session_synopsis`, `query_tool_journey`, etc.) are not no-ops on Mongo but ARE no-ops on stores that haven't implemented them. Reads should route to whichever backend actually implements the query, not a blanket "primary always." Consider a per-method override.
- **FTS index.** Both backends maintain their own FTS index (`events_fts` virtual table / collection). Fan-out is correct; just confirm it works on conformance.
- **Boot reconciler.** `boot_from_sqlite` (`rs/server/src/state.rs:111`) currently reads from one store to rebuild projections. After dual-sink, it should read from `primary` only — no behavioral change.

**Why now-ish, not now:** the user explicitly deferred this on 2026-05-23 to focus on other work. The intent has been documented so we don't lose it; pick this up when the cost of single-backend brittleness outweighs whatever's next on the list.

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

### Multi-Machine Session Aggregation — SHIPPED (with a known convergence caveat)
Implemented via NATS leaf node architecture over Tailscale. Hub NATS on VPS accepts leaf connections on :7422 with token auth; each machine runs a local leaf NATS that forwards events to the hub. `NatsBus::connect()` supports `nats://TOKEN@host:port` URLs. Dual MCP servers (local + remote) let agents query either instance. See `docs/deploy/distributed.md`, `deploy/nats-hub.conf`, `deploy/nats-leaf.conf`. Integration tests cover solo local → solo+VPS → team hub → team+guests state machine (`rs/tests/test_deployment_states.rs`).

**Correction (arch review 2026-05):** the original claim that "all sessions land on every node (JetStream propagates bidirectionally)" is *not* structurally true — it rides NATS **core interest-propagation**, a timing race that drops cross-leaf events on cold boot (faithful 10-node lab fails 4/4: hub 10/10, slowest node 8–9/10). Convergence is currently guaranteed by the **app-level catch-up anti-entropy** (`024fcc2`, `rs/server/src/catch_up.rs`), not by the transport. The transport-native fix is the federation entry below.

### Federation transport — native JetStream cross-domain sources
**Spec:** `docs/research/jetstream-sources-federation.md`. Replace racy core-propagation with native **JetStream cross-domain sources** ("Idea A" from the arch review — picked over making NATS the system of record, which violates JSONL-as-sovereign-record). The spike (`scripts/spike_jetstream_sources.sh`) proved the mechanics: cross-domain `$JS.hub.API` reachable over a token leafnode, loop-free via a publish-only local stream + source-only mirror, and **per-host subject namespacing is mandatory** (else core propagation cross-pollinates and the aggregate double-counts).

`host` in the subject (`events.{host}.{project}.{session}.main`; `host.rs`/`user.rs` already exist subject-safe) is shipped. The node-level publish switch is shipped too: `publish_sessions = false` routes a node's own observed sessions to a `local.>` stream that federation never sources, so they stay on the machine while the node still receives everyone else's. That is the *only* sharing control — there is no per-session or per-person permission model (the attempt at one was removed as unenforceable; once an event is on the bus every connected node has it). Access is enforced at the network layer instead: Tailscale membership decides who can reach the NATS port, and NATS token/accounts decide who can connect and to which subjects. See `docs/deploy/distributed.md` for the model.

**Plan:** branch-per-phase, **TDD + testcontainers throughout** (red container test first). P1 host-in-subject (done) → P2 JetStream sources (`lab_federation_jetstream_sources_10_nodes`, catch-up OFF, vs catch-up's 12.8s). Keep catch-up as the topology-agnostic backstop (and the only path for non-NATS peers). v0 ship line decided after P2 with convergence numbers in hand.

### Federation cold-ramp 10-node regression inside lab framework
`lab_federation_full_mirror_10_nodes_cold` (standalone) converges in ~4.6 s on the dev machine (32 CPUs, 123 GiB RAM, plenty of headroom). The same code path inside `lab_federation_ramp_cold` — after a successful 5-node iteration + a 10 s settle delay — stalls at `slowest_node=8/10` for the full 120 s timeout. Two missing sessions every time. The hub fan-in is fine (10/10 via core leafnode propagation); the events-mirror is short by a few sessions in exactly one leaf. Hypothesis: a self-registration race where one leaf's `register_self_with_hub` lands *after* peer leaves have already started publishing into their events streams, so the source's `start_sequence` is effectively past those events. Standalone-10 doesn't hit this because the first publish racing the first registration *is* the same event sequence; in the ramp's environment something delays one specific leaf's registration enough that other leaves have already advanced. Worth: instrumenting `run_lab_federation` to dump per-leaf stream state on timeout (which leaf, which missing sessions), then either setting source `DeliverPolicy::All` explicitly or sequencing self-registration before any leaf publishes. Surfaced 2026-05-28 during Phase 2b Step 6; the ramp test ships as-is reporting ceiling=5 honestly until this is resolved.

### Live-streaming MCP server: federation-aware subscribe (follow-up to federation Phase 2b)
The streaming MCP server (`rs/mcp/`) wraps `NatsBus::connect(url)` and subscribes to `events.>` to stream the *fleet* view to a Claude session. Once Phase 2b's cross-domain wrapper lands, the bus exposes federation mode (local `events` + source-only `events-mirror`), and `subscribe`/`replay` read both — but the MCP server still calls the solo constructor. After Phase 2b is green, update `rs/mcp/src/nats_bus.rs` and `rs/mcp/src/bin/open-story-mcp.rs` to pass federation config through (or pick a clean "subscribe to fleet" Bus method) so an MCP-connected agent sees the union, not just local. Touches: `InnerNatsBus::connect` wrapper, the streaming subscription in `open-story-mcp`, and `rs/mcp/tests/nats_smoke.rs` (add a federation smoke that asserts events from a peer leaf land in the MCP stream). Surfaced 2026-05-28 while wiring Phase 2b; deferred to keep the federation commit atomic.

### MCP per-session subscribe wildcard is stale on `feat/federation-host-in-subject`
`rs/mcp/src/nats_bus.rs:42` subscribes to `events.*.{session_id}.>` (4 tokens). On this branch, publishes go to `events.{host}.{project}.{session_id}.main|agent.{id}` (5 tokens — see `rs/core/src/paths.rs:119`), so the wildcard never matches and the MCP `subscribe_session` stream stays silent even though the binary connects to NATS and the session is alive. The file's existing TODO at line 42 predicted exactly this (*"when the cybersecurity spike's ... proposal lands, update this wildcard"*). Fix: widen to `events.*.*.{session_id}.>` (or better, pull subject construction into `open_story_bus` and call a typed helper so MCP subscribe and core publish can't drift again). Add a regression test in `rs/mcp/tests/nats_smoke.rs` that publishes one event with the current subject scheme and asserts the subscriber sees it. Surfaced 2026-05-28 while demoing the live stream against this session — WebSocket broadcaster path was unaffected and worked fine; the bug only bites the MCP stdio subscribe path.

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
K8s manifests (NATS StatefulSet + consumer Deployment + agent sidecars), integration tests via K3s testcontainers, and a Helm chart. K3s testcontainer spike exists in the codebase (`rs/tests/helpers/k8s.rs::K3sCluster`, `test_k8s.rs`). **Tailnet-federation k8s tests** are planned in `docs/research/tailnet-federation/K8S_TEST_PLAN.md`, building on `K3sCluster` + `kube`: Phase 1 is NetworkPolicy allow/deny enforcement guarded by a false-green meta-control; Phases 2–4 add the Tailscale-sidecar identity and two-cluster federation ablations. Motivated by interoperating with an inference-cluster peer. Run on a Linux box (e.g. a1 over SSH) — K3s needs real cgroups; macOS Docker Desktop is unreliable for it.

### Tailnet Federation — graduate from research to product
The `docs/research/tailnet-federation/` spike validated (12/12 controlled experiment on Linux + green Rust test `rs/tests/test_tailnet_federation.rs`) that OpenStory federates over a purpose-built Tailscale tailnet with a tag-based ACL as the trust boundary, and hardened a real ACL-bypass — a NATS leaf falling back to a non-tailnet path, fixed with `leafnodes { advertise }` (now noted in `docs/deploy/distributed.md`). Remaining to productize: fold the tailnet-sidecar + tag-ACL setup into `distributed.md` as a first-class "federate with a friend" quickstart; gate `test_tailnet_federation` in CI (needs a Linux runner with `/dev/net/tun` + `NET_ADMIN`); then the k8s tests above. The hermetic harness (`docs/research/tailnet-federation/harness/run.sh`, runnable on a1 over SSH) is the reference oracle.

### OpenClaw Skill Integration
CLI commands (`sessions`, `summary`, `events`, `install-skill`) for conversational session recall via OpenClaw. Includes SessionSummary reducer, digest format for hourly heartbeat, and portable SKILL.md.

### OpenClaw Watchdog via OpenStory
Cron job or systemd timer on the server that queries the OpenStory API to detect when OpenClaw is stuck — consecutive zero-token error responses, or no successful completion in N minutes. When detected, automatically `docker restart openclaw`. This is the dogfood approach: OpenStory's own data powers the health check instead of generic Docker healthchecks that can't distinguish "running but spinning on rate limits" from "working normally." Could be a simple Python script in `scripts/` querying `http://open-story:3002/api/sessions`.

### One-line installer (`curl | sh`)
An optional convenience wrapper that does `brew tap` + `brew install openstory` + `open-story init` + start-services in a single command, for users who want the fast path. Must stay an *optional* in-repo, reviewable `scripts/install.sh` documented alongside the auditable two-command flow — never the headline (a blind `curl … | sh` contradicts OpenStory's "observe, understand, decide" soul, and piping into `sh` breaks the wizard's interactive stdin). Defer until there's demand; the `brew install` + `open-story init` path already covers first-run setup.

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

### `test_container` / `test_security_container` need a NATS sidecar to run locally
Surfaced 2026-06-11 during the master security audit (PR #75). `cargo test --test test_container` fails 7/15 (and `test_security_container` similarly) in a plain local run, but **not because of any code regression** — the `start_open_story` helper (`rs/tests/helpers/container.rs:93`) boots the `open-story:test` image with the Dockerfile default `CMD` (`serve … --data-dir /data --watch-dir /watch`), which has no `--manage-nats` and no bundled `nats-server`. Master makes NATS a hard boot dependency, so the container logs `NATS unavailable`, exits, and the HTTP health-wait at `container.rs:103` times out. Verified independent of code: the failing path uses a *prebuilt image* (not freshly-compiled code), `git diff` of `test_container.rs` / `container.rs` / `rs/Dockerfile` is empty across the audit branch, and a hand-wired `docker run` of the same fresh image **with** a NATS sidecar serves every endpoint correctly. Fix options: (a) teach `start_open_story` to start a `nats:2 -js` sidecar container on a shared docker network and pass `--nats-url` (mirrors what `red_team_live.py`'s manual boot does), or (b) have the test image boot with `--manage-nats` + a bundled `nats-server` binary so it's self-contained. Either makes the container suites runnable via plain `cargo test` instead of only through the compose/CI harness. Low urgency (the suites' vectors are covered by in-process + live-probe tests), but the silent "looks like a regression" failure mode costs every future auditor time — so it's worth closing. See `docs/security/audit-master-2026-06.md` for the full diagnosis.

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

### E2E coverage for streaming session-record pagination
`fix/lazy-load-pagination` (PR #38) added `streamSessionRecords()` and progressive page-by-page dispatch in `Timeline.tsx`. Unit coverage is solid (9 specs in `ui/tests/lib/session-records-pagination.test.ts` covering cursor walk, ordering, abort, reducer dedup), but no e2e exercises the real React lifecycle: StrictMode double-mount, navigation aborting mid-stream, live `enriched` deltas merging during in-flight pages, or the user-visible "page paints after first round-trip" promise. Blocker today is fixture size — none of `e2e/fixtures/seed-data/*.jsonl` exceeds 500 records (largest is 301 lines), so the cursor walk is never triggered. Work shape: (1) add a programmatic seed generator (or static fixture) producing a 600+ event session; (2) write a Playwright spec that opens it, intercepts `/records` requests, asserts at least one `?before_seq=` follow-up fires, and asserts older records become visible after the stream completes. ~30 min once the fixture exists.

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

## Distribution

### Publish the homebrew-openstory tap (first release)
The formula at [`Formula/openstory.rb`](../Formula/openstory.rb) and the bottle workflow at [`.github/workflows/release-binaries.yml`](../.github/workflows/release-binaries.yml) are in place; what's left is the actual publish dance for v0.1.0:
1. Cut `v0.1.0` tag on master (`git tag -a v0.1.0 -m "v0.1.0" && git push --tags`). Triggers both `release.yml` (Docker image to GHCR) and `release-binaries.yml` (macOS bottles on macos-14 + macos-13).
2. Wait for `release-binaries.yml` to upload `*.bottle.tar.gz` + `*.bottle.json` to the GitHub Release page, and check the workflow's `print-bottle-block` job for the `bottle do` snippet.
3. Create the `OpenStoryArc/homebrew-openstory` repo on GitHub (empty, public).
4. Push a single commit with `Formula/openstory.rb` — same content as in this repo, but with the real sha256 substituted and the `bottle do` block pasted in.
5. Verify on a clean macOS user account: `brew tap OpenStoryArc/openstory && brew install openstory && brew services run openstory && curl -fsS http://localhost:3002/api/sessions` (the service self-manages JetStream NATS; `run` avoids registering it at login).
6. Update `README.md` with the `brew tap` + `brew install` instructions.

### Auto-update tap on tag push
Today the bottle workflow uploads artifacts and prints the `bottle do` block; a human pastes it into the tap repo. Wire a final job that checks out `homebrew-openstory`, regenerates `Formula/openstory.rb` from this repo's copy + the new bottle JSONs, commits with a `Co-Authored-By` line, and pushes. Needs a deploy key or PAT scoped to the tap repo (don't reuse `GITHUB_TOKEN` — it can't push cross-repo).

### Sibling `openstory-mcp` formula
`rs/mcp/` (the streaming MCP server) is intentionally outside the main workspace (`rs/Cargo.toml:2-4`) during incubation. Once it stabilizes, ship as a second formula `openstory-mcp.rb` in the same tap, with `depends_on "openstory"` so users can `brew install openstoryarc/openstory/openstory-mcp` and get both. Defer until the MCP crate joins the workspace.

### Homebrew-core qualification (long-term)
The current formula declares `depends_on "nats-server"` — homebrew-core forbids that pattern (formulas must not require an external service to be useful). To qualify for core, OpenStory needs a no-NATS or embedded-NATS mode. Design notes already exist in [`docs/research/nats-permissions-spike.md`](research/nats-permissions-spike.md). Other gates: stable 1.0, 40+ stars, 30-day notability waiting period. Tap-only is the right home until those are cleared.

---

## UI — follow-ups from the session-visibility loop (branch `feat/ui-session-visibility`)

That loop shipped the D3 activity ribbon, Sessions Overview dashboard (calendar +
facets + shareable URLs), tool-trace duration waterfall, the shared clickable
SessionSummary spine (across Explore/Overview/Story), ⌘K palette with frecency
recents, harness-message untruncation, and a shadcn Skeleton polish pass. Per-
iteration UX+design reviews live in `docs/reports/ui-loop-reviews.md`. The items
below were deliberately deferred because they need a human in the loop (their
failure mode is *visual*, which the logic-only test suite can't catch and the
loop's environment couldn't screenshot).

### Color-token pass — inline hex → CSS variables, enforce one accent
Components hardcode Tokyonight hex (`bg-[#1a1b26]`, `text-[#c0caf5]`, …) while
`ui/src/index.css` already defines the matching CSS variables (`--bg`,
`--bg-surface`, `--accent`, …) that almost nothing references. Migrate the inline
hex to the tokens, then enforce a single primary accent (blue `#7aa2f7`) with the
rest demoted to data-encoding only (session/tool/person colors). Unlocks real
theming and the "one accent" discipline the design reviews kept asking for.
Flagged in five consecutive reviews; do it as one deliberate, visually-verified
pass (~30+ components, high churn, regressions are cosmetic so lean on manual QA
plus the full suite).

### Motion primitive — one shared transition token
The app has no motion: the ⌘K palette hard-appears, the Overview drill-in pops
in, ribbon/trace marks don't ease in. Define one shared 120–160ms
scale/opacity/slide token (Apple's "motion that explains") and apply it
consistently to the palette, drill-in, and viz mark entrances. Verify visually.

### ⌘K palette actions (not just navigation)
The palette only navigates. Extend it to run commands the way GitHub/Linear do —
"copy link to this view", "clear filters", "toggle theme", per-session actions —
surfaced alongside the session/tab results. Pure command registry + the existing
fuzzy ranker; low risk once the action model is defined.

### Subagent visibility within a session
Records already carry `is_sidechain` / `agent_id` / `depth`. The activity ribbon
and tool-trace could render subagent lanes (indented/nested) so a session's
delegated work is legible, not flattened. Genuinely new session-visibility value;
needs a visual pass to get the nesting readable.

### Server-side session label skips harness wrappers
`rs/store/src/projection.rs:302` sets the label to the first user prompt
truncated to 50 chars — for `/loop`-style sessions that's harness plumbing
(`<command-message>…`), so the stored label is noise. The UI now cleans this at
render (`ui/src/lib/harness-message.ts`), but the source-of-truth label is still
lossy (affects API consumers, search, exports). Fix: derive the label from the
first *human* prompt, skipping harness-wrapper content, at ingest.

---

## Done (not tracked here)

Completed work lives in git history. For reference, major completed features include: pattern detection pipeline (5 detectors), SQLite event store, pub/sub via NATS, live timeline, explore view split, subagent enrichment, stateful BFF projection, enriched event envelopes, view model crate, testcontainers E2E, configurable projects dir, syntax highlighting, and open-source licensing cleanup.
