# Hermes Integration — Runtime Gaps

*What still needs a real Hermes session before the integration can ship.*

**Date:** 2026-04-08
**Hermes commit verified against (static):** `6e3f7f36`
**Status:** Phase 4 of the simple-first-pass plan. Phases 1–3 are complete; this document is the explicit list of what is *not* done and *cannot* be done from source-reading alone.

---

## How to read this document

Each gap below is a specific runtime claim that the static verification pass could not confirm. Each gap has:

- **What we'd need to learn** — the precise question
- **How to learn it** — the smallest experiment that would answer it
- **What to update once known** — the file:line that would change

The goal is that "boot one Hermes session, run one task" closes most of these in well under an hour.

---

## Gap 1 — Timestamp format

**The claim:** OpenStory's translators expect ISO-8601 timestamps in the form `2026-04-08T14:00:00Z`. The Python prototype's example fixture uses this. The Rust translator at `rs/core/src/translate_hermes.rs:413` simply passes through whatever the envelope provides.

**What we don't know:** The actual format Hermes writes. Inspecting `run_agent.py:2455-2456` shows `datetime.now().isoformat()` — Python's default for naive datetimes is something like `2026-04-08T14:00:00.123456` (no `Z`, no `+00:00`, microseconds included). For tz-aware datetimes it's `2026-04-08T14:00:00+00:00`. The plugin we're proposing would generate its OWN timestamps via `time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())` so this is not a problem for the *plugin* path — but it IS a problem for any future "read existing session_*.json snapshot" backfill path.

**Experiment:** Open one `~/.hermes/sessions/session_*.json` and read the `last_updated` field.

**What to update:** Add a timestamp normalizer to `translate_hermes.rs` that handles whatever Hermes actually emits, OR document that the backfill path requires the plugin's normalized timestamps and snapshots are not directly ingestible.

---

## Gap 2 — Whether `tool_name` is actually present on tool result messages

**The claim:** `tool_name` is **optional** on tool result messages — `gateway/session.py:957` reads `message.get("tool_name")` defensively, but `tests/agent/test_anthropic_adapter.py:590` shows fixtures without it.

**What we don't know:** Whether real Hermes runs include `tool_name` on tool messages, or whether it's a vestigial field that's never set in practice. If it's never set, the prototype's "either tool_call_id or tool_name suffices to identify the tool" assumption is wrong and the plugin would always need to look up the tool name from the preceding tool_use message.

**Experiment:** Run any session with one tool call. Read the resulting `~/.hermes/sessions/session_*.json` and find the message with `"role": "tool"`. Check whether `tool_name` is a key.

**What to update:**
- `rs/core/src/translate_hermes.rs:357` — the build_tool_result function reads `tool_name`. If it's never set, add a "look back at preceding tool_call to recover the name" pass to the translator (or to a downstream patterns consumer).
- `docs/research/hermes-integration/SOURCE_VERIFICATION.md §4.1` — add a verified note one way or the other.

---

## Gap 3 — The exact text content of compression-summary system messages

**The claim:** Hermes injects synthetic system messages for compression summaries and todo snapshots, distinguishable only by content (not by a separate field). The prototype groups them all under `system.injected.other`.

**What we don't know:** What the actual prefix text is. If it's something predictable like `"### Compressed conversation summary"` or `"[Compression checkpoint]"`, a content sniffer in the translator could classify them more precisely (`system.injected.compression`, `system.injected.todo`) so OpenStory's pattern detector can recognize them as semantically distinct.

**Experiment:** Force a compression in one Hermes session by running enough turns to hit the context window (or by setting a tiny context budget). Read the resulting `_session_messages` JSON and find the system message that didn't exist in earlier turns.

**What to update:**
- `rs/core/src/translate_hermes.rs:382` — `build_system_injected` could pattern-match content and emit different subtypes.
- `docs/research/hermes-integration/translate_hermes.py:347-360` (`_make_system_injected`) for the Python prototype.

This is **not load-bearing for v1.** The single `system.injected.other` subtype works; sub-classification is a polish improvement.

---

## Gap 4 — Whether `reasoning_per_turn` and per-message `reasoning` ever differ

**The claim:** `AgentResult` has a `reasoning_per_turn: List[str]` field (per `tests/run_agent/test_agent_loop_tool_calling.py:504`) and individual assistant messages also have a `reasoning` field. The prototype assumes they're the same.

**What we don't know:** Whether `reasoning_per_turn[i]` is exactly equal to `messages[i].reasoning` for each assistant turn, or whether one is a superset of the other. If they differ, the plugin (which captures from `assistant_response`) might be missing reasoning for some turns.

**Experiment:** Run one session that exercises an o1-style provider (OpenAI o1, Anthropic with thinking, or anything that produces reasoning blocks). Compare `result.messages[i].reasoning` to `result.reasoning_per_turn[i]` for each assistant message.

**What to update:** If they differ, the plugin's `_on_post_llm_call` may need to emit two events per turn (the message + its `reasoning_per_turn` entry separately) — though this would also require knowing where Hermes makes `reasoning_per_turn` available to plugin hooks (it's in `AgentResult`, not in any current hook kwarg).

