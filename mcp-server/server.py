"""OpenStory MCP Server — read-only agent self-awareness tools.

Wraps the OpenStory REST API (default: http://localhost:3002) as MCP tools,
letting coding agents query their own operational history.

Usage:
    # stdio transport (default, for Claude Code / IDE integration)
    uv run python server.py

    # SSE transport (for remote / browser-based clients)
    uv run python server.py --sse

    # Custom API base URL
    OPENSTORY_URL=http://myhost:3002 uv run python server.py

    # Test mode — verify all tools are registered
    uv run python server.py --test
"""

from __future__ import annotations

import json
import os
import sys
from typing import Optional

import httpx
from fastmcp import FastMCP

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

BASE_URL = os.environ.get("OPENSTORY_URL", "http://localhost:3002")
API_TOKEN = os.environ.get("OPENSTORY_API_TOKEN", "")

mcp = FastMCP(
    "OpenStory",
    instructions=(
        "OpenStory gives you visibility into your own coding sessions. "
        "Use these tools to understand what happened in past sessions, "
        "search across your history, and check token usage. "
        "All data is read-only — these tools observe, they never interfere."
    ),
)

# ---------------------------------------------------------------------------
# HTTP helper
# ---------------------------------------------------------------------------

def _client() -> httpx.Client:
    headers = {}
    if API_TOKEN:
        headers["Authorization"] = f"Bearer {API_TOKEN}"
    return httpx.Client(base_url=BASE_URL, headers=headers, timeout=30)


def _get(path: str, params: dict | None = None) -> dict | list:
    with _client() as client:
        resp = client.get(path, params=params)
        resp.raise_for_status()
        return resp.json()


# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

@mcp.tool
def list_sessions() -> str:
    """List all coding sessions with metadata.

    Returns session IDs, status, start times, event counts, tools used,
    files edited, model, duration, first prompt, project, and branch.
    Use this to find a session ID for deeper queries.
    """
    sessions = _get("/api/sessions")
    # Return a concise summary rather than the full payload
    result = []
    for s in sessions:
        result.append({
            "session_id": s.get("session_id"),
            "project": s.get("project_name", s.get("project_id")),
            "status": s.get("status"),
            "start_time": s.get("start_time"),
            "duration_ms": s.get("duration_ms"),
            "event_count": s.get("event_count"),
            "tool_calls": s.get("tool_calls"),
            "first_prompt": _truncate(s.get("first_prompt", ""), 120),
            "model": s.get("model"),
            "branch": s.get("branch"),
        })
    return json.dumps(result, indent=2)


@mcp.tool
def session_synopsis(session_id: str) -> str:
    """Get a structured overview of a single session.

    Returns event count, tool count, error count, time range,
    and the top tools used. Good first tool to call when investigating
    a specific session.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/synopsis"), indent=2)


@mcp.tool
def session_activity(session_id: str) -> str:
    """Get detailed activity for a session.

    Returns the first prompt, files touched, tool breakdown,
    error messages, last response, conversation turn count,
    plan count, duration, and start time.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/activity"), indent=2)


@mcp.tool
def tool_journey(session_id: str) -> str:
    """Get the sequence of tool calls in a session.

    Shows the chronological order of tools used and their file targets.
    Useful for understanding the workflow and approach taken.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/tool-journey"), indent=2)


@mcp.tool
def file_impact(session_id: str) -> str:
    """Get file read/write counts for a session.

    Shows which files were read and written, sorted by total operations.
    Useful for understanding which parts of the codebase were affected.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/file-impact"), indent=2)


