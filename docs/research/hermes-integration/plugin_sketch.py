"""
plugin_sketch.py — The hermes-openstory plugin scaffolding.

This file is the executable spec for the standalone third-party package
`hermes-openstory`. When installed via `pip install hermes-openstory`,
Hermes auto-discovers it via the `hermes_agent.plugins` entry-point group
(see `hermes_cli/plugins.py:68` in the hermes-agent repo) and calls
`register(ctx)` at startup.

The plugin does three things:

  1. Registers the `recall` tool — wrapper around OpenStory's REST API,
     so the agent can query its own past work mid-conversation.
     (See recall_tool_sketch.py for the tool's implementation.)

  2. Hooks `post_llm_call` and `post_tool_call` to capture every event
     the agent loop emits, and writes them as Hermes-native JSONL into
     a configurable output directory (default: ~/.hermes/openstory-events/).

  3. Hooks `on_session_finalize` to write a session_end marker that lets
     OpenStory's translator emit a `system.turn.complete` CloudEvent.

OpenStory's existing file watcher picks up the JSONL, recognizes the
hermes-events path pattern, and routes the lines through `translate_hermes.rs`
(the production Rust port of `translate_hermes.py`).

# VERIFIED 2026-04-08 against hermes-agent commit 6e3f7f36
# (see SOURCE_VERIFICATION.md §1 and §2). Every hook signature below is
# now confirmed from a specific `_invoke_hook(...)` call site:
#
#   on_session_start    — run_agent.py:7089  (session_id, model, platform)
#   pre_llm_call        — run_agent.py:7180  (session_id, user_message,
#                                              conversation_history,
#                                              is_first_turn, model, platform)
#   pre_api_request     — run_agent.py:7421  (task_id, session_id, platform,
#                                              model, provider, base_url, ...)
#   post_api_request    — run_agent.py:8600  (same as pre_api_request)
#   post_llm_call       — run_agent.py:9203  (session_id, user_message,
#                                              assistant_response,
#                                              conversation_history,
#                                              model, platform)
#   on_session_end      — run_agent.py:9302  (session_id, completed,
#                                              interrupted, model, platform)
#   pre_tool_call       — model_tools.py:503 (tool_name, args, task_id,
#                                              session_id, tool_call_id)
#   post_tool_call      — model_tools.py:532 (tool_name, args, result,
#                                              task_id, session_id,
#                                              tool_call_id)
#   on_session_finalize — cli.py:617, gateway/run.py:1487
#                                             (session_id, platform)
#   on_session_reset    — gateway/run.py     (session_id, platform)
#
# Three corrections from the original prototype, all forced by these
# verified signatures:
#
#   1. on_session_start does NOT pass system_prompt or tools as kwargs.
#      To capture them, the plugin must read conversation_history[0] on
#      the first pre_llm_call (where the system message lives).
#
#   2. post_llm_call's response kwarg is `assistant_response`, not
#      `response_message`. Bug fixed below.
#
#   3. post_tool_call has NO `is_error` kwarg. The plugin can infer
#      errors from `result` content if it cares; for OpenStory's
#      pipeline this is not currently load-bearing.
#
# A fourth, structural finding (see SOURCE_VERIFICATION.md §2):
# pre_llm_call AND post_llm_call BOTH pass conversation_history — the
# full Hermes-native message list. This means the plugin can capture
# everything by hooking just post_llm_call and diffing the conversation
# history against its previous view. The per-tool_call hooks are
# redundant for capture purposes (the tool result will appear in the
# next pre_llm_call's history). They are kept here for two reasons:
# (a) lower-latency emission, and (b) the prototype is staying close
# to its original shape so the diff against the brief is reviewable.
# A v2 plugin should consider collapsing to a single hook.
#
# CRITICAL NON-USE FLAG (see SOURCE_VERIFICATION.md §1.4):
# pre_llm_call callbacks may RETURN a context dict that Hermes will
# inject into the user message. PluginContext.inject_message() at
# hermes_cli/plugins.py:164 also exists and lets plugins interrupt
# the agent. BOTH ARE FEEDBACK PATHS INTO THE COALGEBRA. The OpenStory
# plugin must keep all hooks returning None and must never call
# inject_message. This preserves the algebra/coalgebra purity that
# LISTENER_AS_ALGEBRA.md is built on.
"""

