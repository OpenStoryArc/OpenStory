# Hermes Integration — Source Verification

*A static-analysis verification of the Hermes Agent integration prototype against the actual `NousResearch/hermes-agent` source code, with no Hermes runtime required.*

**Date:** 2026-04-08
**Hermes commit verified against:** `6e3f7f36` (`docs: add tool_progress_overrides to configuration reference`, latest `main` as of pull)
**Status:** Phase 1 of the simple-first-pass plan in `HERMES_INTEGRATION.md` and `DISTRIBUTION_PLAN.md`.

---

## Method

Each `# VERIFY:` marker in `translate_hermes.py`, `plugin_sketch.py`, and `recall_tool_sketch.py` is treated as a hypothesis. The hypothesis is either:

- **✓ VERIFIED** — confirmed against a specific file:line in the cloned `hermes-agent` repo at `~/projects/hermes-agent`
- **✗ CONTRADICTED** — actual source disagrees with the prototype's assumption; prototype must change
- **○ UNRESOLVED — RUNTIME** — cannot be confirmed by reading source alone; requires a real Hermes session

For each verified claim a file:line reference is recorded so a future agent (or me) can re-verify after Hermes drift.

This document was produced from static reads of the cloned `hermes-agent` repo only. **No Hermes container was booted.** No real session log was captured. All claims are about what the source says *should* happen — runtime behavior remains the load-bearing prerequisite for actually shipping the integration.

---

## 1. Plugin system surface

### 1.1 `VALID_HOOKS` location

**Hypothesis:** `hermes_cli/plugins.py:55-66` (per the brief).

**Result:** **✓ VERIFIED.** Exact file and line range. Lines 55-66 hold:

```python
VALID_HOOKS: Set[str] = {
    "pre_tool_call",
    "post_tool_call",
    "pre_llm_call",
    "post_llm_call",
    "pre_api_request",
    "post_api_request",
    "on_session_start",
    "on_session_end",
    "on_session_finalize",
    "on_session_reset",
}
```

**Discovery (not in the brief):** Two hooks the prototype didn't anticipate exist:

- `pre_api_request` / `post_api_request` — wrap the actual HTTP call to the LLM provider. Higher fidelity than `post_llm_call` if needed for raw response capture; not used by the prototype.
- `on_session_end` — distinct from `on_session_finalize`. See §1.5.

### 1.2 Entry-point group

**Hypothesis:** `hermes_cli/plugins.py:68` declares `ENTRY_POINTS_GROUP = "hermes_agent.plugins"`.

**Result:** **✓ VERIFIED.** Line 68 exactly. The standalone-package distribution path (Track B in the brief) is unblocked: any pip-installable package that exposes a `register(ctx)` function under this entry-point group is auto-discovered by `PluginManager._scan_entry_points()` at `hermes_cli/plugins.py:339`.

### 1.3 `PluginContext` API

**Hypothesis:** `PluginContext` at `hermes_cli/plugins.py:124`, exposes `register_tool()`, `register_hook()`, `register_cli_command()`.

**Result:** **✓ VERIFIED.** Class at `:124`. Methods at:

- `register_tool(name, toolset, schema, handler, check_fn=None, requires_env=None, is_async=False, description="", emoji="")` — `:133`
- `register_hook(hook_name, callback)` — `:218`
- `register_cli_command(name, help, setup_fn, handler_fn, description)` — `:192`

The plugin sketch's `register(ctx)` function calls these correctly.

**Discovery:** `PluginContext` also exposes `inject_message(content, role="user")` at `:164`. This is a *feedback-into-the-coalgebra* path — plugins can interrupt the agent and inject messages mid-run. **The OpenStory plugin must not use this.** It violates the listener-as-algebra thesis. Recording its existence here so we don't accidentally adopt it later.

### 1.4 Hook return value semantics

**New finding:** `invoke_hook()` at `hermes_cli/plugins.py:468-502` does NOT just call hook callbacks — it also collects their return values:

> For ``pre_llm_call``, callbacks may return a dict describing context to inject into the current turn's user message: `{"context": "recalled text..."}` or a plain string.

