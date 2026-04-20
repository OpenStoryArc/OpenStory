# Hermes Integration — Session Report

*Build → Discover → What's next. The simple-first-pass.*

**Date:** 2026-04-08
**Branch:** `research/hermes-integration`
**Method:** Engineering method — every `# VERIFY:` marker treated as a hypothesis with a known test, results recorded as the work proceeded.
**Constraint:** No Hermes container available. Purely static source verification + Rust port + tests.

---

## Part 1 — What I built

### A static-verification report

**`docs/research/hermes-integration/SOURCE_VERIFICATION.md`** — every `# VERIFY:` marker in the original prototype, mapped to either a verified file:line in the cloned `hermes-agent` repo at commit `6e3f7f36`, a contradiction (the prototype was wrong; documented), or a deferred runtime question. 8 sections, ~470 lines.

The verification pass produced 5 corrections to the prototype, all forced by reading actual source:

1. The plugin's `_on_session_start` hook expected `system_prompt` and `tools` kwargs that **are not passed** by Hermes.
2. The plugin's `_on_post_llm_call` expected a `response_message` kwarg that is actually named `assistant_response`.
3. The plugin's `_on_post_tool_call` expected an `is_error` kwarg that **does not exist**.
4. There is no `pre_user_message` hook in `VALID_HOOKS`. User prompts are captured via `pre_llm_call.user_message` instead.
5. Hermes's internal message storage is **always OpenAI shape** — the Anthropic-content-block branches in the translator are dead code in practice (kept defensively but marked).

### An updated Python prototype

**`docs/research/hermes-integration/translate_hermes.py`** — every `# VERIFY:` marker replaced with either a `# VERIFIED:` reference to the source file:line, or a `# RUNTIME:` note for what still needs a real session.

**`docs/research/hermes-integration/plugin_sketch.py`** — three corrected hook signatures (`_on_session_start`, `_on_post_llm_call`, `_on_post_tool_call`), one new hook (`_on_pre_llm_call` replacing the missing `_on_user_message`), one new hook (`_on_session_end` for per-loop completion state), and an explicit "must not feedback" non-use flag for the `pre_llm_call` return-value injection path and `inject_message`.

**`docs/research/hermes-integration/test_translate.py`** — 10 new tests added on top of the original 12, all encoding the verified ground truth from `tests/agent/test_anthropic_adapter.py:575` (the canonical OpenAI-shape tool call), `tests/run_agent/test_agent_loop_tool_calling.py:506` (valid roles), and `run_agent.py:_save_session_log:2408-2472` (the disk format docstring). **All 22 Python tests pass.**

### A Rust translator

**`rs/core/src/translate_hermes.rs`** — 660 lines, parallel to `translate.rs` (Claude Code) and `translate_pi.rs` (pi-mono). Pure functions: `is_hermes_format(line)` for detection and `translate_hermes_line(line, state)` for translation. Returns `Vec<CloudEvent>` with one CloudEvent per logical sub-event (so an assistant turn with thinking + 3 tool calls becomes 4 CloudEvents).