from __future__ import annotations

import json
import os
import threading
import time
from pathlib import Path
from typing import Any, Dict, Optional


# Local import — in the published package these would be a sibling module.
try:
    from recall_tool_sketch import RECALL_TOOL_SCHEMA, recall_handler
except ImportError:
    # When run from outside the prototype directory, fall back gracefully.
    RECALL_TOOL_SCHEMA = {}
    recall_handler = None  # type: ignore[assignment]


# ─────────────────────────────────────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────────────────────────────────────


def _output_dir() -> Path:
    """Where the plugin writes Hermes-native event JSONL.

    Configurable via env var; defaults to ~/.hermes/openstory-events/.
    OpenStory's file watcher should be configured to watch this directory.
    """
    raw = os.environ.get(
        "HERMES_OPENSTORY_OUTPUT_DIR",
        str(Path.home() / ".hermes" / "openstory-events"),
    )
    p = Path(raw).expanduser()
    p.mkdir(parents=True, exist_ok=True)
    return p


# ─────────────────────────────────────────────────────────────────────────────
# Per-session writer
# ─────────────────────────────────────────────────────────────────────────────


class _SessionWriter:
    """Append-only JSONL writer for one Hermes session.

    Threadsafe (the lock matters because Hermes hooks can fire from
    different threads in gateway mode). Lazy file open, monotonic
    sequence number per session.
    """

    def __init__(self, session_id: str, out_dir: Path):
        self.session_id = session_id
        self.path = out_dir / f"{session_id}.jsonl"
        self._seq = 0
        self._lock = threading.Lock()
        self._fh = None  # type: ignore[assignment]

    def _ensure_open(self) -> None:
        if self._fh is None:
            self._fh = open(self.path, "a", encoding="utf-8")

    def append(self, event_type: str, data: Dict[str, Any], **envelope_extra: Any) -> None:
        with self._lock:
            self._ensure_open()
            self._seq += 1
            line = {
                "envelope": {
                    "session_id": self.session_id,
                    "event_seq": self._seq,
                    "timestamp": _iso_now(),
                    "source": "hermes",
                    **envelope_extra,
                },
                "event_type": event_type,
                "data": data,
            }
            self._fh.write(json.dumps(line, separators=(",", ":")) + "\n")
            self._fh.flush()

    def close(self) -> None:
        with self._lock:
            if self._fh is not None:
                self._fh.close()
                self._fh = None


# Global writer registry, keyed by session_id. Created on first event for a
# given session; closed on session_finalize.
_writers: Dict[str, _SessionWriter] = {}
_writers_lock = threading.Lock()


def _writer_for(session_id: str) -> _SessionWriter:
    with _writers_lock:
        if session_id not in _writers:
            _writers[session_id] = _SessionWriter(session_id, _output_dir())
        return _writers[session_id]


def _close_writer(session_id: Optional[str]) -> None:
    if not session_id:
        return
    with _writers_lock:
        w = _writers.pop(session_id, None)
    if w is not None:
        w.close()


# ─────────────────────────────────────────────────────────────────────────────
# Hook callbacks
# ─────────────────────────────────────────────────────────────────────────────
#
# All hook callbacks must be defensive — they run inside the Hermes agent
# loop and must NEVER raise. The plugin manager wraps callbacks in try/except
# (see hermes_cli/plugins.py:_invoke_hook), but we add an extra layer because
# observability code that crashes the host process is the worst kind of
# observability code.


