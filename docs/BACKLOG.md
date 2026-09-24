# Backlog

Ideas and future work for Open Story. Each entry describes *what* and *why* in a short paragraph. When work begins, create a branch — the backlog entry is the spec.

---

## Projection: the next five (2026-09-23)

Each item below is where several open entries already point; building it
retires or advances that cluster. Ordered by where the energy is.

### The reading edition, end to end
One Export dialog with a format picker, Kindle first, both editions rendered
from the same `ReelBundle` so the sensitive-content scan and its receipt travel
with every copy; the player wears the paper-and-ink tokens and hides its chrome
during playback; `chart` becomes a visual kind whose data comes from the
analytics endpoints, rendered client-side in the same palette, so a reel carries
a live figure without a script run. Why: the illustrated arc reel proved figures
carry the story, and a reader who wants "the reel, on my Kindle" should not have
to know which of two buttons to press.

**Shipped so far (2026-09-23, `feat/reel-chart-beats-kindle`):** the Export
dialog has the format picker with the Kindle reading edition first; both
formats bake from the same `ReelBundle` (`ui/src/lib/export-kindle.ts`), so
the scan receipt rides in every edition; the bundle carries each figure's
title for headings and alt text. Remaining: the player in paper and ink with
hidden chrome, `chart` as a visual kind fed by the analytics endpoints, and
retiring the server-side report renderer on the Codex branch in favour of the
shared bundle (keep its edition store).

Retires or advances: "Reel export: Kindle reading edition as the default format"; "The reel player wears the reading aesthetic"; "Chart beats from the record"; "Reels v1 follow-ups" (Tighten export scan regexes; Reel size caps at POST; Automated coverage for spotlight-snapshot capture).

### A human edits the reel
v1 is agents-author, humans-replay. Let a person reorder stops, edit lines, and
trim `clipAt` in place; make the Event Spotlight render tool events the way
EventCard does (tool name, path written, command run) with raw JSON one click
away; add a narration voice and rate picker; badge BLUF-compliant reels in the
list; seed an E2E reel fixture so the click-through is tested. Why: a reel is
the first artifact built to be watched by someone else, and the author should
be able to fix the cut without asking an agent.

Retires or advances: "Reels v1 follow-ups" (Reel editing UI; Narration voice/rate controls; ReelMeta could carry opener/closer presence; E2E: seed-reel fixture; Validation: store-error ≠ not-found); "Event Spotlight renders tool events as raw JSON".

### Every session has a Story
When `GET /api/sessions/{id}/patterns?type=turn.sentence` is empty, the Story
view fetches records and runs `extractCycles(records)` client-side so structural
turns render for any records-bearing session — federated, watcher-only, or old.
Then move the cycle to Rust as a `turn.cycle` pattern so the patterns consumer
streams it live. Why: the blank Story has embarrassed a UI tour since July and
blanks a large fraction of federated sessions; the story is the product, and
the fix is a fold over data we already hold.

Retires or advances: "Federated sessions have records but no turn.sentence patterns (blank Story)"; "Story-Rendering Catch-Up for Sessions Without Hooks (Recursion Principle Test, F-2)"; "Eval-Apply Cycle Detector (Rust)"; "Subagent Task Labels — Restore After Cut".