**This is the second feedback path.** A `pre_llm_call` hook that returns context will have that context injected into the *user message* (preserved across the prompt cache prefix). The OpenStory plugin must keep all hooks returning `None`. Like `inject_message`, this is a deliberate non-use, recorded for clarity.

Each callback is wrapped in its own `try/except` at `:491-500` — so the prototype's defensive `try/except: pass` wrappers are belt-and-suspenders, not strictly required. They're harmless and can stay.

### 1.5 `on_session_end` vs `on_session_finalize` — distinct hooks

**The prototype treats `on_session_finalize` as "the session is over." It is, but there's also a sibling hook `on_session_end` with different semantics that the prototype didn't know about.**

| Hook | Where called | Kwargs | Purpose |
|---|---|---|---|
| `on_session_end` | `run_agent.py:9302` | `session_id, completed, interrupted, model, platform` | Fires at the **end of each agent loop run**. Includes `completed` and `interrupted` booleans — we can know if the loop ended cleanly. |
| `on_session_finalize` | `cli.py:617`, `cli.py:3434`, `cli.py:3437`, `gateway/run.py:1487`, `gateway/run.py:3289` | `session_id, platform` | Fires at **session boundaries** — process exit, `/new`, `/reset`, gateway shutdown. The signal that "this session is done forever, close any per-session resources." |

**Implication for the prototype:** The plugin should hook `on_session_finalize` (current) to close the writer when the session truly ends, AND optionally hook `on_session_end` (new) to record completed/interrupted state per agent loop run. The prototype currently does the first; recommend adding the second.

---

## 2. Hook kwargs — verified from `invoke_hook()` call sites

The brief flagged the kwargs each hook receives as guesses. Here's the actual signature from each `_invoke_hook(...)` call site.

| Hook | File:Line | Verified kwargs |
|---|---|---|
| `on_session_start` | `run_agent.py:7089` | `session_id, model, platform` |
| `pre_llm_call` | `run_agent.py:7180` | `session_id, user_message, conversation_history, is_first_turn, model, platform` |
| `pre_api_request` | `run_agent.py:7421` | `task_id, session_id, platform, model, provider, base_url, api_mode, ...` |
| `post_api_request` | `run_agent.py:8600` | `task_id, session_id, platform, model, provider, base_url, api_mode, ...` |
| `post_llm_call` | `run_agent.py:9203` | `session_id, user_message, assistant_response, conversation_history, model, platform` |
| `on_session_end` | `run_agent.py:9302` | `session_id, completed, interrupted, model, platform` |
| `pre_tool_call` | `model_tools.py:503` | `tool_name, args, task_id, session_id, tool_call_id` |
| `post_tool_call` | `model_tools.py:532` | `tool_name, args, result, task_id, session_id, tool_call_id` |
| `on_session_finalize` | `cli.py:617`, `gateway/run.py:1487`, etc. | `session_id, platform` |

### Prototype corrections forced by these signatures

1. **`_on_session_start`** — the prototype expected `system_prompt` and `tools` kwargs. **Neither is passed.** If the plugin needs them, they have to come from somewhere else (the agent's instance fields, or the next `pre_llm_call`'s `conversation_history[0]` system message).
2. **`_on_post_llm_call`** — the prototype expected `response_message`. **The actual kwarg is `assistant_response`.** Bug in the sketch; trivial fix.
3. **`_on_post_tool_call`** — the prototype expected `is_error: bool`. **There is no `is_error` kwarg.** If the plugin wants to flag tool errors, it must infer from `result` content (string-match for `"Error:"` prefix or similar). Or — better — read the `assistant_response` shape on the next `post_llm_call` to see how the model treated the result.
4. **`pre_llm_call.conversation_history` is the ground truth.** The prototype's `_on_user_message` placeholder hook (which it admitted "Hermes may not have") is unnecessary: `pre_llm_call` gives us `user_message` directly, AND it gives us the full `conversation_history` list which is **the canonical Hermes message dict shape we've been guessing about**.

### A simpler plugin design becomes possible

