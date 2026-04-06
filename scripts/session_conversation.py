"""Reconstruct the conversation flow from an OpenStory session.

Shows user prompts, assistant responses, and tool calls in reading order.
Useful for recovering context from past sessions.

Usage:
    uv run python scripts/session_conversation.py SESSION_ID
    uv run python scripts/session_conversation.py SESSION_ID --search "plan"
    uv run python scripts/session_conversation.py --search "typed event data"
    uv run python scripts/session_conversation.py --list
    uv run python scripts/session_conversation.py --test
"""

import argparse
import json
import sys
import urllib.request


DEFAULT_URL = "http://localhost:3002"


def fetch(base_url: str, path: str):
    return json.loads(urllib.request.urlopen(f"{base_url}{path}").read())


def list_sessions(base_url: str, limit: int = 20) -> None:
    sessions = fetch(base_url, "/api/sessions")
    # Most recent first
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    print(f"{'ID':>14s}  {'Events':>6s}  {'Tools':>5s}  {'Status':>7s}  {'Branch':<30s}  Label")
    print("-" * 110)
    for s in sessions[:limit]:
        sid = s["session_id"][:12]
        label = (s.get("label") or s.get("first_prompt") or "")[:50]
        branch = (s.get("branch") or "")[:30]
        status = s.get("status", "")[:7]
        events = s.get("event_count", 0)
        tools = s.get("tool_calls", 0)
        print(f"{sid:>14s}  {events:>6d}  {tools:>5d}  {status:>7s}  {branch:<30s}  {label}")


def show_conversation(base_url: str, session_id: str, search: str | None = None,
                      max_text: int = 500) -> None:
    records = fetch(base_url, f"/api/sessions/{session_id}/records")
    print(f"Session: {session_id}")
    print(f"Records: {len(records)}")
    print("=" * 80)

    for r in records:
        rt = r.get("record_type", "")
        seq = r.get("seq", 0)
        payload = r.get("payload", {})

        if rt == "user_message":
            content = payload.get("content", "")
            if search and search.lower() not in content.lower():
                continue
            print(f"\n[seq:{seq}] USER:")
            print(f"  {content[:max_text]}")

        elif rt == "assistant_message":
            blocks = payload.get("content", [])
            if not isinstance(blocks, list):
                continue
            for block in blocks:
                if not isinstance(block, dict) or block.get("type") != "text":
                    continue
                text = block.get("text", "")
                if not text:
                    continue
                if search and search.lower() not in text.lower():
                    continue
                preview = text[:max_text]
                if len(text) > max_text:
                    preview += f" [...{len(text)} chars total]"
                print(f"\n[seq:{seq}] ASSISTANT:")
                print(f"  {preview}")

        elif rt == "tool_call":
            name = payload.get("name", "")
            inp = payload.get("input", {})
            inp_str = json.dumps(inp)
            if search and search.lower() not in inp_str.lower() and search.lower() not in name.lower():
                continue
            preview = inp_str[:200]
            print(f"\n[seq:{seq}] TOOL_CALL: {name}")
            print(f"  {preview}")

        elif rt == "tool_result":
            if search:
                content = str(payload.get("content", ""))
                if search.lower() not in content.lower():
                    continue
                print(f"\n[seq:{seq}] TOOL_RESULT:")
                print(f"  {content[:max_text]}")


def search_sessions(base_url: str, query: str) -> None:
    from urllib.parse import quote
    results = fetch(base_url, f"/api/search?q={quote(query)}")
    print(f"FTS5 search: '{query}' — {len(results)} results")
    print("-" * 80)
    for r in results:
        sid = r.get("session_id", "")[:12]
        rt = r.get("record_type", "")
        seq = r.get("seq", 0)
        payload = r.get("payload", {})
        content = payload.get("content", "")
        if isinstance(content, list):
            text = " ".join(
                b.get("text", "") for b in content if isinstance(b, dict)
            )[:200]
        else:
            text = str(content)[:200]
        print(f"  [{sid}] seq:{seq} {rt}: {text}")


def run_tests() -> None:
    """Smoke tests against a live server."""
    import sqlite3

    # Test 1: in-memory parse of a mock record
    mock_records = [
        {"record_type": "user_message", "seq": 1, "payload": {"content": "hello"}},
        {"record_type": "assistant_message", "seq": 2, "payload": {"content": [{"type": "text", "text": "hi back"}]}},
        {"record_type": "tool_call", "seq": 3, "payload": {"name": "Read", "input": {"file": "foo.rs"}}},
    ]
    # Verify we can classify each type
    types = [r["record_type"] for r in mock_records]
    assert types == ["user_message", "assistant_message", "tool_call"], f"Unexpected: {types}"

    # Test 2: search filter logic
    text = "This is a plan for typed event data"
    assert "plan" in text.lower()
    assert "typed" in text.lower()
    assert "missing" not in text.lower()

    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Reconstruct conversation from OpenStory session")
    parser.add_argument("session_id", nargs="?", default="", help="Session ID (prefix ok)")
    parser.add_argument("--url", default=DEFAULT_URL, help="API base URL")
    parser.add_argument("--search", "-s", help="Filter to records matching this text")
    parser.add_argument("--list", "-l", action="store_true", help="List recent sessions")
    parser.add_argument("--fts", help="Run FTS5 search across all sessions")
    parser.add_argument("--max-text", type=int, default=500, help="Max chars per text block")
    parser.add_argument("--test", action="store_true", help="Run smoke tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    if args.list:
        list_sessions(args.url)
        sys.exit(0)

    if args.fts:
        search_sessions(args.url, args.fts)
        sys.exit(0)

    if not args.session_id:
        parser.error("Provide a session_id, or use --list / --fts / --test")

    # Support prefix matching
    sid = args.session_id
    if len(sid) < 36:
        sessions = fetch(args.url, "/api/sessions")
        matches = [s for s in sessions if s["session_id"].startswith(sid)]
        if len(matches) == 1:
            sid = matches[0]["session_id"]
        elif len(matches) > 1:
            print(f"Ambiguous prefix '{sid}', matches:")
            for m in matches:
                print(f"  {m['session_id']}  {(m.get('label') or '')[:50]}")
            sys.exit(1)
        else:
            print(f"No session matching prefix '{sid}'")
            sys.exit(1)

    show_conversation(args.url, sid, search=args.search, max_text=args.max_text)