@mcp.tool
def session_errors(session_id: str) -> str:
    """Get all errors from a session.

    Returns error records with timestamps. Useful for understanding
    what went wrong and when.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/errors"), indent=2)


@mcp.tool
def session_patterns(session_id: str, pattern_type: Optional[str] = None) -> str:
    """Get detected patterns in a session.

    Patterns include eval-apply cycles, git workflows, error recovery,
    test cycles, and detected sentences. Optionally filter by pattern type.

    Args:
        session_id: The session to query.
        pattern_type: Optional filter (e.g., "git.workflow", "error.recovery",
                      "test.cycle", "turn.sentence", "turn.phase").
    """
    params = {}
    if pattern_type:
        params["type"] = pattern_type
    return json.dumps(
        _get(f"/api/sessions/{session_id}/patterns", params=params), indent=2
    )


@mcp.tool
def search(query: str, limit: int = 10, session_id: Optional[str] = None) -> str:
    """Full-text search across all session events.

    Search for keywords, file names, error messages, or any text that
    appeared in your coding sessions. Results include session ID, event ID,
    relevance rank, and a snippet of matching content.

    Args:
        query: Search terms (supports SQLite FTS5 syntax).
        limit: Max results to return (default 10).
        session_id: Optional — scope search to a single session.
    """
    params: dict = {"q": query, "limit": limit}
    if session_id:
        params["session_id"] = session_id
    return json.dumps(_get("/api/search", params=params), indent=2)


@mcp.tool
def agent_search(
    query: str,
    project: Optional[str] = None,
    days: int = 30,
    limit: int = 5,
) -> str:
    """Natural-language search across sessions with relevance ranking.

    Higher-level than raw FTS search — groups results by session and
    ranks by relevance. Good for questions like "when did I last refactor
    the auth module?" or "sessions where I worked on the parser".

    Args:
        query: Natural language search query.
        project: Optional project ID to filter by.
        days: Look back this many days (default 30).
        limit: Max sessions to return (default 5).
    """
    params: dict = {"q": query, "days": days, "limit": limit}
    if project:
        params["project"] = project
    return json.dumps(_get("/api/agent/search", params=params), indent=2)


@mcp.tool
def project_context(project: str) -> str:
    """Get recent session context for a project.

    Returns up to 5 recent sessions with their IDs, labels, event counts,
    and time ranges. Useful for understanding recent work on a project.

    Args:
        project: Project ID (typically derived from the working directory path).
    """
    return json.dumps(
        _get("/api/agent/project-context", params={"project": project}), indent=2
    )


@mcp.tool
def recent_files(project: str) -> str:
    """Get files recently modified in a project.

    Shows which files were touched in recent sessions for this project.

    Args:
        project: Project ID.
    """
    return json.dumps(
        _get("/api/agent/recent-files", params={"project": project}), indent=2
    )


@mcp.tool
def project_pulse(days: int = 7) -> str:
    """Get activity summary across all projects.

    Shows which projects have been active, with session counts, event counts,
    and last activity timestamps. Good for a high-level overview.

    Args:
        days: Look back this many days (default 7).
    """
    return json.dumps(
        _get("/api/insights/pulse", params={"days": days}), indent=2
    )


@mcp.tool
def token_usage(
    days: Optional[int] = None,
    session_id: Optional[str] = None,
    model: str = "sonnet",
) -> str:
    """Get token usage and cost estimates.

    Shows input/output/cache token counts and estimated costs.
    Can scope to a time window, a single session, or a specific model tier.

    Args:
        days: Look back this many days (omit for all time).
        session_id: Optional — scope to a single session.
        model: Pricing tier — "sonnet", "opus", or "haiku" (default "sonnet").
    """
    params: dict = {"model": model}
    if days is not None:
        params["days"] = days
    if session_id:
        params["session_id"] = session_id
    raw = _get("/api/insights/token-usage", params=params)
    cost = raw.get("cost", {})
    result = {
        "session_count": raw.get("session_count"),
        "total_input": raw.get("input_tokens"),
        "total_output": raw.get("output_tokens"),
        "total_cache_read": raw.get("cache_read_tokens"),
        "total_cache_creation": raw.get("cache_creation_tokens"),
        "message_count": raw.get("message_count"),
        "total_tokens": raw.get("total_tokens"),
        "estimated_cost": cost.get("total"),
        "cost_breakdown": {
            "input": cost.get("input"),
            "output": cost.get("output"),
            "cache_read": cost.get("cache_read"),
            "cache_creation": cost.get("cache_creation"),
        },
        "model": cost.get("model"),
        "sessions": raw.get("sessions", []),
    }
    return json.dumps(result, indent=2)


@mcp.tool
def daily_token_usage(days: int = 7) -> str:
    """Get daily token usage trends.

    Shows per-day token counts and cost estimates. Useful for tracking
    usage patterns over time.

    Args:
        days: Number of days to include (default 7).
    """
    return json.dumps(
        _get("/api/insights/token-usage/daily", params={"days": days}), indent=2
    )


@mcp.tool
def productivity(days: int = 30) -> str:
    """Get productivity by hour of day.

    Shows event counts per hour (0-23), revealing when you're most
    active with coding agents.

    Args:
        days: Look back this many days (default 30).
    """
    return json.dumps(
        _get("/api/insights/productivity", params={"days": days}), indent=2
    )


@mcp.tool
def session_transcript(
    session_id: str, assistant_only: bool = False
) -> str:
    """Get the conversation transcript for a session.

    Returns the back-and-forth between user and assistant, including
    tool calls and results. Can filter to assistant messages only.

    Args:
        session_id: The session to retrieve.
        assistant_only: If true, only return assistant text messages.
    """
    params: dict = {}
    if assistant_only:
        params["assistant_only"] = "true"
    return json.dumps(
        _get(f"/api/sessions/{session_id}/transcript", params=params), indent=2
    )


@mcp.tool
def session_plans(session_id: str) -> str:
    """Get plans created during a session.

    Returns plan summaries with IDs, titles, and timestamps.
    Plans are structured approaches the agent laid out during work.

    Args:
        session_id: The session to query.
    """
    return json.dumps(_get(f"/api/sessions/{session_id}/plans"), indent=2)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _truncate(text: str, max_len: int) -> str:
    if len(text) <= max_len:
        return text
    return text[: max_len - 1] + "\u2026"


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

def _self_test():
    """Verify all tools are registered and the server can start."""
    import asyncio

    tools = asyncio.run(mcp.list_tools())
    registered = {t.name for t in tools}
    print(f"Registered {len(registered)} tools:")
    for name in sorted(registered):
        print(f"  - {name}")

    expected = {
        "list_sessions",
        "session_synopsis",
        "session_activity",
        "tool_journey",
        "file_impact",
        "session_errors",
        "session_patterns",
        "search",
        "agent_search",
        "project_context",
        "recent_files",
        "project_pulse",
        "token_usage",
        "daily_token_usage",
        "productivity",
        "session_transcript",
        "session_plans",
    }
    missing = expected - registered
    extra = registered - expected
    if missing:
        print(f"\nMISSING: {missing}")
        sys.exit(1)
    if extra:
        print(f"\nEXTRA (not a failure): {extra}")
    print(f"\nAll {len(expected)} expected tools registered. OK.")


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

def main():
    if "--test" in sys.argv:
        _self_test()
        return
    if "--sse" in sys.argv:
        mcp.run(transport="sse")
    else:
        mcp.run()


if __name__ == "__main__":
    main()