Given that `pre_llm_call` provides `conversation_history` (which is the full message list) and `post_llm_call` provides the same plus `assistant_response`, **the plugin doesn't need to track messages itself**. It can simply diff `conversation_history` between calls and emit any new entries. This eliminates the prototype's per-tool-call hook plumbing for capturing user prompts.

Recommendation: simplify the plugin to a single per-turn hook (`post_llm_call`) that emits all *new* messages since the previous call, plus session lifecycle hooks. The `post_tool_call` hook becomes redundant for capture purposes — its result will be in the next `pre_llm_call`'s `conversation_history` anyway. The only reason to keep `post_tool_call` is **lower-latency emission of tool results** before the next LLM call begins.

---

## 3. The session log file — disk format

This is the **biggest discovery** of the verification pass: the brief's claim about the disk format was *partially* right, but the file is structured differently from what the prototype assumes.

### 3.1 File location and write timing

**Verified at `run_agent.py:933-937`:**

```python
hermes_home = get_hermes_home()
self.logs_dir = hermes_home / "sessions"
self.logs_dir.mkdir(parents=True, exist_ok=True)
self.session_log_file = self.logs_dir / f"session_{self.session_id}.json"
```

So the path is `~/.hermes/sessions/session_{session_id}.json`. The brief's `~/.hermes/logs/session_{id}.json` was **wrong about the directory** — it's `sessions/`, not `logs/`. Cosmetic error, easy fix.

### 3.2 The file is a snapshot, NOT append-only

**Verified at `run_agent.py:2408-2472` (`_save_session_log`).** Quoting the docstring:

> Save the full raw session to a JSON file. Stores every message exactly as the agent sees it: user messages, assistant messages (with reasoning, finish_reason, tool_calls), tool responses (with tool_call_id, tool_name), and injected system messages (compression summaries, todo snapshots, etc.).
>
> REASONING_SCRATCHPAD tags are converted to `<think>` blocks for consistency.
>
> **Overwritten after each turn so it always reflects the latest state.**

This is a **whole-file rewrite per turn**, not append-only. There's even an anti-clobber guard (`:2437-2448`) that refuses to overwrite a larger log with a smaller one — protection against `--resume` clobbering.

**Implication for OpenStory:** OpenStory's existing file watcher is built for append-only JSONL with byte-offset incremental reads. A whole-file rewrite is a different ingestion model — the watcher would re-read the whole file and dedupe by event ID, or it would need a "snapshot diff" mode that compares the current and previous states.

This is a **structural mismatch** between Hermes's persistence model and OpenStory's existing watcher. The plugin path (per-event JSONL into a watched directory) avoids this entirely by emitting append-only data ourselves.

### 3.3 The top-level shape

**Verified at `run_agent.py:2450-2461`:**

```python
{
    "session_id": str,
    "model": str,
    "base_url": str,
    "platform": str,
    "session_start": str,        # ISO-8601
    "last_updated": str,         # ISO-8601
    "system_prompt": str,
    "tools": list,
    "message_count": int,
    "messages": [ ... ],         # the message list
}
```

The prototype's translator currently builds CloudEvents from the bottom up — one envelope per message. It doesn't currently use the snapshot envelope at all. **That's actually fine** — the snapshot envelope's fields (`model`, `system_prompt`, `tools`) belong on the `system.session.start` CloudEvent, which is exactly where the translator already puts them. The plugin just needs to capture them at session start.

But: **the prototype's `_on_session_start` hook can't capture `system_prompt` or `tools`** because they aren't passed as kwargs. The plugin will have to read them from the agent state object differently — or simply emit a session_start event lazily on the first `post_llm_call` (where `conversation_history[0]` will contain the system message).

### 3.4 Two storage paths in the same directory

`~/.hermes/sessions/` contains both file types:

- `session_{id}.json` — snapshot, written by `run_agent.py` for **all modes** (CLI and gateway)
- `{id}.jsonl` — append-only transcript, written by `gateway/session.py:942` for **gateway mode only** (CLI uses SQLite at `~/.hermes/state.db` instead)

So the disk picture is:

| Mode | Snapshot JSON | Transcript JSONL | SQLite |
|---|---|---|---|
| CLI | yes | no | yes (`state.db`) |
| Gateway | yes | yes | yes (`state.db`) |