### The mirror reports on itself
`GET /api/health` returns what each layer thinks is true — store backend vs
expected, watcher last-event age, NATS leaf connectivity and `bus.streams`
msgs/bytes against the configured caps, ingestion rate — and a header indicator
turns yellow or red on drift, with the JSON one click away. Fold in the two
ten-line fixes that keep resurfacing: redact the NATS token from startup logs
and send a `{kind: "lagged", skipped: n}` frame instead of swallowing it. Why:
the oldest open pain here is silent state mismatch, including the stream-cap
wedge that three open PRs (#95, #97, #98) are guessing at.

**Research (2026-09-23):** `docs/research/openstory-as-node/2026-09-23-openstory-as-node.md`
frames the instance as an actor-shaped node and names four day-sized experiments
that feed this item: a health probe script, an agent-payload tolerance test for
`agent: "openactor"`, a static audit of subject publishers, and a testcontainers
stream-cap wedge test. The three-tier line (read; derived state; substance held
by a human credential) is the doctrine this item should implement.

**Plan (2026-09-23):** design `docs/superpowers/specs/2026-09-23-node-ops-design.md`,
scoreboard `docs/research/openstory-as-node/REQUIREMENTS.md` (59 requirements
in eight groups), loop prompt `docs/prompts/node-ops-loop.md`. The groups are
the entries under "Infrastructure: the node watches itself" below.

Retires or advances: "Self-reporting `/api/health` endpoint (silent-state-mismatch detector)"; "WebSocket Lagged Notification (WS walk F-1)"; "HOTFIX: Redact NATS token from startup logs"; "Unify the interaction/control seam onto NATS" (the resumable fan-out).

### Notes in the margin
Annotations render where the thing they annotate lives — inline on the turn or
event in Story and Explore, as a badge on the session card, as a margin note on
a reel stop — anchored to an `anchor` (event_id / turn / file / selector) the
interaction schema already captures. Notes carry a relative timestamp and a
legible person-vs-agent issuer, can be edited (`PATCH`), and deletion respects
authorship. Why: glass ink already scopes strokes per context; notes should
follow, so a human and an agent can discuss a session in its margins rather
than in a corner overlay.

Retires or advances: "Show annotations in context, not just the corner overlay"; "Anchor annotations to a target finer than a session"; "Provenance & ownership on delete"; "Surface created_at + issuer richly"; "Annotation threads / replies".

---

## Guiding principle — the UI is a map: every datum drills to its source

Open Story's UI should be an *interface to the session data* — a map you can
navigate down to the source of anything shown. **No visual is a terminal.**
Every displayed datum links down the tree to what produced it: a sunburst wedge
→ its sessions → its events → the raw transcript line; a token count → the
events that generated it; a sentence → its turn → its tool calls; a file name →
the reads/writes that touched it. This is the data-sovereignty twin of the
no-dead-end-truncation rule (you can always reach the full thing) applied to
navigation (you can always reach the source). Acceptance test for every view:
*can I click this element down to its source event/record?* If not, it's a gap.
This principle unifies the object-navigation work (`focus_event`, `route.eventId`
consumption in Explore/Story, click-granular interaction capture), viz drill-in
(sunburst/treemap labels + drill), and clickable stats.


## Unify the interaction/control seam onto NATS (one bus, one source→sink graph)

Shipped: interactions, control and annotations now publish onto the authored
`ui.*` JetStream stream (`1306546`, `2c6a449`, `ef58034`) and the MCP consumes
it natively (`1f0469f`, `3640c5f` `subscribe_ui_state`). The sovereignty
invariant holds by construction: `events.*` is **OBSERVED** (watcher-sourced,
read-only; only the translate step publishes there), `ui.*` / `overlay.*` is
**AUTHORED**, and no code path in the authored namespace may ever write into,
mutate, or masquerade as the observed namespace. Across the fleet the two
domains federate separately.

**What remains — the browser fan-out.** `/ws` (`rs/server/src/ws.rs`) is still
a tokio `broadcast` channel that silently drops for lagged receivers
(`RecvError::Lagged` is only logged). Make the browser fan-out just another
actor relaying NATS → clients, and evaluate NATS→SSE for it: SSE is a one-way
server→browser stream (the UI is a pure sink; its writes go via REST), simpler
than WS (no upgrade handshake, built-in auto-reconnect, HTTP/2 multiplexing),
and a NATS-backed SSE stream can be **resumable** — a reconnecting client sends
`Last-Event-ID`, the server replays from that NATS sequence, so a lagged client
heals instead of losing messages. Per-client filtering (this session/project)
rides the connection URL. Keep WS only if a future feature needs true
bidirectionality.

## MCP discoverability & self-documentation

Shipped: `instructions` in the `initialize` response and `resources/list` +
`resources/read` exposing the agent-in-ui doc (`fd13ddb`;
`rs/mcp/src/protocol.rs`). Remaining:

- **Prompts** (`prompts/list` + `prompts/get`): reusable templates the server
  offers — e.g. "investigate this session", "drive the dashboard to X",
  "follow the user" — so common workflows are one call, not hand-assembled.
  (In flight on PR #118, memory hands.)
- Optional HTTP convenience: a `/api/mcp-docs` (or fold a docs pointer into
  `/health`) for humans/curl — secondary to the protocol-native hooks above,
  which is where an agent actually looks.

Goal: the MCP teaches itself. A fresh agent reads `instructions`, sees the tool
map + when-to-use, and can pull deeper docs as resources — no source-diving.

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

### DashMap discipline guardrail
Six `StoreState` fields are now `Arc<DashMap>`. The concurrency model requires: never hold a `RefMut` from `.entry()` across an `.await` or across a second `.get()` / `.entry()` on the same map (shard-lock deadlock). Scope guards tightly. Document this in `CLAUDE.md` principles so new contributors don't have to learn it from a stuck test. Consider a lightweight runtime assertion in debug builds that panics on held guards across await points.

### Retire `ingest_events` fully (test migration)
`ingest_events` is production-dead after Phase 1.5 but still pub-exported and used by ~15 integration test files (70 call sites) that do `state.write() + ingest_events(&mut s, ...)`. The tests work because `&mut AppState` auto-derefs to `&AppState` and the inline `!bus.is_active()` demo-mode guard still persists events when the test harness uses `NoopBus`. Migrate all test call sites to `TestActors::drive_batch` (which runs the full four-actor pipeline), delete the demo-mode guard, delete `ingest_events` + `IngestResult` from `rs/server/src/ingest.rs`, remove the re-exports in `rs/src/server/mod.rs`. Pure housekeeping — no behavior change in production.

## Observability

### Cost & Token Tracking
Token usage (input, output, cache reads/writes) per session is surfaced in the
UI (`e79a06d`, `ui/src/components/viz/TokenReport.tsx`). Remaining: estimated
cost based on model pricing, token timelines, and cache hit ratios — the
financial visibility into agent work. Token usage analytics scripts exist
(`scripts/token_usage.py`); this is about surfacing cost in the UI.

### Per-Call Model-Aware Cost Estimation
The model string (`claude-opus-4-6`, `claude-haiku-4-5-20251001`, etc.) is already present in the raw event payload at `data.raw.message.model`. Today `token_usage.py` and the MCP `token_usage` tool apply a single flat pricing tier across all sessions — the user has to guess which model they were running. The prototype exists (`scripts/cost_by_model.py`). Production path: update `token_usage.py` to default to per-call model extraction (with `--model` as an override), update the MCP `token_usage` and `daily_token_usage` tools to return model-aware costs, and add a `model` column to the Rust `token_usage` and `daily_token_usage` analytics queries.

### Anomaly Detection & Behavioral Alerts
Rule-based detection for unusual patterns: destructive git commands, high error rates, tool loops, token spikes. Rules are pure functions evaluated during event ingestion, surfacing alerts without interfering with agent execution. Builds on the existing pattern detection pipeline.

### Stream Architecture: WebSocket protocol redesign (what remains)
Of the original five steps, the broadcast consumer is decomposed onto its own
NATS subscription (PR #29, `rs/server/src/consumers/broadcast.rs`) and Explore
was rebuilt on REST (PR #96). Still open:

- **Redesign the WebSocket protocol.** Today's `initial_state` / `enriched`
  shape (`rs/server/src/ws.rs`) is still there. New shape: `kind: "event"` and
  `kind: "pattern"` — minimal, pure, streaming. No preload payload by default;
  if a client wants recent history it asks via REST.
- **Delete `ingest_events`** — tracked as "Retire `ingest_events` fully".
- **Delete `state.store.detected_patterns`** once no view depends on
  `initial_state` or `BroadcastMessage::Enriched`.

**Validation criterion:** the Live tab on a fresh page reload starts with
**zero records**, and fills only with events that arrive after connect. The
Story tab does the same with patterns. No view should depend on
`initial_state` or `BroadcastMessage::Enriched` after this lands.

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

### `ToolOutcome` for pi-mono (Domain Events follow-up)
`ToolOutcome` (`FileCreated`, `FileModified`, `FileRead`, `CommandExecuted`,
`SearchPerformed`, `SubAgentSpawned`) is implemented in `translate.rs` for
Claude Code and shows as domain fact badges on every Story card.
`translate_pi.rs` still emits none, so pi-mono Story cards carry no domain
facts.

### Agent Behavior Patterns
Cross-session analytics revealing longitudinal trends: tool preferences, session duration, token consumption over time, error rates by task type. Answers questions like "I spend 60% of tokens on test-writing" by aggregating over persisted event data.

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

### Session Replay & Playback
Chronological playback of session events with transport controls (play/pause/speed) and a visual timeline showing event density. Works client-side with persisted event data — lets you experience a session's narrative flow.

### Session Comparison
Side-by-side comparison of two sessions highlighting deltas in duration, token usage, tool distribution, files touched, and error counts. Enables learning from repeated tasks and calibrating agent directives.

## Export & Portability

### Export Formats
Client-side export of sessions to Markdown transcripts, JSON archives, and CSV summaries with session metadata headers. User data should be useful without Open Story.

### Offline & Local-First Mode
Load persisted JSONL files directly into the UI without a server connection for air-gapped review, CI artifact analysis, and portable data sharing. Reuses all existing read-only views by swapping the data source from WebSocket to file parsing.

### CSV Export for APIs
Server-side `?format=csv` query parameter across analytics endpoints (sessions, token usage, daily trends, project pulse, tool journeys, file impact). Enables spreadsheet analysis and data pipeline integration.

---

## UI

### Explore Tree View
Render the causal event tree (parent_uuid relationships) as a collapsible, interactive tree within Explore, showing actual session structure rather than a flat list.

### Timeline Rendering Performance
Fix virtualizer layout shifts when rows expand to show detail inline. Expanded rows should push subsequent rows down without overlap.

### Live Pattern Notifications
Toast notifications when patterns are detected (test cycles, error recovery, git workflows), with click-through to highlight relevant events and optional timeline overlay showing pattern temporal span.

### Mermaid Diagrams
Transform structured data (tool journey, token usage, session flow) into visual Mermaid diagrams (flowcharts, pie charts, sequence diagrams), with optional server-side rendering.

## Infrastructure

### MCP performance testing — spike
We have a criterion bench scaffold at `rs/mcp/benches/` (added in commit `perf(mcp): criterion benches…`) covering per-tool latency (`tools.rs` — `tools/list`, `list_sessions`, `session_synopsis`, `search` at 10/100/1000-session sizes) and streaming throughput (`streaming.rs` — `subscribe_session` batches/sec through the stdio pipe). Run with `cargo bench -p open-story-mcp` — the baseline numbers from when it landed are in that commit message. Right now this is a measurement scaffold only: nothing runs in CI, no committed baseline, no regression gate. The spike is: figure out whether MCP performance is load-bearing enough to deserve a regression gate, and if so which form. Options sketched out in the design conversation: (a) save criterion baselines into the repo and document `cargo bench -- --baseline main` as a manual pre-merge check; (b) a CI workflow that runs benches and fails on >X% regression — known noisy on shared GitHub runners, so threshold tuning is the open problem; (c) a tiny `tests/perf.rs` that runs the same drivers with hard-coded latency ceilings — cheaper but cruder. Threads to pull: what's the actual agent workload (subscribe-heavy? query-heavy?), what's the cheapest meaningful regression signal, and whether HTML reports in `rs/target/criterion/` are worth committing as artifacts. Surfaced 2026-05-21 alongside the MCP-rewrite PR — keep the scaffold runnable-on-demand until a real perf problem makes the gate worth the noise tax.

### MCP streaming tools over WebSocket (drop the NATS dependency)
The query tools now read through the REST API (`HttpEventStore`, commit `feat(mcp): read query tools through REST API`), so the MCP no longer opens SQLite relative to its launch directory. The two streaming tools — `subscribe_session` and `subscribe_tokens` — still subscribe over NATS via `crate::subscription::Subscribe` (`NatsBus`), so the binary keeps a hard NATS dependency (`OPENSTORY_NATS_URL`, default `nats://localhost:4222`) and bails at boot if NATS is unreachable. NATS is a fixed URL, not cwd-relative, so it isn't the directory-fragility bug — but it's a second piece of infrastructure the MCP must reach. Phase 2: migrate both streaming tools to the server's `/ws` WebSocket (`rs/server/src/ws.rs`) so the MCP needs only `OPENSTORY_API_URL`. Design notes: `/ws` currently opens with an `initial_state` frame (patterns + session_labels for sessions within `watch_backfill_hours`) and then broadcasts `BroadcastMessage`s for *all* sessions; the client ignores inbound messages. To replace NATS, the streaming layer needs a per-session subscription (filter the broadcast to one `session_id`, or add a subscribe frame the server honors) and a `Subscribe` impl backed by a WS client (`tokio-tungstenite` / `reqwest`'s upgrade) that adapts the frame shape to the existing `StreamEvent` the stdio handler emits as `notifications/openstory/stream` + `notifications/openstory/tokens`. Threads to pull: does `/ws` need a new "subscribe to session X" control frame (today it's broadcast-to-all), and can `subscribe_tokens`' running tally be computed client-side from the same per-message events. Once done, the binary's NATS env and boot check come out, and `Server` no longer carries a NATS `Subscribe`. Surfaced 2026-06-13 alongside the query-decoupling cutover.

### CI/CD pipeline
We have GitHub Actions running `cargo test` + UI tests on PRs, but no real release / deploy pipeline. Every Hetzner redeploy is currently a manual `scripts/deploy/deploy.sh` run, and there's no automated path that exercises the production Docker images end-to-end before they ship. What we want: (1) tagged releases that build the `open-story:test`, `openclaw-mcp:latest`, and `open-story-mcp` binary artifacts; (2) on each PR, a workflow that does `docker build` for the openclaw image so we catch Dockerfile drift before merge (currently nothing builds these in CI — they break silently until a deploy); (3) a `deploy` workflow gated on a tag or manual dispatch that ships to Hetzner via SSH, with a rollback path. Bonus: testcontainer-based smoke that asserts the freshly-built openclaw image can connect to a NATS + Mongo container and serve at least one tool call. Existing pieces to compose: `.github/workflows/test.yml` (PR gate), `Dockerfile` (open-story:test), `Dockerfile.openclaw` (openclaw-mcp image), `scripts/deploy/{smoke,deploy}.sh`. Decide between GitHub Actions + GHCR vs. an external CI; prefer GHA for simplicity and visibility next to the PR workflow. Surfaced 2026-05-19 alongside the MCP-rewrite cutover, which exposed that the Docker images are part of the change surface but nothing in CI exercises them.

### Pi-Mono Assistant Message Rendering
The dashboard renders some pi-mono `assistant_message` events as blank cards. The data is present in the API (verified via `curl`), but the UI's content block extraction doesn't handle all pi-mono response formats correctly. The pi-mono format uses `content: [{text: "..."}]` arrays where Claude Code uses plain strings. The views layer branches on `agent` field but some assistant message structures still fall through. Fix in the views crate (`from_cloud_event.rs`) and/or the UI's `EventCard` component.

### Pi-Mono Skipped Entry Types
The pi-mono translator (`translate_pi.rs`) skips 6 entry types: `thinking_level_change`, `branch_summary`, `label`, `custom`, `custom_message`, `session_info`. Real sessions produce `thinking_level_change` frequently. The others are defined in pi-mono's type system but rarely seen. Add match arms to translate these into `system.*` subtypes. The views layer's existing `system.*` catch-all handles them as SystemEvent records, so no views changes needed.

### Pi-Mono Validation Script
Automated format gap detection script (`scripts/validate_openclaw.py`) that scans session directories, translates all JSONL files, and reports subtype distribution, tool name distribution, lines that produced 0 events (format gaps), and parse errors. Reuses the pattern from `scripts/translate_pi_mono.py`. Run against `~/.pi/agent/sessions/` or `~/.openclaw/agents/` to find format gaps before they become bugs.

### Multi-Agent UI — Agent Filter
Cross-agent analytics landed on the Canvas (agent×project matrix `b806864`,
per-agent duration beeswarm `1468cf7`, per-agent event volume `5b5acc1`).
Remaining: an agent-platform filter (`"claude-code"`, `"pi-mono"`, `"codex"`,
`"grok"`) on the dashboard sidebar so the session list can be narrowed by which
coding agent produced it.

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

### Federation cold-ramp 10-node regression inside lab framework
`lab_federation_full_mirror_10_nodes_cold` (standalone) converges in ~4.6 s on the dev machine (32 CPUs, 123 GiB RAM, plenty of headroom). The same code path inside `lab_federation_ramp_cold` — after a successful 5-node iteration + a 10 s settle delay — stalls at `slowest_node=8/10` for the full 120 s timeout. Two missing sessions every time. The hub fan-in is fine (10/10 via core leafnode propagation); the events-mirror is short by a few sessions in exactly one leaf. Hypothesis: a self-registration race where one leaf's `register_self_with_hub` lands *after* peer leaves have already started publishing into their events streams, so the source's `start_sequence` is effectively past those events. Standalone-10 doesn't hit this because the first publish racing the first registration *is* the same event sequence; in the ramp's environment something delays one specific leaf's registration enough that other leaves have already advanced. Worth: instrumenting `run_lab_federation` to dump per-leaf stream state on timeout (which leaf, which missing sessions), then either setting source `DeliverPolicy::All` explicitly or sequencing self-registration before any leaf publishes. Surfaced 2026-05-28 during Phase 2b Step 6; the ramp test ships as-is reporting ceiling=5 honestly until this is resolved.

### Live-streaming MCP server: federation-aware subscribe (follow-up to federation Phase 2b)
The streaming MCP server (`rs/mcp/`) wraps `NatsBus::connect(url)` and subscribes to `events.>` to stream the *fleet* view to a Claude session. Once Phase 2b's cross-domain wrapper lands, the bus exposes federation mode (local `events` + source-only `events-mirror`), and `subscribe`/`replay` read both — but the MCP server still calls the solo constructor. After Phase 2b is green, update `rs/mcp/src/nats_bus.rs` and `rs/mcp/src/bin/open-story-mcp.rs` to pass federation config through (or pick a clean "subscribe to fleet" Bus method) so an MCP-connected agent sees the union, not just local. Touches: `InnerNatsBus::connect` wrapper, the streaming subscription in `open-story-mcp`, and `rs/mcp/tests/nats_smoke.rs` (add a federation smoke that asserts events from a peer leaf land in the MCP stream). Surfaced 2026-05-28 while wiring Phase 2b; deferred to keep the federation commit atomic.

### Distributed Deployment Security Hardening
With NATS leaf node streaming, every machine gets a full copy of all team data (sessions, prompts, file contents, tool outputs). This is the correct sovereignty behavior but raises security concerns for team deployments. Items to address:

**NATS accounts for team partitioning.** Today all leaf nodes share a single NATS account — everyone sees everything. NATS accounts would let each team member publish to their own subject namespace and selectively subscribe to others. This enables the "Team Partitioned" deployment state where alice sees only her sessions locally unless she explicitly subscribes to bob's. Requires NATS account configuration on the hub and per-user credentials on each leaf.

**Credential files instead of token-in-URL.** The NATS token currently appears in the URL (`nats://TOKEN@host:port`), which shows up in process listings and Docker inspect. NATS supports credential files (`.creds`) that keep secrets out of command-line args and environment variables. Update `NatsBus::connect()` to accept a `--nats-creds` path. (Log-output leakage is covered separately as a hotfix — see "HOTFIX: Redact NATS token from startup logs" above.)

**SQLCipher for local stores.** Every machine's SQLite database contains all team sessions in plaintext. The `db_key` config field already exists but isn't exercised in the distributed deployment. Document and test SQLCipher with the leaf node setup so stolen laptops don't leak team data.

**API auth on the hub dashboard.** The VPS hub serves the common dashboard. Without `OPEN_STORY_API_TOKEN`, anyone on the Tailscale network can browse all sessions. Document setting the token and update the Caddy config to pass auth headers.

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

## Quality

### Eval-Apply Data Quality Hardening (recurring)
Regular exercise: run `scripts/analyze_turn_shapes.py --all` against live sessions to map the problem space, update probability-class test fixtures (`rs/tests/fixtures/turn_probability_classes.json`), and add assertions for any new edge cases discovered. The distribution of real event sequences is the ground truth — the detector must handle what agents actually produce, not what we imagine they produce. Key metrics to track: turns/sentences ratio (should be 1.0), is_error capture rate (should match raw data), turn number continuity (no gaps), env_delta accuracy. Current known gaps: 7 session mismatches between turns and sentences, subagent sessions produce flushed turns that may lack enough content for meaningful sentences.

### Eval-Apply Scope Open/Close Imbalance
Sessions show a ~4× ratio of `eval_apply.scope_open` to `eval_apply.scope_close` patterns. Example: session `06907d46` had 2754 opens vs 721 closes. Two candidate causes: (1) the detector is missing close events in some compound-procedure shapes, (2) subagent flushes (`SubAgentSpawned` outcomes) close scopes implicitly without emitting `scope_close`. Either way scopes should balance — the imbalance breaks any consumer that tries to use scope nesting to reconstruct call hierarchies. Fix: add detector instrumentation/assertions that every `scope_open` eventually emits a `scope_close` (or a typed flush event), then audit which paths drop one. See `docs/research/sessions/06907d46-feat-story-tab-data.md` for the original observation.

### Remove Orphaned Semantic Crate
`rs/semantic/` exists on disk with its own `Cargo.toml` (`open-story-semantic`, with feature flags for Qdrant + ONNX), but it's **not** a workspace member in `rs/Cargo.toml` and no other crate depends on it. It's vestigial Qdrant-based semantic search code from before SQLite FTS5 replaced it. The replacement is real and working: `rs/store/src/sqlite_store.rs` has an `events_fts` virtual table (line 146), an `index_fts()` function, and a `search_fts()` function that powers `GET /api/search`. The `/api/search` endpoint already routes through FTS5, not Qdrant. Action: `git rm -r rs/semantic/`, drop the `qdrant_url` / `embedding_model_path` / `semantic_enabled` fields from `Config`, remove any documentation references that still mention semantic search via Qdrant. Surfaced by `scripts/check_docs.py` — the validator caught that 4 docs claimed 9 crates while the workspace had 8 because the orphan was on disk but not in the build.

### Turn Vocabulary Collision
Two scripts disagree on what "turn" means: `sessionstory.py` counts `system.turn.complete` events (true model turns, e.g., 63 for session `06907d46`), while `analyze_event_groups.py` counts user-prompt windows (e.g., 155 for the same session). Both are correct for their question but the shared label is confusing — a reader of one script's output and the other's will get incompatible numbers. Resolution: rename `analyze_event_groups.py`'s "Turn N" output to "Window N" or "Prompt N", and add a short note to both scripts' docstrings clarifying the distinction. Optional: add a `--turn-mode={model,prompt}` flag where it makes sense.

### UI Battle-Hardening
Performance and chaos testing: synthetic event firehose (throughput, latency, memory), render fidelity under load, interactive chaos (click storm, filter switching), DPI/viewport matrix, 8-hour soak tests.

### E2E coverage for streaming session-record pagination
`fix/lazy-load-pagination` (PR #38) added `streamSessionRecords()` and progressive page-by-page dispatch in `Timeline.tsx`. Unit coverage is solid (9 specs in `ui/tests/lib/session-records-pagination.test.ts` covering cursor walk, ordering, abort, reducer dedup), but no e2e exercises the real React lifecycle: StrictMode double-mount, navigation aborting mid-stream, live `enriched` deltas merging during in-flight pages, or the user-visible "page paints after first round-trip" promise. Blocker today is fixture size — none of `e2e/fixtures/seed-data/*.jsonl` exceeds 500 records (largest is 301 lines), so the cursor walk is never triggered. Work shape: (1) add a programmatic seed generator (or static fixture) producing a 600+ event session; (2) write a Playwright spec that opens it, intercepts `/records` requests, asserts at least one `?before_seq=` follow-up fires, and asserts older records become visible after the stream completes. ~30 min once the fixture exists.

### Maintenance Script
Create `just check` command verifying project health: tests pass, Docker images current, dependencies updated, lint clean, E2E fixtures present, git state clean.

### Performance Bottleneck Fixes
Chunked backfill (`29993c7`, `BATCH_CHUNK_SIZE` in `rs/src/watcher.rs`) and the
bounded LRU projection cache (`3a7a9b6`) shipped. Remaining: diagnose and fix
the 20KB payload cliff.

### Multi-Container Load Test
Docker Compose setup simulating many concurrent agents posting to a single Open Story instance. Measure SQLite contention, NATS throughput, WebSocket broadcast latency, and find the concurrent session ceiling.

### Testcontainer Improvements
Fix container test infrastructure: shared container pattern, silent fixture mtime failures, log capture on failure. Add comprehensive endpoint sweep, WebSocket testing, error path coverage.

### Tool Result Syntax Highlighting (T1 from architecture audit)
`ToolResultDetail` in `ui/src/components/RecordDetail.tsx:252` renders Read tool output as `<CodeBlock>{output}</CodeBlock>` with no language/path/toolName props, so `detectLanguage` falls through to `"text"` and rust/python/toml files display uncolored. The paired ToolCall carries the file path via `call_id` — fix is UI-side: parent component already has the ViewRecord list, look up the paired ToolCall and pass `filePath` + `toolName` down to `ToolResultDetail` → `CodeBlock`. Also wire `strip-line-numbers.ts` into this path (pi-mono bakes line numbers into Read output; they interfere with highlighting). Write UI unit test first — expect `language="rust"` when a paired ToolCall has `.rs` input. See `docs/research/architecture-audit/T1_SYNTAX_HIGHLIGHTING.md` for full recon.

### NATS Subject Sanitization (T3 from architecture audit)
`rs/core/src/paths.rs:38` `nats_subject_from_path()` composes subjects via raw string interpolation of project and session names. Path segments containing `.`, ` `, `*`, or `>` flow into the subject unchanged — dots create extra tokens that break `events.{project}.>` hierarchical subscriptions, spaces produce NATS-invalid subjects that fail at publish, and wildcard characters shadow subscription matching. Not hit in practice today (Claude Code / pi-mono default dirs use UUIDs) but a latent footgun. Fix: lightweight sanitizer that replaces the four problem characters with `_` and logs a warning when rewriting. See `docs/research/architecture-audit/T3_NATS_SUBJECT_ALIGNMENT.md` for three design options (sanitize / percent-encode / hash-prefix) and the recommendation. L1 characterization tests are already in place at `paths.rs` `subject_*` tests — they'll catch any divergence when the sanitizer lands.

### Promote Agent Payload Round-Trip Tests into Conformance Suite (T6 from architecture audit)
Three inline tests in `rs/store/src/sqlite_store.rs` (`t6_pi_mono_agent_payload_round_trips`, `t6_claude_code_agent_payload_round_trips`, `t6_hermes_agent_payload_round_trips`) cover AgentPayload variant + typed-field round-trip for SQLite. Move them (with a backend-agnostic builder helper) into `rs/store/tests/event_store_conformance.rs` so MongoStore inherits the same guarantees. Mongo uses BSON which has real type-width quirks (i32 vs i64, datetime coercion) that a blob-TEXT SQLite pass can hide — this is the natural place to catch them. Low risk; one builder refactor.

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

### Story-Rendering Catch-Up for Sessions Without Hooks (Recursion Principle Test, F-2)
~40 historical claude-code sessions in the local instance have ZERO `system.turn.complete` events because they were ingested via the watcher path without the Stop hook configured. They have full event history but no turn boundaries → no sentences. New sessions with hooks work fine. Fix shapes (any of): (1) infer turn boundaries from event clustering on watcher-only sessions; (2) document hook setup in onboarding so this doesn't keep happening; (3) backfill turn.complete events on a re-ingest pass. Surfaced by the recursion test.

### CI Testcontainers Spike
Investigate what's needed to run Docker-based testcontainer tests (compose tests, container integration tests) in GitHub Actions CI. Currently skipped because CI runners lack the local `open-story:test` image and Docker setup. Spike should cover: GitHub Actions Docker service containers vs Docker-in-Docker, building the test image in CI (caching strategies for the Rust build), NATS sidecar setup, and whether the compose tests can run within the free-tier minute budget. Goal is a concrete proposal, not implementation.

## Distribution

### Auto-update tap on tag push
Today the bottle workflow uploads artifacts and prints the `bottle do` block; a human pastes it into the tap repo. Wire a final job that checks out `homebrew-openstory`, regenerates `Formula/openstory.rb` from this repo's copy + the new bottle JSONs, commits with a `Co-Authored-By` line, and pushes. Needs a deploy key or PAT scoped to the tap repo (don't reuse `GITHUB_TOKEN` — it can't push cross-repo).

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

## Publish reels across the fleet

Once Reels ship (see `docs/superpowers/specs/2026-08-04-reels-design.md`), let
one person *publish* a reel so a teammate can view it: Katie publishes the cut
she made of her loop-engineering week; it appears in Max's Reels tab, marked
with her principal. Why: reels are the first artifact in the product built to
be *watched by someone else* — sharing is their natural completion, and it
turns session history into team communication. Design sketch (preserve): reels
are already portable JSON files referencing events by id; publishing = a
`reel.published` CloudEvent, receiver lands the file in its own
`data_dir/reels/` tagged with the author principal. Playback on the receiving
side degrades gracefully when referenced events aren't in the local store
(caption-only stops, or fetch-on-demand from the hub — decide then). "Send" is
deliberately not the verb: nothing is pushed at a person; it's published to
the fleet and appears in the mirror.

**Taxonomy sketch (2026-08-06 design session).** Reels are *authored
artifacts*, not observed history — they must NOT ride `events.>` (that stream
is the mirror of what agents did; publishing annotations onto it pollutes the
record). The precedent is sentences: derived/authored things get their own
stream (`patterns` carries PatternEvents with `pattern_type: turn.sentence`,
durable, bus-propagated, persisted to their own table). Reels follow the same
shape one level up:

- **Event:** `type: io.arc.event`, subtype `reel.published`, data = the full
  reel JSON (small, pure text; cap stops/line/title lengths at POST — see the
  final-review note on unbounded writes) + `principal_id` of the author.
  Re-publish of the same reel id = overwrite (id is the idempotency key,
  matching save semantics).
- **Subject:** a new authored-artifact family, e.g.
  `artifacts.{principal}.reel.{reel_id}` — sibling to the "Unify the
  interaction/control seam onto NATS" entry's `ui.{principal}.*` proposal;
  same authored-vs-observed boundary, one bus, one set of sinks.
- **Stream:** durable JetStream (like `events`/`patterns`, NOT the
  interest-based `changes`) so machines that are offline when Katie publishes
  catch up on reconnect — late joiners replay, nothing is lost.
- **Federation:** TODAY only `events.>` traverses the leaf/hub link — ui/
  authored artifacts do not replicate at all. The leaf and hub configs must
  export/import `artifacts.>` too; that change lands in the openstory-deploy
  repo (box-only edits get clobbered by deploy.sh).
- **Receiver:** a small artifacts consumer (or the persist consumer) on each
  node subscribes `artifacts.>`, writes the reel file into local
  `data_dir/reels/`, tags author principal. It must NOT gate on local event
  validation (Katie's store won't have Max's events) — mark unresolved stops
  instead; the Reels tab shows "published by <principal>" and plays
  caption-only where the spotlight can't resolve. The broadcast consumer
  relays the same event so the Reels tab updates live.
- Publishing stays explicit: a reel saved locally is local; `publish_reel`
  (API + MCP verb) is the deliberate act that puts it on the bus.

---

## Reel export: Kindle reading edition as the default format

Unify the two export paths (the self-contained HTML from PR #112 and the Kindle
reading edition on `codex/kindle-reel-reports`) behind one Export dialog with a
format picker, Kindle first. Both formats render the same `ReelBundle`, so the
sensitive-content scan and its receipt travel with every edition. Editions are
durable snapshots under `reels/reports/`; delivery is download in v1. Why: a
reader who wants "the reel, on my Kindle" should not have to know which of two
buttons to press, and what leaves the machine must be checked whichever shape it
takes. Design: `docs/superpowers/specs/2026-09-23-reel-export-kindle-default-design.md`.

## The reel player wears the reading aesthetic

Paper and ink tokens, a book serif for captions and narration, chrome that hides
during playback with a thin footer (stop N of M left, percent right), and figure
beats framed as the Kindle edition frames them. Why: the edition should look like
the reel and the reel like the edition; this is the first surface to adopt the
aesthetic before the whole-UI question is decided. Research:
`docs/research/kindle-reading-aesthetic.md` (eight decisions, sourced).

## Chart beats from the record

`scripts/arc_figures.py` renders paper-and-ink PNG figures (commits per week,
tokens per day, write-surface timeline, open PRs by age) and
`scripts/post_reel.py` posts a reel spec with `{"figure": name}` image stops
resolved from its manifest. Promote this into the product: a `chart` visual kind
whose data comes from the analytics endpoints and is rendered client-side in the
same palette, so a reel can carry a live figure without a script run. Why: the
first illustrated arc reel (2026-09-23) proved figures carry the story better
than a fourth spotlight; a figure that is a data URL is portable but frozen.

## Infrastructure: the node watches itself

Eight groups from the 2026-09-23 audit (`docs/research/openstory-as-node/`).
Each is a requirement group in `REQUIREMENTS.md`; entries here say what and
why, the scoreboard says how it is tested.

### Structured logging with a log ring (group L)

Adopt `tracing` with a JSON-lines formatter behind `log_format`, an `event`
name and `actor` on every line, `session_id` and `subject` where they exist,
the managed NATS child's output captured to a rotated file instead of null,
replay progress logged, and an in-process ring served at `GET /api/logs`. Why:
today everything is a bare print with no level or session, the NATS child is
silent by construction, and a replay that took eleven minutes showed nothing.
An agent watching the node has nothing to read.

### No swallowed errors, supervised consumers (group E)

A static audit fails the build on `let _ =` over a fallible persist, publish,
append, or index; each failure logs and counts. A supervisor restarts a
consumer that exits, with backoff, and exposes restart counts. Watcher publish
failures log subject and error (fifteen Grok failures on a clean boot today
are unexplained). Translate rejections count by reason. The NATS child's death
is noticed within seconds. Why: consumers die silently and persist discards
failed writes; the hub crash loop in July was invisible for the same reason.

### A health endpoint that can say no (group H)

`/api/health` reports boot phase with replay progress and returns 503 until
serving; real bus connection; per-stream bytes against caps; per-consumer
alive, restarts, lag; leaf configured and connected; per-watcher age and
failures; version, sha, build time, store size, RSS. A header dot in the UI.
Why: `bus.connected` is true forever because `NatsBus` never overrides
`is_active`; the July stream-cap wedge that three PRs guess at is a number
the node can read from its own NATS on `:8222` and does not.

### Presence: the node's health as a fact on the bus (group P)

A `presence.{host}.{principal}` CloudEvent every fifteen seconds with the
health payload, stored in its own table, exported across the leaf link, read
by the fleet tab and by `GET /api/fleet/presence` with staleness. Why: one
signal for the hub, the fleet, the dashboard, and an ops agent; and the
substrate DORA is measured on.

### Telemetry without a vendor (group O)

Metrics on by default at `/metrics` (events by agent, consumer lag, stream
bytes, restarts, publish failures); optional OTLP export of the same plus a
sampled span per event; one "Node" Grafana dashboard replacing the March
ones; PR #46 closed in favour. Why: the observe stack has one commit from
March and nobody has looked since; OTel is the right export, not the right
logger.

### Ops hands on the MCP, tiers 0 and 1 only (group M)

`node_health`, `node_logs`, `node_streams`, `fleet_presence`,
`subscribe_health` (read); `node_reproject`, `node_verify`, `node_catch_up`,
`node_prune` (derived state, author stamped, on `ops.proposal.>` then
`ops.command.>`, refused while not serving). Tier 2 (restart, resize, rotate)
is never on the MCP; the agent files a proposal with evidence and a person
acts. A test asserts the MCP publishes only `ops.proposal.>` and `ui.>`. Why:
this is "monitor and drive" within the soul; an agent can watch, diagnose,
fix what is derived, and propose the rest.

### The Kubernetes shape: one pod, one node, one principal (group K)

A kustomize base with the production image plus a NATS leaf sidecar, PVCs for
store and JetStream, liveness on `/health`, readiness on `/api/health`, a
startup probe sized for cold replay, JSON logs to stdout, a manifest check
that refuses `replicas > 1`, an optional ops-agent pod with no cluster
credential, `os-loop-` namespaces on a1 for experiments, and a testcontainers
stream-cap wedge test. Why: horizontal scale is by node, not by replicating a
node's ingestion; the shape must say so in YAML.

### DORA from the node's own record (group D)

Every signal carries the git sha. `scripts/dora.py` computes deployment
frequency, lead time, change failure rate, and time to restore from presence
and git; the health probe is the deploy gate with rollback on a critical
after the startup window; rollback is one documented line per host shape;
four tiles on the Admin tab. Why: the four keys are the standard, and this
node already holds the data to compute them honestly.

## Done (not tracked here)

Completed work lives in git history. For reference, major completed features include: pattern detection pipeline (5 detectors), SQLite event store, pub/sub via NATS, live timeline, explore view split, subagent enrichment, stateful BFF projection, enriched event envelopes, view model crate, testcontainers E2E, configurable projects dir, syntax highlighting, and open-source licensing cleanup.

### Event Spotlight renders tool events as raw JSON
Dogfooding reels surfaced it: spotlighting a tool_call/tool_result projects
the raw payload JSON full-screen — honest, but unreadable as a story beat
(a FileCreated result is a wall of braces). The spotlight is a projector
surface; it should render tool events the way EventCard does — tool name +
the human-salient fields (path written, command run, output text) — while
keeping raw JSON one click away (no dead ends). Reuse the views-layer/
EventCard extraction rather than inventing a second renderer. Interim
mitigation lives in the reel skill (prefer prose events, clipAt).

### Reels v1 follow-ups (from the 2026-08-07 dogfooding session)
The first iteration shipped: reel format (opener/stops/closer), Reels tab
with a spotlight player (BLUF opener card, cinema captions, back/jump/
segmented progress, TTS narration), MCP verbs, /openstory:reel skill with
narrative shapes (pyramid, ABT, story spine, kishōtenketsu, sparkline) and
the BLUF rule. Next, in rough priority order:
- **Diagram beats in the pen's hand.** Diagram stops now use the theme palette,
  but they still read as wireframes. Render them in the agent pen's language —
  hand-drawn box strokes (RDP-simplified paths like the portrait recipes),
  single ink + accent, Georgia labels — so agent diagrams look drawn by the
  same pen that annotates them. The ink recipes from feat/agent-pen
  (draw-trace.ts, draw-portrait.ts) are the starting material.
- **Reel editing UI.** v1 is agents-author/humans-replay; a human should be
  able to reorder stops, edit lines, and trim clipAt in place.
- **Reel size caps at POST.** stops count / line / title length — the store
  writes unbounded client JSON today (final-review recommendation).
- **Validation: store-error ≠ not-found.** session_events read errors
  currently classify stops as invented (422); surface a 500-family error
  instead so agents aren't told their real event is fake (deferred T2
  minor).
- **E2E: seed-reel fixture.** click reel → opener → spotlight advances →
  title card, per the spec's deferred test (needs a seed fixture in
  e2e/fixtures/seed-data).
- **Narration voice/rate controls.** Web Speech voice picker + speed,
  per-user preference.
- **ReelMeta could carry opener/closer presence** so the list view can badge
  BLUF-compliant reels.
- **Video export — second ReelBundle consumer.** The HTML export bakes a
  versioned ReelBundle (schema: ui/src/lib/reel-bundle.ts; spec:
  docs/superpowers/specs/2026-08-22-reel-export-design.md). The video renderer
  from docs/research/reel-to-video.md should consume the same bundle: headless
  render of each slide stage → frames, TTS for lines, mux; embed the bundle as
  MP4 metadata + per-beat chapter provenance (platforms that transcode strip
  metadata — on-screen citations stay burned in).
- **Tighten export scan regexes.** The sensitive-content scanner
  (ui/src/lib/export-scan.ts) misses some real shapes: JSON quoted-key secrets
  (`\"api_key\": \"...\"`), lowercase `bearer`, bare `/Users/name` with no trailing
  slash, and only the first match per family per row. Broaden coverage; it's the
  gate that warns before a reel leaves the machine.
- **Automated coverage for spotlight-snapshot capture.** `collectBundle`'s
  real-EventSpotlight createRoot capture path (ui/src/lib/export-collect.ts)
  has no automated test (jsdom can't mount/lay it out); it's currently only
  verified by manual Chrome walkthrough. Add coverage (headless browser /
  integration) so the offscreen-capture containing-block fix and
  sanitize-before-embed can't silently regress.