Key design choices:
- **Internal `LineCtx` struct** bundles the per-line envelope fields so builder functions take 3 args instead of 8 (clippy clean).
- **`HermesPayload` typed struct** added to `event_data.rs` as a new variant of the `AgentPayload` enum (alongside `ClaudeCode` and `PiMono`).
- **Deterministic synthetic event IDs** via `uuid5(NAMESPACE_URL, "hermes:{session}:{seq}:{subtype}")` — stable across re-translation passes, mirrors the Python prototype.
- **Defensive `parse_arguments_field`** handles JSON-string args (the canonical case), pre-parsed object args (forward-compat), and malformed JSON (stashed under `_raw` so the data isn't lost).
- **All accessor methods** in `AgentPayload` (12 of them) extended for the `Hermes` variant; downstream crates use `_` catch-alls so no other crate needed touching.

**Tests:** 21 unit tests inline in `translate_hermes.rs`, covering format detection, full session walks, every canonical message shape, multi-tool fan-out, orphaned tool results, deterministic IDs, ID uniqueness, and the `parse_arguments_field` edge cases. **All 21 pass.**

### A runtime-gaps document

**`docs/research/hermes-integration/RUNTIME_GAPS.md`** — explicit list of what cannot be confirmed without a real Hermes session, with the smallest experiment to close each gap and the estimated effort. 8 gaps total; 5 are minor, 1 is "we resolved this by changing the architecture," 1 is medium-priority for plugin correctness, and 1 is the v0.1.0 smoke test gate.

### Summary by file

| File | Status | Lines |
|---|---|---|
| `docs/research/hermes-integration/SOURCE_VERIFICATION.md` | new | 470 |
| `docs/research/hermes-integration/RUNTIME_GAPS.md` | new | 175 |
| `docs/research/hermes-integration/SESSION_REPORT_2026-04-08.md` | new | (this file) |
| `docs/research/hermes-integration/translate_hermes.py` | edited | +30 / -25 (comments) |
| `docs/research/hermes-integration/plugin_sketch.py` | edited | +85 / -45 |
| `docs/research/hermes-integration/test_translate.py` | edited | +175 / -0 |
| `rs/core/src/translate_hermes.rs` | new | 660 |
| `rs/core/src/event_data.rs` | edited | +160 / -20 |
| `rs/core/src/lib.rs` | edited | +1 / -0 |
| `rs/core/Cargo.toml` | edited | +1 / -1 (uuid v5 feature) |

### Test counts

| Suite | Before | After | Delta |
|---|---|---|---|
| Python `test_translate.py` | 12 | **22** | +10 |
| Rust `translate_hermes::tests` | 0 | **21** | +21 |
| Rust workspace lib tests (total) | ~431 | **452** | +21 |

All passing. No regressions in any other crate.

---

## Part 2 — What I discovered

Real findings from reading the source, ranked by surprise.

### 1. Hermes's persistence shape is structurally incompatible with OpenStory's existing watcher (the biggest surprise)

`run_agent.py:_save_session_log` writes `~/.hermes/sessions/session_{id}.json` as a **whole-file snapshot rewritten on every turn**. There's even an anti-clobber guard to prevent `--resume` from clobbering. OpenStory's existing file watcher uses byte-offset incremental reads built for append-only JSONL.

A "just point the existing watcher at `~/.hermes/sessions/`" path *cannot work* — the watcher would re-read the whole file every time it changes and would have to dedupe by event ID. This is solvable but it's a different ingestion model than what OpenStory has today.

**Implication:** The plugin path (emit per-event JSONL into a watched directory) is *more* important than the original brief made it sound. It's not the polite alternative — it's the only sensible v1 ingestion strategy. The plugin produces append-only JSONL that the existing watcher can consume without changes.

Gateway mode also writes a separate `{id}.jsonl` file (`gateway/session.py:942`) which IS append-only — but it only exists for gateway mode, not CLI mode, and the plugin gives one uniform path for both.

### 2. Hermes uses the OpenAI message shape internally, always

Despite being provider-polymorphic at the API boundary (it can call Claude, GPT, Gemini, MiMo, etc.), Hermes stores all conversations as OpenAI-shape messages. The Anthropic adapter at `tests/agent/test_anthropic_adapter.py:575` is a one-way translator: OpenAI → Anthropic for outgoing API calls. Anthropic responses get converted *back* to OpenAI shape before they enter `_session_messages`.

This eliminates an entire branch of the prototype's translator (the `_has_anthropic_tool_use` / `_translate_anthropic_tool_use` functions). They're kept in the Python prototype as defensive dead code marked DEAD AS OF 2026-04-08; the Rust port omits them entirely for clarity (the Rust translator can be re-extended in 5 lines if Hermes ever changes its mind).

The verified canonical message shapes (from `tests/agent/test_anthropic_adapter.py:575-697`):

- **Assistant tool call:** `{"role": "assistant", "content": "...", "tool_calls": [{"id": "tc_1", "function": {"name": "search", "arguments": '{"query": "test"}'}}]}`. Note `arguments` is a **JSON string**, not a parsed dict.
- **Tool result:** `{"role": "tool", "tool_call_id": "tc_1", "content": "results"}`. No `is_error`, `tool_name` is optional.
- **System (incl. injected):** `{"role": "system", "content": "..."}`. No special tag for compression summaries — distinguishable only by content.

### 3. Two feedback paths exist that the listener-as-algebra thesis must explicitly avoid

`hermes_cli/plugins.py:468-502` documents that `pre_llm_call` callbacks may **return** a context dict that Hermes will inject into the user message:

> For ``pre_llm_call``, callbacks may return a dict describing context to inject into the current turn's user message: `{"context": "recalled text..."}` or a plain string. ... All injected context is ephemeral — never persisted to session DB.

And `PluginContext.inject_message()` at `hermes_cli/plugins.py:164` lets plugins **interrupt** the agent and inject a message mid-run.

**Both are feedback paths into the coalgebra.** They're useful tools in general — exactly the mechanism a memory or recall plugin would use to surface relevant context to the agent — but they directly contradict the listener-as-algebra thesis, which says the OpenStory plugin should be a pure observer.

The OpenStory plugin must:
- Make every hook callback return `None` (so it never injects context)
- Never call `inject_message`

The verification doc and the plugin sketch both flag these as deliberate non-uses. Recording them here so a future maintainer doesn't accidentally promote one of them to a "convenient" feedback path and silently break the algebra/coalgebra split.

This finding is the most interesting one philosophically — the categorical framing isn't just abstract aesthetics. It produces a *real* engineering rule: Hermes hands you the gun; you must not pull the trigger.

### 4. `pre_llm_call.conversation_history` is a complete diff source

Both `pre_llm_call` and `post_llm_call` receive the **full Hermes-native message list** as `conversation_history`. This means the plugin doesn't need to track messages itself — it can diff `conversation_history` between calls and emit any new entries.

This collapses the plugin's three message-capture hooks (user, assistant, tool) into one and eliminates the user-prompt-capture problem entirely. The original prototype had a defensive `_on_user_message` placeholder hook with a `# VERIFY:` admitting "Hermes may not have such a hook" — there isn't one, but `pre_llm_call.user_message` directly provides the user input AND `conversation_history` provides everything else.

The current prototype doesn't take advantage of this — it keeps the per-call hooks for lower-latency emission and to stay close to the original shape. A v2 plugin should consider collapsing to a single hook.

### 5. `on_session_end` is a separate hook from `on_session_finalize`

The brief assumed `on_session_finalize` was "the session is over." It is, but `run_agent.py:9302` also fires `on_session_end` at the end of *each agent loop run* with `completed` and `interrupted` booleans. The two hooks have distinct semantics:

- `on_session_end` — per-agent-loop, fires at the end of each run, includes loop completion state
- `on_session_finalize` — per-session-boundary, fires on process exit / `/new` / `/reset`

The updated plugin hooks both: `on_session_end` to emit a per-turn-loop "turn complete" marker, `on_session_finalize` to close the writer when the session truly ends.

### 6. Hermes is bigger than it looks

The cloned repo has `cli.py` at 390KB (8400+ lines), `run_agent.py` at 482KB (9800+ lines), and a comprehensive test suite (`tests/agent/`, `tests/run_agent/`, `tests/cli/`, `tests/gateway/`, `tests/hermes_cli/`, etc.). That's the kind of repo where reading source efficiently matters — and where the *tests* are the cleanest specifications. The single most useful discovery this session was that `tests/agent/test_anthropic_adapter.py` is an executable specification for the canonical message shape: every test in that file uses the OpenAI shape as input, definitively answering the OpenAI-vs-Anthropic question that the brief flagged as the load-bearing prerequisite.

User suggestion mid-session — *"hermes might have tests or unit tests, or you could write unit tests that enable exploration and validation of hypothesis :)"* — was the right move. Tests are executable assertions about shape; reading them is faster than reading source and more reliable than reading docstrings.

### 7. Two new hooks the prototype didn't anticipate

`pre_api_request` and `post_api_request` (at `run_agent.py:7421` and `:8600`) wrap the actual HTTP call to the LLM provider, including `provider`, `base_url`, `api_mode`. Higher fidelity than `post_llm_call` if needed for raw response capture. Not used by the v1 plugin but recorded as a future option.

---

## Part 3 — What's next

What this session opens up, and what it doesn't.

### The thing this is about now

**Get a Hermes container running.** That's the entire blocker for everything else. The original brief estimated 30 minutes for the verification step "if Hermes boots cleanly"; that's still right, and the Hermes Docker setup (`docker/`, `Dockerfile` in the cloned repo) plus the existing `setup-hermes.sh` makes it tractable.

Once Hermes runs:

1. **Run the verification task once** (30 min). The recommended task ("Read README, list Python files, write summary") exercises every shape we need.
2. **Resolve gaps 1, 2, 4, 5** in `RUNTIME_GAPS.md` from one finalized session log (~30 min).
3. **Build the standalone `hermes-openstory` package** — port `plugin_sketch.py` and `recall_tool_sketch.py` into a real package layout, set up `pyproject.toml` with the entry-point declaration, write package-level tests (mocking the HTTP layer for the recall tool, mocking `invoke_hook` for the lifecycle hooks). ~half a day.
4. **End-to-end smoke test (gap 8)** — install the package into Hermes, run a session, watch it appear in OpenStory's dashboard, call the recall tool. This is the v0.1.0 ship gate.
5. **Publish to PyPI as `hermes-openstory v0.1.0`.**

Net effort estimate after Hermes is running: **~1 day** including package layout, smoke test, and publish. Without Hermes the upper bound is the same as today — blocked.

### What this session changed about my thinking

- **The plugin is more central than I thought.** I went in expecting "the plugin is one option among several." The discovered architecture (snapshot disk format, OpenAI internal shape, no append-only file path in CLI mode) means the plugin is the only sensible v1 ingestion path. That's not a downgrade — it's clarity.
- **The five integrations from the original brief are still right, but their order is firmer.** (1) translator+sink → (2) recall tool → (3) skill-extraction signals → (4) StructuralTurn training export → (5) cross-provider comparison. Items 3–5 all require items 1–2 to be deployed and exercised first; the temptation to start sketching them in parallel was wrong.
- **The categorical framing is engineering, not aesthetic.** "OpenStory must not feedback into the coalgebra" produces a concrete rule (return None from every hook callback, never call `inject_message`) that flows directly from the `LISTENER_AS_ALGEBRA.md` brief. I hadn't internalized that this would actually constrain the implementation — and the constraint is healthy.
- **Tests are specifications.** This will go into memory.

### What this session deliberately did NOT do

(Things I considered and rejected for scope reasons.)

- ❌ **Build the standalone `hermes-openstory` PyPI package.** Blocked on hook-kwarg verification. Building it now would mean shipping the package against an unverified shape.
- ❌ **Wire the Rust translator into OpenStory's file watcher.** The watcher routing (path-pattern check + first-line sniff) is needed before the translator is reachable from a running OpenStory server. It's small (~30 lines), but it touches a different crate and would benefit from a real plugin emitting real lines into the watched directory before being added.
- ❌ **The recall tool E2E test.** Blocked on the package being installed.
- ❌ **Backfill script for existing snapshots.** Blocked on resolving gap 1 (timestamp format) and on a snapshot-diff watcher mode in OpenStory that doesn't exist yet.
- ❌ **Schema parity test against pi-mono.** The Hermes translator's `HermesPayload` shares many fields with `PiMonoPayload` (text, model, tool, args, tool_call_id, etc.) but they're separate types because the views layer should branch on `agent` to render correctly. Unifying them would break the data sovereignty principle (each agent's native field names preserved).

### The interesting next-next question

After v0.1.0 ships, **the most novel feature is integration #4 — the StructuralTurn training-data export.** The research question is *"does training a tool-calling model on structurally decomposed traces (eval/apply phases separated, domain facts extracted, subagent boundaries explicit) produce better behavior than training on raw messages?"*

OpenStory + Hermes are uniquely positioned to answer this:
- OpenStory has the structurally decomposed turns (`StructuralTurn` at `rs/patterns/src/eval_apply.rs:37`)
- Hermes has the trajectory pipeline (`_convert_to_trajectory_format`, the tinker-atropos integration)
- The two together can emit training data in either shape from the same source sessions

That's a research output the OpenStory project has been positioned for since the patterns crate landed but hasn't yet had a downstream consumer for. Hermes is the first agent that actually has a use for it.

This is what makes the integration matter beyond "another agent OpenStory can observe." It's a research path.

### What I want to ask the user

1. **Is now the right time to commit Phases 1–3?** I've kept all changes uncommitted per the instruction. The work is one logical unit (research artifact + prototype updates + Rust translator) and would commit cleanly. Suggested message body would be "Phase 1–3 of the Hermes integration simple-first-pass: source verification, prototype updates, Rust translator port. All 22 Python + 21 Rust tests passing. Runtime gaps and findings documented."
2. **When you do get a Hermes container running, do you want me to do the verification pass myself, or do you want to capture the artifact and hand it to me?** Both are fine; the second is faster.
3. **Does the architecture finding change anything else you've been thinking about?** The "snapshot disk format vs append-only watcher" mismatch might have implications for other agent integrations beyond Hermes — anything that snapshots rather than appends would hit the same wall.

---

*Engineering method, hypothesis-driven, no Hermes runtime required. Exact measurements: 22 Python tests passing, 21 Rust tests passing, 452 total workspace lib tests passing, 0 regressions, 0 net clippy warnings introduced. Time: roughly the duration of one focused session.*
