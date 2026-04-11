"""Unit tests for MCP server hardening.

Tests the HTTP client lifecycle, error handling, and session_story refactor
WITHOUT needing a live OpenStory backend. Everything httpx is mocked.

Usage:
    uv run pytest test_hardening.py -v
"""

from __future__ import annotations

import asyncio
import json
from unittest.mock import MagicMock, patch

import httpx
import pytest

import server


@pytest.fixture(autouse=True)
def reset_client():
    """Ensure each test starts with a fresh client singleton."""
    server._http = None
    yield
    if server._http is not None:
        server._http.close()
        server._http = None


# ---------------------------------------------------------------------------
# _get_client singleton
# ---------------------------------------------------------------------------

def test_get_client_returns_httpx_client():
    client = server._get_client()
    assert isinstance(client, httpx.Client)


def test_get_client_is_reused():
    c1 = server._get_client()
    c2 = server._get_client()
    assert c1 is c2


def test_get_client_has_transport_retries():
    """HTTPTransport(retries=3) handles connection-level retries automatically."""
    client = server._get_client()
    # httpx stores the transport on ._transport
    transport = client._transport
    assert isinstance(transport, httpx.HTTPTransport)


# ---------------------------------------------------------------------------
# _get error handling
# ---------------------------------------------------------------------------

def _mock_response(status_code: int, json_data: dict | list) -> MagicMock:
    resp = MagicMock(spec=httpx.Response)
    resp.status_code = status_code
    resp.json.return_value = json_data
    if status_code >= 400:
        resp.raise_for_status.side_effect = httpx.HTTPStatusError(
            f"{status_code}", request=MagicMock(), response=resp
        )
    else:
        resp.raise_for_status.return_value = None
    return resp


def test_get_success_returns_parsed_json():
    expected = {"sessions": [{"session_id": "abc"}]}
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.return_value = _mock_response(200, expected)
        mock_client_factory.return_value = mock_client

        result = server._get("/api/sessions")
        assert result == expected


def test_get_http_error_returns_structured_error():
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.return_value = _mock_response(503, {})
        mock_client_factory.return_value = mock_client

        result = server._get("/api/sessions")
        assert isinstance(result, dict)
        assert "error" in result
        assert result["status_code"] == 503


def test_get_connect_error_returns_structured_error():
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.side_effect = httpx.ConnectError("connection refused")
        mock_client_factory.return_value = mock_client

        result = server._get("/api/sessions")
        assert isinstance(result, dict)
        assert "error" in result
        assert result["status_code"] is None
        assert "connect" in result["error"].lower() or server.BASE_URL in result["error"]


def test_get_timeout_returns_structured_error():
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.side_effect = httpx.TimeoutException("timeout")
        mock_client_factory.return_value = mock_client

        result = server._get("/api/sessions")
        assert isinstance(result, dict)
        assert "error" in result
        assert "timed out" in result["error"].lower() or "timeout" in result["error"].lower()


# ---------------------------------------------------------------------------
# session_story — no subprocess
# ---------------------------------------------------------------------------

def _fixture_records() -> list[dict]:
    return [
        {"record_type": "user_message", "timestamp": "2026-04-06T10:00:00Z",
         "payload": {"content": "let's build a thing"}},
        {"record_type": "tool_call", "timestamp": "2026-04-06T10:00:10Z",
         "payload": {"name": "Bash"}},
        {"record_type": "turn_end", "timestamp": "2026-04-06T10:00:30Z", "payload": {}},
    ]


def _fixture_patterns() -> list[dict]:
    return [
        {"pattern_type": "turn.sentence", "summary": "Claude ran Bash"},
        {"pattern_type": "turn.phase", "metadata": {"phase": "implementation"}},
    ]


def test_session_story_no_subprocess():
    """session_story should import summarize(), not spawn a subprocess."""
    records_response = _fixture_records()
    patterns_response = {"patterns": _fixture_patterns()}

    def fake_get(path: str, params=None):
        if "records" in path:
            return records_response
        if "patterns" in path:
            return patterns_response
        return []

    # Patch subprocess.run to blow up if it's called — proves we don't shell out
    with patch.object(server, "_get", side_effect=fake_get), \
         patch("subprocess.run", side_effect=AssertionError("subprocess should not be called")):
        result_json = _call_tool_sync("session_story", {"session_id": "test-session"})
        parsed = json.loads(result_json)

    assert parsed["session_id"] == "test-session"
    assert parsed["total_records"] == 3
    assert parsed["turn_count"] == 1
    assert parsed["tool_call_counts"] == {"Bash": 1}


def test_session_story_propagates_error_from_get():
    """If _get returns an error dict, session_story should surface it cleanly."""
    error_response = {"error": "Cannot connect", "status_code": None}

    with patch.object(server, "_get", return_value=error_response):
        result_json = _call_tool_sync("session_story", {"session_id": "test-session"})
        parsed = json.loads(result_json)

    assert "error" in parsed


# ---------------------------------------------------------------------------
# health_check tool
# ---------------------------------------------------------------------------

def test_health_check_ok():
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.return_value = _mock_response(200, {"sessions": []})
        mock_client_factory.return_value = mock_client

        result_json = _call_tool_sync("health_check", {})
        parsed = json.loads(result_json)
        assert parsed["status"] == "ok"


def test_health_check_unreachable():
    with patch.object(server, "_get_client") as mock_client_factory:
        mock_client = MagicMock()
        mock_client.get.side_effect = httpx.ConnectError("refused")
        mock_client_factory.return_value = mock_client

        result_json = _call_tool_sync("health_check", {})
        parsed = json.loads(result_json)
        assert parsed["status"] == "unreachable"
        assert "error" in parsed


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def _call_tool_sync(name: str, args: dict) -> str:
    """Invoke an MCP tool through FastMCP's dispatch and return its string output."""
    result = asyncio.run(server.mcp.call_tool(name, args))
    return result.content[0].text