**Implication:** A gateway-only OpenStory integration *could* skip the plugin and just watch `~/.hermes/sessions/*.jsonl` directly. That's appealing — zero-install for gateway users. But it doesn't work for CLI users, which is the more common case, so the plugin path remains the right v1.

---

## 4. The internal message dict shape (the load-bearing question)

The prototype's translator has branches for two shapes — OpenAI-style `tool_calls` and Anthropic-style `content` blocks with `tool_use` — and a `# VERIFY:` marker at every spot.

**Definitive answer from `tests/agent/test_anthropic_adapter.py:575-697`:** Hermes's **internal message storage is always OpenAI shape**. The Anthropic adapter (`convert_messages_to_anthropic`) is a **one-way translator** at the API boundary that takes OpenAI-shape messages and produces Anthropic-shape API requests. It is not bidirectional.

Quoting the test fixture at `tests/agent/test_anthropic_adapter.py:575`:

```python
def test_converts_tool_calls(self):
    messages = [
        {
            "role": "assistant",
            "content": "Let me search.",
            "tool_calls": [
                {
                    "id": "tc_1",
                    "function": {
                        "name": "search",
                        "arguments": '{"query": "test"}',
                    },
                }
            ],
        },
        {"role": "tool", "tool_call_id": "tc_1", "content": "search results"},
    ]
    _, result = convert_messages_to_anthropic(messages)
```

The test takes the OpenAI shape as **input**. Several other tests in the same file (`test_converts_tool_results`, `test_merges_consecutive_tool_results`, `test_strips_orphaned_tool_use`) all use the same OpenAI input shape.

### 4.1 Verified message shapes

**User message:**
```python
{"role": "user", "content": "string"}
```

**Assistant text-only message:**
```python
{
    "role": "assistant",
    "content": "string",                # text or empty string
    "reasoning": "string" | None,       # optional, top-level field
    "finish_reason": "stop" | None,
}
```

**Assistant tool-call message:**
```python
{
    "role": "assistant",
    "content": "" | "string",           # may be empty when tool_calls present
    "tool_calls": [
        {
            "id": "tc_1",
            "function": {
                "name": "tool_name",
                "arguments": '{"json": "string"}',  # JSON STRING, not parsed dict
            },
        },
    ],
    "reasoning": "string" | None,
    "finish_reason": "tool_calls" | "stop",
}
```

**Tool result message:**
```python
{
    "role": "tool",
    "tool_call_id": "tc_1",
    "content": "result string",         # plain string, not array
    "tool_name": "search",              # OPTIONAL — present in some paths, absent in others
}
```

**System message (including injected):**
```python
{"role": "system", "content": "string"}
```

There is **no `is_error` field** on tool messages. There is **no `name` alias for `tool_name`** — the prototype's defensive `msg.get("tool_name") or msg.get("name")` should drop the `name` fallback. Same for `tool_call_id` vs `id` on the tool result — the canonical key is `tool_call_id`.

### 4.2 Edge cases verified from tests

- **Tool calls use JSON string for `arguments`** — `'{"query": "test"}'`, not `{"query": "test"}`. The translator handles this with `json.loads(args_raw) if isinstance(args_raw, str)`. ✓
- **`content` can be empty string** on assistant messages with tool calls. The prototype handles via `_extract_text` returning `""`.
- **Tool results can be orphaned by compression** (`tests/agent/test_anthropic_adapter.py:651`) — context compression can strip the assistant tool_use message but leave the tool_result. The translator should not crash on this; currently it doesn't (it would just emit a tool_result CloudEvent with no preceding tool_use, which is fine for OpenStory's pipeline).
- **The Anthropic SDK is consumed via streaming** at `run_agent.py:4543` (`_call_anthropic`), but the streamed result is **converted back to OpenAI shape** before being stored in `_session_messages`. So the only time Anthropic content blocks exist in Hermes is mid-stream inside the SDK adapter — never in persisted state.

### 4.3 The translator's Anthropic-content-block branch is dead code

