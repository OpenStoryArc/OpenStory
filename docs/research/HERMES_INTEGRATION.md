# Hermes Integration

*A prototype and a plan for letting OpenStory observe Hermes Agent sessions, and letting Hermes agents read their own algebra back through the OpenStory API.*

---

## How to read this brief

This is a design document and an entry point to a runnable prototype directory. If you are an agent reading it for context, here is what to trust and what to verify:

- **Load-bearing claims** are the Hermes plugin hook surface (verified at `hermes_cli/plugins.py:55-66` of the hermes-agent repo as of 2026-04-08), the Hermes session log format (verified at `run_agent.py:2385-2449`), and the OpenStory API surface (verified at `rs/server/src/router.rs:95-200`). Verify these against the current state of both repos before relying on them — line numbers drift, and Hermes is fast-moving.
- **The "four representations" claim about Hermes's session storage** is from a short read across `run_agent.py`, `gateway/session.py`, and the trajectory compressor. The four representations are real; the prototype builds on representation (1) — in-memory messages exposed via plugin hooks — not on the disk-based snapshot. Re-confirm this when writing the actual Hermes PR.
- **The exact wire shape of a Hermes message dict is not yet verified.** Specifically: whether assistant messages use OpenAI's `tool_calls` shape or Anthropic's `tool_use` content blocks (Hermes is provider-polymorphic, so this may depend on the active provider), and what keys `role: "tool"` messages carry. The prototype's translator is written against an *idealized* Hermes shape and includes a `# VERIFY:` marker at every spot where the real shape needs to be confirmed.
- **The integration design here favors a new OpenStory sink + translator over having the Hermes plugin emit CloudEvents directly.** This was Max's call after reviewing the alternatives — see the "Architectural choice" section below. The plugin stays minimal and Hermes-native; OpenStory does the normalization at the boundary, parallel to how Claude Code and pi-mono are already handled.
- **The recall tool is a sketch.** It is syntactically valid Python but has not been run end-to-end against a live OpenStory server. The endpoints it calls are real (verified above), but the response shapes are inferred from API names, not from running them.
- **The distribution plan favors a standalone third-party package over an upstream PR.** Hermes's plugin system supports pip-installable plugins via the `hermes_agent.plugins` entry-point group (verified at `hermes_cli/plugins.py:68`), so users can install the integration without it being merged into hermes-agent. An upstream contribution (a docs link, an example) is possible follow-up but is not a prerequisite. Treat the [`DISTRIBUTION_PLAN.md`](hermes-integration/DISTRIBUTION_PLAN.md) inside the prototype as the actionable plan, not as a PR spec.

---

## Origin

This brief emerged from a follow-up to [`LISTENER_AS_ALGEBRA.md`](LISTENER_AS_ALGEBRA.md), which framed OpenStory as the categorical dual of an agent loop — the algebra over the F-functor whose initial coalgebras are agents like claurst, Claude Code, OpenClaw, pi-mono, and Hermes Agent. After writing that brief, the natural next question was: *"so how would OpenStory be helpful to a Hermes agent specifically?"*