This is **lower priority** than gaps 1–3. The simple case (assistant.reasoning is the only reasoning) is already handled correctly.

---

## Gap 5 — User-message capture in pre_llm_call

**The claim:** The plugin captures user prompts via `pre_llm_call.user_message`, which is verified to exist as a kwarg at `run_agent.py:7180`.

**What we don't know:**
- Whether `user_message` is always a string, or whether gateway-mode messages with attachments arrive as a structured object (the plugin currently does `str(user_message or "")` as a defensive coercion).
- Whether `pre_llm_call` fires once per user turn (expected) or once per LLM call within a turn (which would cause duplicate user-prompt CloudEvents — the agent loop calls the LLM multiple times within a single user turn when tool calls are involved).

**Experiment:** Add a print statement to a test plugin's `_on_pre_llm_call` and run a 5-turn session that exercises both text-only and tool-using turns. Count the firings.

**What to update:**
- If `pre_llm_call` fires per LLM call rather than per user turn, the plugin needs to track `is_first_turn` plus message-list cardinality to dedupe — or switch to a single-source pattern that diffs `conversation_history` against the previously-seen list.
- `docs/research/hermes-integration/plugin_sketch.py:_on_pre_llm_call` would need a dedup guard.

---

## Gap 6 — Plugin lifecycle in long-lived gateway sessions

**The claim:** The plugin opens a `_SessionWriter` on first event and closes it on `on_session_finalize`. In gateway mode, sessions can persist for days across many `on_session_end` events (one per agent loop run within the session).

**What we don't know:** Whether the writer survives idle periods cleanly (file handles can be reaped), whether the gateway's session reset semantics actually fire `on_session_finalize` reliably, and whether multi-agent gateway mode (Slack + Telegram simultaneously, all hitting the same plugin singleton) needs locking beyond the existing `threading.Lock` per writer.

**Experiment:** Run a gateway session, send messages to it from two different platforms simultaneously, then `/new` it. Confirm both sets of messages land in the right per-session files and the writers close cleanly.

**What to update:** Possibly nothing — the threading.Lock per writer already handles concurrent writes within a single session. The cross-session case is handled by the writer registry. The risk is a race condition in registry creation that we haven't seen because we haven't run it.

---

## Gap 7 — Snapshot ingestion is structurally incompatible with OpenStory's watcher

**The claim:** Hermes writes `~/.hermes/sessions/session_{id}.json` as a whole-file snapshot rewritten on every turn (`run_agent.py:_save_session_log`). OpenStory's existing file watcher uses byte-offset incremental reads built for append-only JSONL.

**This is not a runtime gap — it's a verified architectural finding.** It belongs here because it shapes the integration plan: the plugin path is the *only* path that doesn't require new OpenStory watcher infrastructure. A future "backfill from existing snapshots" feature would need a new snapshot-diff mode in the watcher — non-trivial work that should not block v1.

**What to update:** `docs/BACKLOG.md` — add an entry for "snapshot watcher mode" if backfill becomes a priority.

---

## Gap 8 — End-to-end smoke test

**The claim:** Once the plugin is installed via `pip install hermes-openstory` and Hermes is started, the end-to-end loop works:

1. Hermes auto-discovers the plugin via the `hermes_agent.plugins` entry point
2. The plugin's hooks fire and write JSONL into the watched directory
3. OpenStory's watcher picks up the lines and routes them through `translate_hermes.rs`
4. CloudEvents flow through the existing pipeline and appear in the dashboard
5. The recall tool, registered through the plugin, returns structured answers from OpenStory's REST API

**What we don't know:** Whether *any* of step 1, 2, 3, 4, or 5 actually works in practice. Each piece has been verified statically; none have been verified together.

**Experiment:** This is the v0.1.0 ship gate. Build the standalone `hermes-openstory` package, install it into a Hermes environment, run a session, watch the dashboard, call the recall tool from inside the agent.

**What to update:** Everything in this folder gets a "VERIFIED END-TO-END" stamp once this passes.

---

## Gap summary

| # | Gap | Severity | Effort to close (with real Hermes) |
|---|---|---|---|
| 1 | Timestamp format | Low (only matters for backfill, plugin path is fine) | 30 seconds |
| 2 | `tool_name` presence | Medium (translator behavior may differ) | 30 seconds |
| 3 | Compression-summary content prefix | Low (polish, not load-bearing) | 1 turn that hits compression |
| 4 | reasoning_per_turn vs per-message reasoning | Low | 1 thinking-enabled provider session |
| 5 | pre_llm_call firing cardinality | Medium (could cause duplicates) | 1 session with 5 turns |
| 6 | Plugin lifecycle in long-lived gateway sessions | Low (existing locking should suffice) | Multi-day gateway test |
| 7 | Snapshot ingestion mismatch | None — already resolved (not pursuing snapshot path) | N/A |
| 8 | End-to-end smoke test | **High** (the v0.1.0 ship gate) | ~30 minutes if Hermes boots cleanly |

**Total estimated runtime work after a Hermes container is available: ~1 hour for gaps 1–6, plus ~30 minutes for gap 8 (the smoke test).** The real cost is getting a Hermes container running for the first time, which we have not done yet and which is the actual blocker.