The translator's `_has_anthropic_tool_use()` and `_translate_anthropic_tool_use()` functions handle `content: [{type: "tool_use", ...}]` blocks. **These will never fire against real Hermes data.** They can be removed.

**Recommendation:** Keep them for now (defensive against shape drift in a future Hermes release), but mark them clearly as `# DEAD AS OF 2026-04-08 (hermes-agent 6e3f7f36):` so a future maintainer knows they're untested-in-practice. The cost of keeping them is ~30 lines; the value if Hermes ever changes is significant.

### 4.4 Reasoning is a string, not a content block

Two pieces of evidence:

1. The session log docstring at `run_agent.py:2413` says "assistant messages (with reasoning, finish_reason, tool_calls)" — listing `reasoning` as a top-level field alongside the other top-level OpenAI fields.
2. The Anthropic streaming consumer at `run_agent.py:4582-4586` extracts `thinking_text` from `content_block_delta` events and fires `_fire_reasoning_delta(thinking_text)` — meaning the streamed Anthropic thinking blocks are converted to a flat reasoning string before being stored.

So the translator's `(a) top-level reasoning` branch is the right one. The `(b) Anthropic thinking content block` branch is dead code (same recommendation as 4.3 — keep but mark).

### 4.5 System-injected messages have no special tag

Plain `{"role": "system", "content": "..."}`. Compression summaries and todo snapshots are distinguishable only by their content text (e.g., "Compressed history follows:" or similar prefixes), not by a separate field. The prototype currently maps all system messages to `system.injected.other` — that's fine for v1 but means OpenStory can't tell a compression summary apart from a regular system prompt without content sniffing. **Defer.**

---

## 5. Updated `# VERIFY:` marker status

| Marker location | Status | Resolution |
|---|---|---|
| `translate_hermes.py:153` (reasoning location) | ✓ VERIFIED | Top-level `reasoning` string field is canonical. Anthropic content blocks never persist. |
| `translate_hermes.py:209` (tool_calls shape) | ✓ VERIFIED | OpenAI shape always. Anthropic shape branch is dead. |
| `translate_hermes.py:330` (tool message keys) | ✓ VERIFIED | Use `tool_call_id` (not `id`). `tool_name` may be present but is optional; do NOT alias `name`. |
| `translate_hermes.py:350` (system-injected tagging) | ○ UNRESOLVED — RUNTIME (degraded) | No special tag exists. Content sniffing required if differentiation matters. Out of scope for v1. |
| `plugin_sketch.py:27` (hook signatures) | ✓ VERIFIED | All call sites read; see §2 table. |
| `plugin_sketch.py:165` (`on_session_start` kwargs) | ✗ CONTRADICTED | `system_prompt` and `tools` are NOT passed. Must capture from `pre_llm_call.conversation_history[0]` instead. |
| `plugin_sketch.py:191` (`post_llm_call` kwargs) | ✗ CONTRADICTED | Actual kwarg is `assistant_response`, not `response_message`. |
| `plugin_sketch.py:209` (`post_tool_call` kwargs) | ✗ CONTRADICTED | No `is_error` kwarg. Must infer from result content if needed. |
| `plugin_sketch.py:234` (`pre_user_message` hook existence) | ✓ VERIFIED — does NOT exist | Use `pre_llm_call.user_message` instead. |
| `recall_tool_sketch.py:22` (OpenStory endpoint paths) | ✓ VERIFIED previously | Endpoints exist in `rs/server/src/router.rs`. Not re-verified in this pass — out of scope. |

---

## 6. What still requires runtime verification

After this static pass, the following claims **cannot be confirmed without booting a real Hermes session**:

1. **The exact ISO-8601 format Hermes uses for `session_start` / `last_updated`** — the source uses `datetime.now().isoformat()` (no Z suffix, no timezone awareness in the default). The translator's expected `2026-04-08T14:00:00Z` format would not match. We need to read one real timestamp to confirm whether Hermes uses `+00:00`, no offset, microseconds, etc.
2. **Whether `tool_name` actually appears on stored tool result messages** — the gateway DB writer reads it, the docstring claims it, but no test in the cloned repo writes it. Possible the field is set in a code path we haven't found, or possible it's only set by older code paths and is silently absent in current runs.
3. **The exact text content of compression-summary system messages** — `_session_messages` may contain system messages with auto-generated content, and we'd want to know the prefix (e.g., `"[Compressed]"` or `"### Conversation summary"`) so a future detector can pattern-match them.
4. **Whether `reasoning_per_turn` (from `AgentResult`) ever differs from per-message `reasoning` fields** — if it does, the prototype is missing reasoning for some turns.
5. **End-to-end smoke test** — whether the plugin actually runs cleanly inside a Hermes process with `pip install hermes-openstory` and `register(ctx)` is auto-discovered.

Items 1–3 can be answered by capturing a single real session log file. Items 4–5 require actually running the plugin inside Hermes.

---

## 7. Architecture revision recommended

Two findings push the integration architecture toward something simpler than the brief originally described.

### 7.1 The plugin can be turn-scoped, not call-scoped

Original prototype: hooks `post_llm_call` AND `post_tool_call` AND `on_session_start` AND `on_session_finalize`, with each hook independently maintaining a per-session writer that appends one Hermes-native event line per call.

**Simpler design enabled by `post_llm_call.conversation_history`:** A single hook that runs once per LLM turn and emits *all new messages* since the previous call. The `conversation_history` parameter is a complete diff source — compare the current list against the previously-seen list, emit anything new. This collapses three message-capture hooks into one and eliminates the user-prompt-capture problem entirely.

The session lifecycle hooks (`on_session_start`, `on_session_finalize`, `on_session_reset`) stay as-is for opening and closing the writer.

### 7.2 The translator's input shape can be simpler

Original prototype: a small envelope (`session_id`, `event_seq`, `timestamp`, `source`, ...) wrapping each Hermes-native event.

**Simpler now that we know the ground truth:** The Hermes message dicts can flow into OpenStory unmodified. The envelope only needs `session_id` and a timestamp at the top level — Hermes already provides everything else inside the message dict. If we keep the envelope minimal (just `{session_id, timestamp, message: <hermes-native>}`), the plugin gets simpler AND the translator's job becomes more obviously "this is a Hermes message; produce CloudEvents."

Recommend updating `example_hermes_events.jsonl` to reflect this simpler envelope as part of Phase 2.

---

## 8. What this verification has changed about the plan

**Things we can do now that we couldn't before:**

- Update `translate_hermes.py` to use the verified shapes (Phase 2)
- Update `plugin_sketch.py` to use the correct hook kwargs (Phase 2)
- Port to `rs/core/src/translate_hermes.rs` with confidence (Phase 3)
- Write tests against synthetic-but-shape-accurate fixtures (Phase 2-3)

**Things still gated on a real Hermes session:**

- End-to-end smoke test
- The exact timestamp format (item 1 above)
- Confirming the `tool_name` discrepancy (item 2 above)
- Building and shipping the standalone PyPI package (its tests need real-process validation)

**Things this verification revealed but the brief didn't anticipate:**

- The session log is a snapshot, not append-only — changes ingestion strategy
- The internal storage shape is *always* OpenAI — eliminates one full code branch
- Two feedback hooks exist (`pre_llm_call` return value and `inject_message`) — must be deliberately not-used to preserve the listener thesis
- A simpler turn-scoped plugin design becomes possible
- `on_session_end` is a separate hook from `on_session_finalize` with useful per-turn-loop kwargs (`completed`, `interrupted`)

**The biggest single architectural finding:** The plugin path is *more* important than the brief suggested, not less. The disk format (snapshot rewritten per turn) is structurally incompatible with OpenStory's append-only watcher, so the plugin's "emit append-only JSONL into a watched directory" path is the only sensible v1 ingestion strategy. The fallback "just read the existing session JSON" path the brief mentioned would require a new snapshot-diff watcher mode that doesn't exist in OpenStory today.

---

*Verified by: Claude Opus 4.6 (1M context), 2026-04-08, against `~/projects/hermes-agent` at commit `6e3f7f36`. Method: pure static reads; no Hermes runtime; no real session log captured.*
