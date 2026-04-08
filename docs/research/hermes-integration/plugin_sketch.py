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

# VERIFY: Hermes plugin hook signatures (the `**kwargs` shape passed by
# `invoke_hook()`) are inferred from the `invoke_hook(...)` call sites at:
#   gateway/run.py:1487  — on_session_finalize(session_id=..., platform=...)
#   cli.py:617           — on_session_finalize(session_id=..., platform=...)
# The post_llm_call and post_tool_call kwargs need to be confirmed by
# grepping their invoke_hook() call sites in the hermes-agent repo before
# the package ships. The current signatures are educated guesses.
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
    system_prompt: Optional[str] = None,
    tools: Optional[list] = None,
    **_kwargs: Any,
) -> None:
    # VERIFY: confirm the kwargs Hermes passes to on_session_start by reading
    # gateway/run.py and cli.py for the invoke_hook("on_session_start", ...)
    # call sites. This signature is a guess.
    try:
        if not session_id:
            return
        w = _writer_for(session_id)
        w.append(
            "session_start",
            {
                "system_prompt_preview": (system_prompt or "")[:500],
                "tools": tools or [],
            },
            model=model or "",
            platform=platform or "",
        )
    except Exception:
        pass


def _on_post_llm_call(
    session_id: Optional[str] = None,
    response_message: Optional[Dict[str, Any]] = None,
    **_kwargs: Any,
) -> None:
    # VERIFY: confirm post_llm_call signature. The interesting argument is
    # the assistant message dict the model just produced. Hermes likely passes
    # the full response object — find the field name and adjust.
    try:
        if not session_id or not response_message:
            return
        _writer_for(session_id).append("message", response_message)
    except Exception:
        pass


def _on_post_tool_call(
    session_id: Optional[str] = None,
    tool_name: Optional[str] = None,
    tool_call_id: Optional[str] = None,
    result: Optional[str] = None,
    is_error: bool = False,
    **_kwargs: Any,
) -> None:
    # VERIFY: confirm post_tool_call signature. The plugin needs the
    # tool_call_id so the resulting CloudEvent can be linked back to the
    # originating tool_use event.
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
                "is_error": bool(is_error),
            },
        )
    except Exception:
        pass


def _on_user_message(
    session_id: Optional[str] = None,
    content: Optional[str] = None,
    **_kwargs: Any,
) -> None:
    # VERIFY: Hermes may not have an explicit on_user_message hook. If not,
    # the plugin will need to capture user prompts via post_llm_call by
    # noting the *previous* turn's user content. For the prototype we assume
    # such a hook (or wrapper) exists.
    try:
        if not session_id or content is None:
            return
        _writer_for(session_id).append(
            "message",
            {"role": "user", "content": content},
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
    ctx.register_hook("on_session_start", _on_session_start)
    ctx.register_hook("post_llm_call", _on_post_llm_call)
    ctx.register_hook("post_tool_call", _on_post_tool_call)
    ctx.register_hook("on_session_finalize", _on_session_finalize)
    ctx.register_hook("on_session_reset", _on_session_reset)

    # Note: Hermes does not currently appear to expose a `pre_user_message`
    # hook (see VALID_HOOKS in hermes_cli/plugins.py). Capturing user inputs
    # may require attaching a `pre_llm_call` hook and reading the trailing
    # user message from the messages list. This is one of the things to
    # confirm during the verification step.


# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────


def _iso_now() -> str:
    # ISO-8601 with seconds precision and trailing Z, matching what
    # OpenStory's existing translators emit.
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