def _on_session_start(
    session_id: Optional[str] = None,
    model: Optional[str] = None,
    platform: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFIED 2026-04-08: run_agent.py:7089 passes ONLY (session_id, model,
    # platform). The previously expected `system_prompt` and `tools` kwargs
    # are NOT passed. To capture them the plugin would have to read
    # conversation_history[0] on the first pre_llm_call (where the system
    # message lives). For now we emit the session_start event with what we
    # have; an enrichment pass on the first pre_llm_call could fill in the
    # missing fields. # RUNTIME: confirm the conversation_history[0] shape
    # against a real session.
    try:
        if not session_id:
            return
        w = _writer_for(session_id)
        w.append(
            "session_start",
            {
                "system_prompt_preview": "",  # see comment above
                "tools": [],                  # see comment above
            },
            model=model or "",
            platform=platform or "",
        )
    except Exception:
        pass


def _on_post_llm_call(
    session_id: Optional[str] = None,
    user_message: Optional[Any] = None,
    assistant_response: Optional[Dict[str, Any]] = None,
    conversation_history: Optional[list] = None,
    model: Optional[str] = None,
    platform: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFIED 2026-04-08: run_agent.py:9203 passes
    # (session_id, user_message, assistant_response, conversation_history,
    #  model, platform).
    #
    # The original sketch expected `response_message` — that was a guess
    # and is wrong. The correct kwarg name is `assistant_response`.
    #
    # We also receive `conversation_history` which is the FULL Hermes-native
    # message list including the brand-new assistant_response. This is the
    # ground truth for the per-turn message stream and lets future versions
    # of this plugin capture everything from a single hook by diffing
    # against the previous view (see SOURCE_VERIFICATION.md §7.1).
    #
    # For now we keep the simpler "emit just the new assistant message"
    # behavior that the prototype shipped with — diffing against
    # conversation_history is left for the v2 plugin once we have real
    # data to validate against.
    try:
        if not session_id or not assistant_response:
            return
        _writer_for(session_id).append("message", assistant_response)
    except Exception:
        pass


def _on_post_tool_call(
    tool_name: Optional[str] = None,
    args: Optional[Dict[str, Any]] = None,
    result: Optional[str] = None,
    task_id: Optional[str] = None,
    session_id: Optional[str] = None,
    tool_call_id: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFIED 2026-04-08: model_tools.py:532 passes
    # (tool_name, args, result, task_id, session_id, tool_call_id).
    #
    # The original sketch expected `is_error` — that does NOT exist as a
    # kwarg. If error detection becomes important, infer from `result`
    # content (e.g., string-prefix match on "Error:") or read the next
    # `pre_llm_call`'s `conversation_history` to see how the model
    # framed the result. For v1 this is not load-bearing.
    #
    # `args` and `task_id` were not in the original sketch but are
    # passed by Hermes — captured here for completeness (args could be
    # useful as fallback if the assistant tool_use message is somehow
    # lost; task_id matters in subagent contexts).
    try:
        if not session_id:
            return
        _writer_for(session_id).append(
            "message",
            {
                "role": "tool",
                "tool_call_id": tool_call_id or "",
                "tool_name": tool_name or "",
                "content": result or "",
                # Note: no `is_error` field — verified absent in canonical
                # Hermes shape (see SOURCE_VERIFICATION.md §4.1).
            },
        )
    except Exception:
        pass


def _on_pre_llm_call(
    session_id: Optional[str] = None,
    user_message: Optional[Any] = None,
    conversation_history: Optional[list] = None,
    is_first_turn: bool = False,
    model: Optional[str] = None,
    platform: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFIED 2026-04-08: run_agent.py:7180 passes
    # (session_id, user_message, conversation_history, is_first_turn,
    #  model, platform).
    #
    # This hook REPLACES the prototype's missing `_on_user_message` hook
    # entirely. There is no `pre_user_message` in Hermes (see
    # SOURCE_VERIFICATION.md §1.1 — VALID_HOOKS does not include one);
    # `pre_llm_call` is the right place to capture user prompts because
    # it fires immediately before each LLM call, and the parameter
    # `user_message` IS the user-input string for that turn.
    #
    # IMPORTANT: this callback MUST return None. Hermes interprets a
    # non-None return value from a pre_llm_call hook as context to inject
    # into the user message (see SOURCE_VERIFICATION.md §1.4). Returning
    # None preserves the listener-as-algebra purity.
    try:
        if not session_id:
            return
        # user_message may be a string (CLI mode) or a structured object
        # (gateway mode with attachments). For v1 we serialize either.
        text = user_message if isinstance(user_message, str) else str(user_message or "")
        _writer_for(session_id).append(
            "message",
            {"role": "user", "content": text},
        )
    except Exception:
        pass


def _on_session_end(
    session_id: Optional[str] = None,
    completed: bool = False,
    interrupted: bool = False,
    model: Optional[str] = None,
    platform: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFIED 2026-04-08: run_agent.py:9302 passes
    # (session_id, completed, interrupted, model, platform).
    #
    # This is a new hook the original prototype didn't know about. It
    # fires at the end of EACH agent loop run with `completed` and
    # `interrupted` booleans. Distinct from on_session_finalize, which
    # fires at session boundaries (exit, /new, /reset).
    #
    # We emit a per-loop-run "turn complete" marker so OpenStory's
    # patterns layer can see the loop-end state. The writer stays open
    # because the session may continue after this in CLI mode.
    try:
        if not session_id:
            return
        _writer_for(session_id).append(
            "session_end",  # event_type — translator emits system.turn.complete
            {
                "reason": "completed" if completed else ("interrupted" if interrupted else "unknown"),
                "completed": bool(completed),
                "interrupted": bool(interrupted),
            },
        )
    except Exception:
        pass


def _on_session_finalize(
    session_id: Optional[str] = None,
    platform: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # Verified: gateway/run.py:1487 and cli.py:617 invoke_hook signatures.
    try:
        if not session_id:
            return
        w = _writer_for(session_id)
        w.append("session_end", {"reason": "finalize"})
        _close_writer(session_id)
    except Exception:
        pass


def _on_session_reset(
    session_id: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    try:
        _close_writer(session_id)
    except Exception:
        pass


# ─────────────────────────────────────────────────────────────────────────────
# Plugin entry point
# ─────────────────────────────────────────────────────────────────────────────


def register(ctx: Any) -> None:
    """Hermes plugin entry point.

    Called by the Hermes plugin manager (`PluginManager._load_plugin`) after
    discovering this package via the `hermes_agent.plugins` entry-point group.
    `ctx` is a `PluginContext` (see `hermes_cli/plugins.py:124`) — it exposes
    `register_tool()`, `register_hook()`, and `register_cli_command()`.
    """

    # ── Register the `recall` tool ──
    if recall_handler is not None and RECALL_TOOL_SCHEMA:
        ctx.register_tool(
            name="recall",
            toolset="memory",
            schema=RECALL_TOOL_SCHEMA,
            handler=recall_handler,
            description="Query OpenStory for structural views of past sessions.",
        )

    # ── Register lifecycle hooks ──
    # All hooks below have signatures verified against hermes-agent 6e3f7f36
    # on 2026-04-08 (see SOURCE_VERIFICATION.md §2). Each callback returns
    # None — that is required for pre_llm_call to preserve the
    # algebra/coalgebra split (a non-None return injects context into the
    # user message). Even though only pre_llm_call has this contract, we
    # follow the rule uniformly so a future maintainer doesn't accidentally
    # promote one of the other callbacks to a feedback path.
    ctx.register_hook("on_session_start", _on_session_start)
    ctx.register_hook("pre_llm_call", _on_pre_llm_call)
    ctx.register_hook("post_llm_call", _on_post_llm_call)
    ctx.register_hook("post_tool_call", _on_post_tool_call)
    ctx.register_hook("on_session_end", _on_session_end)
    ctx.register_hook("on_session_finalize", _on_session_finalize)
    ctx.register_hook("on_session_reset", _on_session_reset)


# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _iso_now() -> str:
    # ISO-8601 with seconds precision and trailing Z, matching what
    # OpenStory's existing translators emit.
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
