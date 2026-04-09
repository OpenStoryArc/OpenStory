"""Smoke-test every MCP tool against a live OpenStory instance.

Requires OpenStory running on OPENSTORY_URL (default http://localhost:3002).

Usage:
    uv run python test_tools.py          # run all tests
    uv run python test_tools.py --brief  # one-line-per-tool summary
"""

from __future__ import annotations

import asyncio
import json
import sys

from server import mcp


async def call(name: str, args: dict | None = None) -> object:
    """Call a tool and return parsed JSON."""
    result = await mcp.call_tool(name, args or {})
    return json.loads(result.content[0].text)


async def run_all(brief: bool = False):
    passed = 0
    failed = 0

    async def check(name: str, args: dict | None = None, validate=None):
        nonlocal passed, failed
        try:
            data = await call(name, args)
            if validate:
                validate(data)
            if brief:
                print(f"  PASS  {name}")
            passed += 1
            return data
        except Exception as e:
            print(f"  FAIL  {name}: {e}")
            failed += 1
            return None

    # -- list_sessions (needed for other tests) --
    sessions = await call("list_sessions", {})
    assert len(sessions) > 0, "no sessions found"
    sid = sessions[0]["session_id"]
    project_id = "-Users-maxglassie-projects-OpenStory"
    print(f"Using session: {sid[:16]}  ({len(sessions)} total)")
    passed += 1
    if brief:
        print(f"  PASS  list_sessions")

    if not brief:
        print(f"\n--- Session tools (session={sid[:12]}) ---")

    await check(
        "session_synopsis",
        {"session_id": sid},
        lambda d: assert_keys(d, ["session_id", "event_count", "top_tools"]),
    )

    await check(
        "session_activity",
        {"session_id": sid},
        lambda d: assert_keys(d, ["first_prompt", "tool_breakdown"]),
    )

    await check(
        "tool_journey",
        {"session_id": sid},
        lambda d: assert_type(d, list),
    )

    await check(
        "file_impact",
        {"session_id": sid},
        lambda d: assert_type(d, list),
    )

    await check(
        "session_errors",
        {"session_id": sid},
        lambda d: assert_type(d, list),
    )

    await check(
        "session_patterns",
        {"session_id": sid},
        lambda d: "patterns" in d or isinstance(d, list),
    )

    await check(
        "session_patterns",
        {"session_id": sid, "pattern_type": "turn.sentence"},
    )

    await check(
        "session_transcript",
        {"session_id": sid},
        lambda d: assert_keys(d, ["entries"]),
    )

    await check(
        "session_transcript",
        {"session_id": sid, "assistant_only": True},
        lambda d: assert_keys(d, ["entries"]),
    )

    await check(
        "session_plans",
        {"session_id": sid},
        lambda d: assert_type(d, list),
    )

    if not brief:
        print(f"\n--- Search tools ---")

    await check(
        "search",
        {"query": "test", "limit": 3},
        lambda d: assert_type(d, list),
    )

    await check(
        "agent_search",
        {"query": "token usage", "limit": 3},
        lambda d: assert_keys(d, ["query", "results"]),
    )

    if not brief:
        print(f"\n--- Project tools ---")

    await check(
        "project_context",
        {"project": project_id},
        lambda d: assert_type(d, list),
    )

    await check(
        "recent_files",
        {"project": project_id},
        lambda d: assert_type(d, list),
    )

    if not brief:
        print(f"\n--- Insights tools ---")

    await check(
        "project_pulse",
        {"days": 7},
        lambda d: assert_type(d, list),
    )

    await check(
        "token_usage",
        {"days": 7, "model": "sonnet"},
        lambda d: assert_keys(d, ["total_input", "total_output", "estimated_cost"]),
    )

    await check(
        "daily_token_usage",
        {"days": 3},
        lambda d: assert_type(d, list),
    )

    await check(
        "productivity",
        {"days": 30},
        lambda d: assert_type(d, list),
    )

    print(f"\n{passed} passed, {failed} failed out of {passed + failed} tools")
    return failed == 0


def assert_keys(data: object, keys: list[str]):
    assert isinstance(data, dict), f"expected dict, got {type(data).__name__}"
    for k in keys:
        assert k in data, f"missing key: {k}"


def assert_type(data: object, t: type):
    assert isinstance(data, t), f"expected {t.__name__}, got {type(data).__name__}"


if __name__ == "__main__":
    brief = "--brief" in sys.argv
    ok = asyncio.run(run_all(brief=brief))
    sys.exit(0 if ok else 1)