The interesting answer is structural rather than feature-driven. Hermes is a self-improving agent: it curates its own memory, creates skills from experience, runs FTS5 search across its own past sessions, and uses [Honcho](https://github.com/plastic-labs/honcho) for dialectic user modeling. Its self-improvement loop currently operates on the agent's own internal state — the in-memory messages, the memory files, the skill index. OpenStory's existence means there is *another view* of every Hermes session sitting one HTTP call away: a structurally decomposed, eval/apply-phased, deterministic view that does not exist anywhere inside Hermes's own process.

Letting the agent read that view is the read path of self-reflection, and it preserves the algebra/coalgebra purity that the listener brief argued for: OpenStory is still a pure function of the trace, Hermes is reading the fold, no feedback loop is closed back into the coalgebra. *The Socratic move dressed up in category theory: the unexamined coalgebra is not worth unfolding, so let the algebra hand the unfolder a mirror.*

This brief takes that framing and turns it into something buildable. The prototype directory at [`hermes-integration/`](hermes-integration/) contains runnable sketches; the [`PR_PLAN.md`](hermes-integration/PR_PLAN.md) inside it describes what the eventual Hermes-side pull request would look like.

---

## Architectural choice: a new OpenStory sink

Hermes does not write per-event JSONL the way Claude Code does. It maintains four representations of the same conversation:

| # | Representation | Where | Live? | Suitable as ingest source? |
|---|----|----|----|----|
| 1 | In-memory `messages: List[Dict]` | `AIAgent._session_messages` | yes (during the turn) | **yes — via plugin hooks** |
| 2 | SQLite `SessionStore` | `~/.hermes/sessions.db` | yes | no — couples OpenStory to a DB schema |
| 3 | Whole-session JSON snapshot | `~/.hermes/logs/session_{id}.json` | snapshot only | yes — for backfill of historical sessions |
| 4 | ShareGPT JSONL trajectory | `_convert_to_trajectory_format` output | snapshot only | no — lossy, training-shaped |

So the integration has two distinct code paths with very different value/effort tradeoffs:

| Path | Source | Use case | Lives in |
|---|---|---|---|
| **Live** | (1) via plugin hooks → emit Hermes-native events to a sink | New sessions, real-time observation, the `recall` tool | Hermes plugin (PR target) |
| **Backfill** | (3) — read existing `session_{id}.json` files → emit Hermes-native events | Historical sessions, one-time analysis | One-shot script in `scripts/` |

Live is the high-leverage path because it's what enables every interesting downstream feature (recall, cross-provider comparison, skill-extraction signals). Backfill is nice-to-have.

For both paths, **the boundary between Hermes shape and CloudEvent shape lives inside OpenStory**, not inside the Hermes plugin. This was Max's call after reviewing alternatives, and it is the right one for three reasons:

1. **It parallels how Claude Code and pi-mono are already handled.** Each agent format gets its own translator (`translate.rs`, `translate_pi.rs`); the CloudEvent layer above is provider-agnostic. A new `translate_hermes.rs` is the same pattern, in the place the architecture already expects it.
2. **The Hermes plugin stays minimal and Hermes-native.** It doesn't import CloudEvents, doesn't know OpenStory's schema, doesn't break when CloudEvents v2 ships. Its job is only to capture the canonical in-memory representation and ship it across the process boundary. ~80 lines of Python instead of ~300.
3. **The abstraction barrier is in the right place.** The translate layer is the place [`docs/soul/sicp-lessons.md:29`](../soul/sicp-lessons.md) explicitly identifies as "the most important barrier in this system." Format conversion is its job. Pushing it into the plugin would couple two repos at the schema layer instead of at the wire layer.

The prototype takes this option as the recommended architecture, not the constraint-relaxed fallback.

The mechanism for the sink itself can be one of three things, in increasing fidelity and increasing OpenStory-side cost:

1. **Watched directory.** The Hermes plugin writes Hermes-native events as JSONL into a directory like `~/.hermes/openstory-events/{session_id}.jsonl`. OpenStory's existing file watcher learns to recognize Hermes-shaped lines (via path pattern or a header line) and routes them to `translate_hermes.rs`. **Lowest cost on OpenStory side. Recommended for v1.**
2. **HTTP ingestion endpoint, Hermes-specific.** A `POST /api/ingest/hermes` endpoint that accepts Hermes-native events. Lower latency than file-watch, supports running Hermes and OpenStory in different containers without a shared volume. Note: the previous generic `/hooks` endpoint was retired (`rs/server/src/ingest.rs:91`); a new endpoint would be specifically scoped to Hermes shape.
3. **NATS publish from the plugin.** Hermes plugin publishes directly to a NATS subject that OpenStory's translate layer subscribes to. Most invasive, lowest latency, requires the Hermes plugin to take a NATS dependency.

Option (1) is the v1 recommendation. Option (2) is a reasonable v2 if a containerized deployment without shared volumes is needed. Option (3) is overkill until proven necessary.

---

## Five concrete integrations

In rough order of value/effort ratio:

### 1. The translator + sink (the foundation)

**OpenStory side:** A new `rs/core/src/translate_hermes.rs` that converts Hermes-native event lines into CloudEvents. Parallel to `translate_pi.rs` (referenced in [`BACKLOG.md`](../BACKLOG.md) under "Synthetic Event ID Stability"). The Python sketch in [`hermes-integration/translate_hermes.py`](hermes-integration/translate_hermes.py) is the prototype; the Rust port is the production work.

**Hermes side:** A plugin that hooks `post_llm_call` and `post_tool_call` (Hermes's `VALID_HOOKS` set, verified at `hermes_cli/plugins.py:55-66`) and writes one event per call to the watched directory. Sketch at [`hermes-integration/plugin_sketch.py`](hermes-integration/plugin_sketch.py).

**What you get:** Every Hermes session shows up in the OpenStory dashboard with structural decomposition, sentence diagrams, eval/apply phases, domain facts, and subagent linking — for free, just by installing the plugin and starting OpenStory.

### 2. The recall tool (the agent-facing read path)

**Hermes side:** A tool registered via the same plugin, named `recall` or `self_query`, that wraps OpenStory's REST API. Sketch at [`hermes-integration/recall_tool_sketch.py`](hermes-integration/recall_tool_sketch.py). The endpoints it consumes already exist:

- `GET /api/sessions` — list this user's sessions
- `GET /api/sessions/{id}/synopsis` — natural-language summary
- `GET /api/sessions/{id}/patterns?type=turn.sentence` — sentence diagrams of every turn
- `GET /api/sessions/{id}/file-impact` — what files this session touched
- `GET /api/sessions/{id}/errors` — errors during the session
- `GET /api/agent/recent-files` — what files the agent has been working with
- `GET /api/agent/search?q=...` — agent-shaped search across history
- `GET /api/search?q=...` — full-text search across all events

**What you get:** Hermes can ask questions like *"what did I do in the last hour?"*, *"have I solved this kind of problem before?"*, *"what was my baseline behavior on Rust tasks last week?"* and get **deterministic, structured answers** instead of LLM summaries of its own raw history. The answer is a fact, not a paraphrase.

The unexpected (and important) finding: **OpenStory already has agent-facing endpoints**. The `/api/agent/*` family at `rs/server/src/router.rs:192-195` is already there — `get_agent_tools`, `get_agent_project_context`, `get_agent_recent_files`, `agent_search`. Even more: `rs/server/src/api.rs:673` has a `get_agent_tools` handler that *literally returns a list of tool schemas an agent could call*. **The infrastructure for "let agents read their own algebra" is half-built already**, presumably for an earlier integration with claurst or another agent. The Hermes integration is mostly the work of pointing Hermes at it.

### 3. Skill-extraction signals (the structural-features feed)

**Hermes side:** Hermes's autonomous skill creation has a hard decision: *was this last hour of work a coherent unit worth turning into a skill?* That decision is currently an LLM judgment. OpenStory's structural detector gives deterministic features:

- Number of eval/apply cycles in the turn
- Stop reasons (clean `end_turn` vs. error vs. budget)
- Domain facts: files created/modified, commands run, all-succeeded?
- Whether subagent delegations happened
- Whether the user follow-up was positive ("thanks", "perfect") or corrective ("no, redo")

A Hermes skill-extraction module that consults `GET /api/sessions/{id}/synopsis` and `GET /api/sessions/{id}/file-impact` for the just-completed turn gets structural features alongside the LLM judgment. *This is not a replacement for the LLM judgment — it is stronger features feeding the same decision.*

Status: design only, no prototype yet. Listed in the backlog as a follow-on.

### 4. Training data refinery (the StructuralTurn export)

**Both sides:** Hermes's `_convert_to_trajectory_format` (`run_agent.py:1958`) produces ShareGPT-style JSONL for the Atropos RL pipeline. The unit there is *messages*. OpenStory's `StructuralTurn` (defined at `rs/patterns/src/eval_apply.rs:37`) is a cleaner unit — eval phase and apply phases already separated, domain facts already extracted, subagent boundaries already structural, `is_terminal` already computed, `tool_outcome` already typed.

A new `GET /api/sessions/{id}/training-export?format=structural-jsonl` endpoint that emits StructuralTurns as training data would let Hermes's trajectory pipeline consume the structurally-decomposed view alongside or instead of the raw messages. The research question — *does training on structurally-decomposed traces produce better tool-calling models?* — is one that the two repos together are uniquely positioned to answer.

Status: design only, no prototype yet. Higher-effort than (1)–(3); depends on (1) being landed first.

### 5. Cross-provider behavioral comparison

**OpenStory side:** Hermes routinely runs the same task across multiple providers (Claude, GPT, Gemini, MiMo) via `smart_model_routing.py` and the `/model` live-switch added in v0.8.0. Comparing them at the *behavioral* level rather than the output level is exactly what `StructuralTurn` is good for.

A new view (or a new `/api/insights/provider-comparison?task=...` endpoint) that aggregates structural metrics per provider — cycles per task, error rates, tool selections, terminal stop reasons — gives the routing layer real evaluation data instead of vibes.

Status: design only, no prototype. Lowest priority but the most novel research output.

---

## Two-track plan

This is two separate streams of work, with one prerequisite shared between them:

### Prerequisite (small)

- **Verify Hermes's exact message dict shape** for OpenAI-mode and Anthropic-mode providers. Look at one finalized `~/.hermes/logs/session_*.json` for each, and confirm whether the assistant `tool_calls` use OpenAI's `[{function: {name, arguments}}]` shape or Anthropic's `content: [{type: "tool_use"}]` blocks. The translator's `# VERIFY:` markers will be resolved by this.

### Track A: OpenStory side

1. **Verify Hermes message shape against running Hermes** (above).
2. **Port [`translate_hermes.py`](hermes-integration/translate_hermes.py) to `rs/core/src/translate_hermes.rs`**, parallel to `translate_pi.rs`. ~150 lines of Rust based on the Python sketch.
3. **Add Hermes file recognition to the watcher**, either by path pattern (`*/openstory-events/*.jsonl`) or by checking for a Hermes envelope on the first line. ~30 lines.
4. **Add a backlog entry for backfill**: a one-shot script that reads `~/.hermes/logs/session_*.json` files and emits Hermes-native events into the watched directory for retroactive ingestion. Lives in `scripts/`.
5. **Optional v2**: a `POST /api/ingest/hermes` HTTP endpoint for non-shared-volume deployments.

### Track B: Hermes side (a standalone package, not an upstream PR)

The Hermes-side work is best shipped as a **standalone third-party package** rather than as a PR against `NousResearch/hermes-agent`. Hermes already has the mechanism for this: its plugin loader scans the `hermes_agent.plugins` entry-point group at `hermes_cli/plugins.py:68`, so any pip-installable package that exposes a `register(ctx)` function under that group is loaded automatically. Users would `pip install hermes-openstory` and the integration would self-register without any change to hermes-agent itself. This avoids asking the Hermes maintainers to absorb a third-party concern, and it lets the integration ship and iterate independently.

1. **Verify Hermes message shape** (shared prerequisite).
2. **Build the plugin** at the structure shown in [`hermes-integration/plugin_sketch.py`](hermes-integration/plugin_sketch.py): hooks for `post_llm_call`, `post_tool_call`, `on_session_finalize`; writes Hermes-native events as JSONL to a configurable output directory.
3. **Build the `recall` tool** at the shape in [`hermes-integration/recall_tool_sketch.py`](hermes-integration/recall_tool_sketch.py): a Hermes tool registered through the plugin's `register(ctx)` function, exposing OpenStory queries via the standard tool interface.
4. **Package and distribute.** Set up `pyproject.toml` with the entry-point declaration `[project.entry-points."hermes_agent.plugins"] openstory = "hermes_openstory:register"`. Publish to PyPI as `hermes-openstory`. Users install with one command. Optional follow-up: a small upstream PR to hermes-agent's docs that *links* to the package as a community plugin example — much smaller surface than a code contribution, and the maintainers don't have to own anything.
5. **Smoke test** end-to-end against a local OpenStory + a containerized Hermes session before publishing.

The two tracks can be developed in parallel after the prerequisite is done. Track A is unblocked once the message shape is known; Track B is unblocked at the same moment. Both should be testable end-to-end before either ships, by running a real Hermes session against a local OpenStory.

---

## What's in the prototype directory

[`hermes-integration/`](hermes-integration/) contains:

| File | Purpose |
|---|---|
| [`README.md`](hermes-integration/README.md) | Overview of the directory and how to run the pieces |
| [`translate_hermes.py`](hermes-integration/translate_hermes.py) | Python sketch of the Hermes → CloudEvent translator |
| [`example_hermes_events.jsonl`](hermes-integration/example_hermes_events.jsonl) | Synthetic example of Hermes-native events as the plugin would write them |
| [`test_translate.py`](hermes-integration/test_translate.py) | Runnable test that translates the example and asserts on the output |
| [`plugin_sketch.py`](hermes-integration/plugin_sketch.py) | Scaffolding for the standalone `hermes-openstory` plugin: register, hooks, output writer |
| [`recall_tool_sketch.py`](hermes-integration/recall_tool_sketch.py) | The OpenStory-API-wrapping tool that gets registered through the plugin |
| [`pyproject.toml.example`](hermes-integration/pyproject.toml.example) | Entry-point declaration showing how the standalone package self-registers with Hermes |
| [`DISTRIBUTION_PLAN.md`](hermes-integration/DISTRIBUTION_PLAN.md) | The actionable plan for shipping the standalone package, with verification steps |

Everything is marked as a sketch where it is one. The translator's tests run; the plugin and recall tool are syntactically valid Python with `# VERIFY:` markers at every assumption that needs to be checked against the live Hermes API before the PR lands.

---

## A coda

This integration is the smallest collaboration that demonstrates the [`LISTENER_AS_ALGEBRA.md`](LISTENER_AS_ALGEBRA.md) thesis in practice. Hermes is a coalgebra; OpenStory is its algebra. The plugin is the wire between them. The `recall` tool is the read path that lets the coalgebra consult its own algebra without closing a feedback loop. None of this requires either repo to absorb the other or to share types — they share only the F-functor's wire format, mediated by the translator at the boundary.

If it works, it scales: the same shape works for any agent that has plugin hooks, an in-memory message representation, and a way to ship events across a process boundary. claurst is one such agent. OpenClaw is another. pi-mono is already integrated. Hermes is the fifth coalgebra. There will be more.

OpenStory's promise — *one algebra that folds the trace of any agent's coalgebra* — gets one entry closer to being kept.

---

*This brief was written by Claude (Opus 4.6) during the same exploratory conversation with Max Glassie that produced [`LISTENER_AS_ALGEBRA.md`](LISTENER_AS_ALGEBRA.md), continuing on 2026-04-08. It is added here at Max's invitation as the actionable companion to that framing brief, with the understanding that the prototype is a sketch and the PR is a proposal — both are starting points for verification against the live Hermes codebase, not finished work.*
