"""
recall_tool_sketch.py — A Hermes tool that wraps OpenStory's REST API.

This is the read path of self-reflection. The Hermes agent calls `recall`
mid-conversation to query OpenStory for structural views of its own past
work — sentence diagrams, file impact, errors, synopses — and gets a
deterministic, structured answer instead of an LLM summary of its own raw
context.

The tool is registered via the hermes-openstory plugin (see
plugin_sketch.py). It fits the standard Hermes tool surface:

    {
      "name": "recall",
      "description": "...",
      "parameters": {... JSONSchema ...}
    }

with a handler function that returns a JSON string (Hermes's tool result
convention). The handler is pure HTTP — no shared state, no callbacks.

# VERIFY: the OpenStory endpoint paths below are taken from
# rs/server/src/router.rs:95-200 as of 2026-04-08. Confirm against the
# running server before publishing the package.
"""

from __future__ import annotations

import json
import os
import urllib.parse
import urllib.request
from typing import Any, Dict, Optional


# ─────────────────────────────────────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────────────────────────────────────

DEFAULT_OPENSTORY_URL = os.environ.get(
    "OPENSTORY_API_URL", "http://localhost:3002"
)
DEFAULT_TIMEOUT_S = 5.0


# ─────────────────────────────────────────────────────────────────────────────
# Tool schema (OpenAI function-calling shape, the way Hermes expects)
# ─────────────────────────────────────────────────────────────────────────────

RECALL_TOOL_SCHEMA: Dict[str, Any] = {
    "name": "recall",
    "description": (
        "Query your own past work via OpenStory. Use this to look up what "
        "happened in earlier sessions — what files you touched, what tools "
        "you used, what errors occurred, what the user asked for. Returns "
        "structured data, not an LLM summary. Prefer this over re-reading "
        "raw conversation history when you need to recall what you did."
    ),
    "parameters": {
        "type": "object",
        "properties": {
            "query_type": {
                "type": "string",
                "enum": [
                    "list_sessions",
                    "session_synopsis",
                    "session_patterns",
                    "session_file_impact",
                    "session_errors",
                    "session_tool_journey",
                    "search",
                    "recent_files",
                ],
                "description": (
                    "Which kind of query to run. "
                    "list_sessions: list recent sessions for a project. "
                    "session_synopsis: natural-language summary of a session. "
                    "session_patterns: sentence diagrams of every turn in a session. "
                    "session_file_impact: which files were touched in a session. "
                    "session_errors: errors that occurred in a session. "
                    "session_tool_journey: the chronological sequence of tool calls. "
                    "search: full-text search across all sessions. "
                    "recent_files: files the agent has been working with recently."
                ),
            },
            "session_id": {
                "type": "string",
                "description": "Required for session_* query types. The OpenStory session ID to query.",
            },
            "project": {
                "type": "string",
                "description": "Required for list_sessions and recent_files. The project name as OpenStory knows it.",
            },
            "search_query": {
                "type": "string",
                "description": "Required for the 'search' query type. The text to search for.",
            },
            "limit": {
                "type": "integer",
                "description": "Optional: max number of results to return. Defaults to 10.",
                "default": 10,
            },
        },
        "required": ["query_type"],
    },
}


# ─────────────────────────────────────────────────────────────────────────────
# Handler — the function Hermes will call when the tool is invoked
# ─────────────────────────────────────────────────────────────────────────────


def recall_handler(
    query_type: str,
    session_id: Optional[str] = None,
    project: Optional[str] = None,
    search_query: Optional[str] = None,
    limit: int = 10,
    _base_url: str = DEFAULT_OPENSTORY_URL,
    **_extra: Any,
) -> str:
    """Dispatch to the right OpenStory endpoint and return a JSON string.

    Hermes tool handlers conventionally return a JSON-encoded string
    (the agent loop sees it as the tool result text). On error we return
    a JSON object with `success: false` and an error message — the model
    can then decide whether to retry or take a different path.
    """
    try:
        if query_type == "list_sessions":
            if not project:
                return _err("project is required for list_sessions")
            data = _get(_base_url, "/api/sessions", {"project": project, "limit": limit})
            return _ok(data)

        if query_type == "session_synopsis":
            if not session_id:
                return _err("session_id is required for session_synopsis")
            data = _get(_base_url, f"/api/sessions/{session_id}/synopsis")
            return _ok(data)

        if query_type == "session_patterns":
            if not session_id:
                return _err("session_id is required for session_patterns")
            data = _get(
                _base_url,
                f"/api/sessions/{session_id}/patterns",
                {"type": "turn.sentence"},
            )
            return _ok(data)

        if query_type == "session_file_impact":
            if not session_id:
                return _err("session_id is required for session_file_impact")
            data = _get(_base_url, f"/api/sessions/{session_id}/file-impact")
            return _ok(data)

        if query_type == "session_errors":
            if not session_id:
                return _err("session_id is required for session_errors")
            data = _get(_base_url, f"/api/sessions/{session_id}/errors")
            return _ok(data)

        if query_type == "session_tool_journey":
            if not session_id:
                return _err("session_id is required for session_tool_journey")
            data = _get(_base_url, f"/api/sessions/{session_id}/tool-journey")
            return _ok(data)

        if query_type == "search":
            if not search_query:
                return _err("search_query is required for search")
            data = _get(_base_url, "/api/search", {"q": search_query, "limit": limit})
            return _ok(data)

        if query_type == "recent_files":
            if not project:
                return _err("project is required for recent_files")
            data = _get(_base_url, "/api/agent/recent-files", {"project": project})
            return _ok(data)

        return _err(f"unknown query_type: {query_type}")

    except urllib.error.URLError as e:
        return _err(
            f"OpenStory unreachable at {_base_url}: {e}. "
            f"Is OpenStory running? (`just up` in the openstory repo)"
        )
    except Exception as e:
        return _err(f"recall failed: {type(e).__name__}: {e}")


# ─────────────────────────────────────────────────────────────────────────────
# HTTP helpers
# ─────────────────────────────────────────────────────────────────────────────


def _get(base: str, path: str, params: Optional[Dict[str, Any]] = None) -> Any:
    url = base.rstrip("/") + path
    if params:
        url = url + "?" + urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=DEFAULT_TIMEOUT_S) as resp:
        body = resp.read().decode("utf-8")
    return json.loads(body)


def _ok(result: Any) -> str:
    return json.dumps({"success": True, "result": result})


def _err(msg: str) -> str:
    return json.dumps({"success": False, "error": msg})
